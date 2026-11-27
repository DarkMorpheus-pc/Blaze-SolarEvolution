// ==============================================================================
// BlazeAI Turkish Voice Pipeline (whisper.cpp + Piper TTS + PipeWire) & Vision
// - Audio Input (Kulak): /usr/bin/pw-record (16kHz mono s16) -> whisper-cli
// - Audio Output (Agiz): Sentence-streamed Piper TTS (tr_TR-dfki-medium /
//   tr_TR-fahrettin-medium) -> /usr/bin/pw-play with instant barge-in cancel
// - On-demand unprivileged downloads under ~/.local/share/solarui/blazeai/voice/
// - Explicit user-triggered Wayland Vision capture via /usr/bin/grim + Base64
// ==============================================================================

use crate::blazeai::LocalMemoryStore;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static VOICE_SEQ: AtomicU64 = AtomicU64::new(1);

pub const PIPER_VOICE_PERSEPHONE: &str = "tr_TR-dfki-medium";
pub const PIPER_VOICE_KOZ: &str = "tr_TR-fahrettin-medium";

pub fn voice_root_dir() -> PathBuf {
    LocalMemoryStore::default_data_dir().join("voice")
}

pub fn piper_binary_path() -> Option<PathBuf> {
    let user_bin = voice_root_dir().join("piper/piper");
    if user_bin.is_file() {
        return Some(user_bin);
    }
    for sys in ["/usr/bin/piper", "/usr/local/bin/piper", "/usr/bin/piper-tts"] {
        let p = PathBuf::from(sys);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn whisper_binary_path() -> Option<PathBuf> {
    let user_bin = voice_root_dir().join("bin/whisper-cli");
    if user_bin.is_file() {
        return Some(user_bin);
    }
    for sys in ["/usr/bin/whisper-cli", "/usr/local/bin/whisper-cli", "/usr/bin/whisper-cpp"] {
        let p = PathBuf::from(sys);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn piper_voice_model_path(voice_id: &str) -> PathBuf {
    let safe = sanitize_voice_id(voice_id);
    voice_root_dir().join(format!("voices/{}.onnx", safe))
}

pub fn whisper_model_path(model_file: &str) -> PathBuf {
    let safe = model_file
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect::<String>();
    let name = if safe.ends_with(".bin") {
        safe
    } else {
        "ggml-base.bin".to_string()
    };
    voice_root_dir().join("models").join(name)
}

fn sanitize_voice_id(voice_id: &str) -> String {
    match voice_id.trim() {
        PIPER_VOICE_KOZ => PIPER_VOICE_KOZ.to_string(),
        _ => PIPER_VOICE_PERSEPHONE.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoicePackStatus {
    pub piper_ready: bool,
    pub persephone_voice_ready: bool,
    pub koz_voice_ready: bool,
    pub whisper_ready: bool,
    pub whisper_model_ready: bool,
}

impl VoicePackStatus {
    pub fn probe(stt_model: &str) -> Self {
        Self {
            piper_ready: piper_binary_path().is_some(),
            persephone_voice_ready: piper_voice_model_path(PIPER_VOICE_PERSEPHONE).is_file(),
            koz_voice_ready: piper_voice_model_path(PIPER_VOICE_KOZ).is_file(),
            whisper_ready: whisper_binary_path().is_some(),
            whisper_model_ready: whisper_model_path(stt_model).is_file(),
        }
    }

    pub fn tts_ready_for(&self, voice_id: &str) -> bool {
        self.piper_ready && piper_voice_model_path(voice_id).is_file()
    }

    pub fn stt_ready(&self) -> bool {
        self.whisper_ready && self.whisper_model_ready
    }
}

fn download_file_cancelable(url: &str, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    if !(url.starts_with("https://github.com/")
        || url.starts_with("https://huggingface.co/"))
    {
        return Err("Yalnızca resmi GitHub veya HuggingFace kaynaklarına izin verilir".to_string());
    }
    if let Some(parent) = dest.parent() {
        let _ = fs::create_dir_all(parent);
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
    }
    let tmp = dest.with_extension("download.tmp");
    let _ = fs::remove_file(&tmp);

    let mut child = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "600", "-o"])
        .arg(&tmp)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("İndirme başlatılamadı: {}", e))?;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&tmp);
            return Err("İndirme iptal edildi".to_string());
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                if !st.success() {
                    let _ = fs::remove_file(&tmp);
                    return Err(format!("Dosya indirilemedi: {}", url));
                }
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(180)),
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(format!("İndirme hatası: {}", e));
            }
        }
    }

    fs::rename(&tmp, dest).map_err(|e| format!("Dosya kaydedilemedi: {}", e))?;
    Ok(())
}

/// Downloads Piper TTS Linux x86_64 binary and both official Turkish ONNX voices
/// (`tr_TR-dfki-medium` for Persephone and `tr_TR-fahrettin-medium` for Köz).
pub fn install_turkish_tts_pack<F>(cancel: &AtomicBool, on_status: F) -> Result<(), String>
where
    F: Fn(&str),
{
    let root = voice_root_dir();
    let _ = fs::create_dir_all(&root);
    let _ = fs::set_permissions(&root, fs::Permissions::from_mode(0o700));

    if piper_binary_path().is_none() {
        on_status("Piper TTS motoru indiriliyor (~25 MB)...");
        let tar_path = root.join("piper_linux_x86_64.tar.gz");
        download_file_cancelable(
            "https://github.com/rhasspy/piper/releases/latest/download/piper_linux_x86_64.tar.gz",
            &tar_path,
            cancel,
        )?;
        on_status("Piper TTS açılıyor...");
        let st = Command::new("/usr/bin/tar")
            .args(["-xzf"])
            .arg(&tar_path)
            .arg("-C")
            .arg(&root)
            .status()
            .map_err(|e| format!("tar hatası: {}", e))?;
        let _ = fs::remove_file(&tar_path);
        if !st.success() {
            return Err("Piper TTS arşivi açılamadı".to_string());
        }
    }

    let voice_specs = [
        (
            PIPER_VOICE_PERSEPHONE,
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/tr/tr_TR/dfki/medium/tr_TR-dfki-medium.onnx",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/tr/tr_TR/dfki/medium/tr_TR-dfki-medium.onnx.json",
            "Persephone Türkçe sesi indiriliyor (tr_TR-dfki-medium)...",
        ),
        (
            PIPER_VOICE_KOZ,
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/tr/tr_TR/fahrettin/medium/tr_TR-fahrettin-medium.onnx",
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/tr/tr_TR/fahrettin/medium/tr_TR-fahrettin-medium.onnx.json",
            "Köz Türkçe sesi indiriliyor (tr_TR-fahrettin-medium)...",
        ),
    ];

    for (vid, onnx_url, json_url, msg) in voice_specs {
        let onnx_dest = piper_voice_model_path(vid);
        let json_dest = onnx_dest.with_extension("onnx.json");
        if !onnx_dest.is_file() {
            on_status(msg);
            download_file_cancelable(onnx_url, &onnx_dest, cancel)?;
        }
        if !json_dest.is_file() {
            download_file_cancelable(json_url, &json_dest, cancel)?;
        }
    }
    on_status("Türkçe ses paketi hazır");
    Ok(())
}

/// Downloads Whisper `ggml-base.bin` model for Turkish speech-to-text.
pub fn install_whisper_stt_model<F>(
    stt_model: &str,
    cancel: &AtomicBool,
    on_status: F,
) -> Result<(), String>
where
    F: Fn(&str),
{
    let dest = whisper_model_path(stt_model);
    if dest.is_file() {
        return Ok(());
    }
    let fname = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "ggml-base.bin".to_string());
    let url = format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        fname
    );
    on_status(&format!("Whisper Türkçe dinleme modeli indiriliyor ({})...", fname));
    download_file_cancelable(&url, &dest, cancel)?;
    on_status("Whisper ses tanıma modeli hazır");
    Ok(())
}

/// Cleans markdown markup and splits text into natural sentences for low-latency TTS streaming.
pub fn split_into_speakable_sentences(text: &str) -> Vec<String> {
    let mut clean = String::with_capacity(text.len());
    let mut in_code_fence = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        let stripped = t
            .trim_start_matches('#')
            .trim_start_matches('-')
            .trim_start_matches('*')
            .replace("**", "")
            .replace("__", "")
            .replace('`', "");
        if !stripped.trim().is_empty() {
            clean.push_str(stripped.trim());
            clean.push('\n');
        }
    }

    let mut sentences = Vec::new();
    let mut buf = String::new();
    for ch in clean.chars() {
        buf.push(ch);
        if matches!(ch, '.' | '!' | '?' | '\n') && buf.trim().chars().count() >= 12 {
            let s = buf.split_whitespace().collect::<Vec<_>>().join(" ");
            if !s.is_empty() {
                sentences.push(s);
            }
            buf.clear();
        }
    }
    let rem = buf.split_whitespace().collect::<Vec<_>>().join(" ");
    if !rem.is_empty() {
        sentences.push(rem);
    }
    sentences
}

#[derive(Clone)]
pub struct VoiceSpeaker {
    cancel: Arc<AtomicBool>,
    active_pids: Arc<Mutex<Vec<u32>>>,
}

impl Default for VoiceSpeaker {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceSpeaker {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            active_pids: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Instant barge-in: immediately halts any ongoing Piper TTS + PipeWire playback.
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if let Ok(mut pids) = self.active_pids.lock() {
            for pid in pids.drain(..) {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }
    }

    pub fn speak_async(
        &self,
        text: String,
        voice_id: String,
        on_done: impl FnOnce() + Send + 'static,
    ) -> Result<(), String> {
        let Some(piper_bin) = piper_binary_path() else {
            return Err("Piper TTS kurulu değil (Ayarlar -> Ses bölümünden indirin)".to_string());
        };
        let model_path = piper_voice_model_path(&voice_id);
        if !model_path.is_file() {
            return Err(format!(
                "Türkçe ses modeli bulunamadı ({}); Ayarlar -> Ses bölümünden indirin",
                voice_id
            ));
        }

        self.stop();
        let cancel = Arc::new(AtomicBool::new(false));
        let prev_cancel = self.cancel.clone();
        prev_cancel.store(false, Ordering::SeqCst);
        let shared_cancel = self.cancel.clone();
        let pids_arc = self.active_pids.clone();
        let sentences = split_into_speakable_sentences(&text);
        let _ = cancel;

        std::thread::spawn(move || {
            for sentence in sentences {
                if shared_cancel.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut piper) = Command::new(&piper_bin)
                    .args(["--model"])
                    .arg(&model_path)
                    .arg("--output-raw")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    break;
                };
                let piper_pid = piper.id();
                let Some(piper_stdout) = piper.stdout.take() else {
                    let _ = piper.kill();
                    break;
                };
                let Ok(mut play) = Command::new("/usr/bin/pw-play")
                    .args(["--rate", "22050", "--channels", "1", "--format", "s16", "-"])
                    .stdin(Stdio::from(piper_stdout))
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                else {
                    let _ = piper.kill();
                    break;
                };
                let play_pid = play.id();
                if let Ok(mut guard) = pids_arc.lock() {
                    guard.push(piper_pid);
                    guard.push(play_pid);
                }
                if let Some(mut stdin) = piper.stdin.take() {
                    let _ = stdin.write_all(sentence.as_bytes());
                    let _ = stdin.write_all(b"\n");
                }
                let _ = piper.wait();
                let _ = play.wait();
                if let Ok(mut guard) = pids_arc.lock() {
                    guard.retain(|p| *p != piper_pid && *p != play_pid);
                }
            }
            on_done();
        });
        Ok(())
    }
}

pub struct ActiveRecording {
    child: Child,
    wav_path: PathBuf,
}

impl ActiveRecording {
    /// Begins 16 kHz mono s16 PipeWire capture into a private `0600` runtime WAV file.
    pub fn start() -> Result<Self, String> {
        let rt_dir = solar_common::get_solar_runtime_dir().join("blazeai");
        let _ = fs::create_dir_all(&rt_dir);
        let _ = fs::set_permissions(&rt_dir, fs::Permissions::from_mode(0o700));
        let seq = VOICE_SEQ.fetch_add(1, Ordering::Relaxed);
        let wav_path = rt_dir.join(format!("rec-{}-{}.wav", std::process::id(), seq));

        let child = Command::new("/usr/bin/pw-record")
            .args(["--rate", "16000", "--channels", "1", "--format", "s16"])
            .arg(&wav_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Mikrofon kaydı (pw-record) başlatılamadı: {}", e))?;

        Ok(Self { child, wav_path })
    }

    pub fn stop_and_transcribe(mut self, stt_model: &str) -> Result<String, String> {
        let pid = self.child.id() as i32;
        unsafe {
            libc::kill(pid, libc::SIGINT);
        }
        std::thread::sleep(Duration::from_millis(120));
        let _ = self.child.kill();
        let _ = self.child.wait();

        let wav = self.wav_path.clone();
        let cleanup = || {
            let _ = fs::remove_file(&wav);
        };

        let Some(whisper_bin) = whisper_binary_path() else {
            cleanup();
            return Err("whisper-cli bulunamadı (yerel ses tanıma için whisper.cpp kurulu olmalı)".to_string());
        };
        let model = whisper_model_path(stt_model);
        if !model.is_file() {
            cleanup();
            return Err("Whisper Türkçe modeli indirilmemiş (Ayarlar -> Ses bölümünden indirin)".to_string());
        }

        let out = Command::new(&whisper_bin)
            .args(["-m"])
            .arg(&model)
            .args(["-f"])
            .arg(&wav)
            .args(["-l", "tr", "--no-timestamps"])
            .output()
            .map_err(|e| {
                cleanup();
                format!("whisper-cli çalıştırılamadı: {}", e)
            })?;
        cleanup();

        if !out.status.success() {
            return Err("Ses metne dönüştürülemedi".to_string());
        }
        let text = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty() && !l.starts_with('['))
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            Err("Ses algılanamadı".to_string())
        } else {
            Ok(text)
        }
    }

    pub fn cancel(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.wav_path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisionAttachment {
    pub mime_type: String,
    pub base64_data: String,
    pub width: i32,
    pub height: i32,
}

/// Captures a single explicit Wayland screenshot via `/usr/bin/grim`, downscales to <= 1280px wide,
/// encodes to Base64, and immediately deletes the temporary file from disk.
pub fn capture_wayland_screen_for_vision() -> Result<VisionAttachment, String> {
    let rt_dir = solar_common::get_solar_runtime_dir().join("blazeai");
    let _ = fs::create_dir_all(&rt_dir);
    let _ = fs::set_permissions(&rt_dir, fs::Permissions::from_mode(0o700));
    let seq = VOICE_SEQ.fetch_add(1, Ordering::Relaxed);
    let raw_png = rt_dir.join(format!("vision-{}-{}.png", std::process::id(), seq));

    let st = Command::new("/usr/bin/grim")
        .arg(&raw_png)
        .status()
        .map_err(|e| format!("Ekran yakalama (grim) başlatılamadı: {}", e))?;
    if !st.success() || !raw_png.is_file() {
        let _ = fs::remove_file(&raw_png);
        return Err("Wayland ekran görüntüsü yakalanamadı".to_string());
    }

    let res = (|| -> Result<VisionAttachment, String> {
        let pixbuf = gdk_pixbuf::Pixbuf::from_file_at_scale(&raw_png, 1280, 720, true)
            .map_err(|e| format!("Ekran görüntüsü ölçeklenemedi: {}", e))?;
        let width = pixbuf.width();
        let height = pixbuf.height();
        let jpeg_bytes = pixbuf
            .save_to_bufferv("jpeg", &[("quality", "82")])
            .map_err(|e| format!("JPEG kodlama hatası: {}", e))?;
        Ok(VisionAttachment {
            mime_type: "image/jpeg".to_string(),
            base64_data: base64_encode(&jpeg_bytes),
            width,
            height,
        })
    })();
    let _ = fs::remove_file(&raw_png);
    res
}

pub fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentence_splitter_cleans_markdown_and_skips_code_blocks() {
        let sample = "## Özet\n**BlazeOS** hazır. Şimdi test edebilirsin!\n```bash\necho gizli\n```\nHarika görünüyor?";
        let parts = split_into_speakable_sentences(sample);
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "Özet BlazeOS hazır.");
        assert_eq!(parts[1], "Şimdi test edebilirsin!");
        assert_eq!(parts[2], "Harika görünüyor?");
        assert!(!parts.iter().any(|s| s.contains("echo gizli")));
    }

    #[test]
    fn base64_encoder_matches_rfc4648_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"BlazeOS"), "QmxhemVPUw==");
    }
}
