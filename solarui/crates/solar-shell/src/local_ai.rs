// ==============================================================================
// BlazeAI Local Model Hub & Hardware Tier Recommender
// - Probes host RAM (/proc/meminfo) and GPU/VRAM (nvidia-smi, DRM sysfs, lspci)
// - Recommends optimal open-source tier (1.5B-2B, 4B, 8B-9B, 12B, 14B-16B)
// - Manages unprivileged user-space Ollama runtime bound strictly to 127.0.0.1
// - Streams NDJSON model pulls with real-time progress, cancellation, and delete
// ==============================================================================

use crate::blazeai::{probe_ollama_models, validate_http_url, LocalMemoryStore};
use std::fs;
use std::io::{BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalModelTier {
    pub tier_id: &'static str,
    pub label_tr: &'static str,
    pub primary_tag: &'static str,
    pub alt_tag: &'static str,
    pub size_hint: &'static str,
    pub min_ram_gb: u32,
    pub supports_vision: bool,
    pub supports_thinking: bool,
    pub desc_tr: &'static str,
}

pub const LOCAL_MODEL_TIERS: &[LocalModelTier] = &[
    LocalModelTier {
        tier_id: "2b",
        label_tr: "1.5B–2B · Ultra Hafif",
        primary_tag: "qwen3.5:2b",
        alt_tag: "qwen2.5:1.5b",
        size_hint: "~1.8 GB",
        min_ram_gb: 4,
        supports_vision: true,
        supports_thinking: true,
        desc_tr: "Eski veya harici GPU'suz sistemler için CPU'da bile akıcı",
    },
    LocalModelTier {
        tier_id: "4b",
        label_tr: "4B · Dengeli Hafif",
        primary_tag: "qwen3.5:4b",
        alt_tag: "gemma4:e2b",
        size_hint: "~3.2 GB",
        min_ram_gb: 8,
        supports_vision: true,
        supports_thinking: true,
        desc_tr: "8 GB RAM laptoplarda hızlı Türkçe yanıt, görsel ve ajan desteği",
    },
    LocalModelTier {
        tier_id: "8b",
        label_tr: "8B–9B · Önerilen",
        primary_tag: "qwen3.5:9b",
        alt_tag: "qwen3:8b",
        size_hint: "~6.0 GB",
        min_ram_gb: 16,
        supports_vision: true,
        supports_thinking: true,
        desc_tr: "BlazeOS varsayılan önerisi: hibrit düşünme, ekran okuma ve araçlar",
    },
    LocalModelTier {
        tier_id: "12b",
        label_tr: "12B · Yüksek Türkçe",
        primary_tag: "gemma4:12b",
        alt_tag: "gemma3:12b",
        size_hint: "~8.0 GB",
        min_ram_gb: 16,
        supports_vision: true,
        supports_thinking: true,
        desc_tr: "Güçlü GPU (12 GB VRAM) için akıcı Türkçe ve çok modlu analiz",
    },
    LocalModelTier {
        tier_id: "14b",
        label_tr: "14B–16B · İş İstasyonu",
        primary_tag: "qwen3:14b",
        alt_tag: "deepseek-r1:14b",
        size_hint: "~9.5 GB",
        min_ram_gb: 24,
        supports_vision: false,
        supports_thinking: true,
        desc_tr: "İş istasyonu / 16 GB VRAM için derin kodlama ve araştırma",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareProfile {
    pub ram_mb: u64,
    pub vram_mb: u64,
    pub gpu_name: String,
    pub recommended_tier_id: &'static str,
}

pub fn recommend_tier_for_hardware(ram_mb: u64, vram_mb: u64) -> &'static str {
    if vram_mb >= 14_000 {
        "14b"
    } else if vram_mb >= 10_000 {
        "12b"
    } else if vram_mb >= 6_000 || ram_mb >= 14_000 {
        "8b"
    } else if vram_mb >= 3_500 || ram_mb >= 7_500 {
        "4b"
    } else {
        "2b"
    }
}

pub fn tier_by_id(tier_id: &str) -> Option<&'static LocalModelTier> {
    LOCAL_MODEL_TIERS.iter().find(|t| t.tier_id == tier_id)
}

pub fn model_supports_vision(model_name: &str) -> bool {
    let lower = model_name.to_ascii_lowercase();
    lower.contains("qwen3.5")
        || lower.contains("gemma4")
        || lower.contains("gemma3")
        || lower.contains("vl")
        || lower.contains("vision")
        || lower.contains("llava")
        || lower.contains("minicpm")
        || lower.contains("gemini")
        || lower.contains("gpt-4o")
        || lower.contains("gpt-4.1")
        || lower.contains("gpt-5")
        || lower.contains("claude")
}

pub fn model_supports_thinking(model_name: &str) -> bool {
    let lower = model_name.to_ascii_lowercase();
    lower.contains("qwen3")
        || lower.contains("gemma4")
        || lower.contains("deepseek-r1")
        || lower.contains("qwq")
        || lower.contains("thinking")
}

impl HardwareProfile {
    pub fn probe() -> Self {
        let ram_mb = read_total_ram_mb();
        let (gpu_name, vram_mb) = detect_gpu_and_vram();
        let recommended_tier_id = recommend_tier_for_hardware(ram_mb, vram_mb);
        Self {
            ram_mb,
            vram_mb,
            gpu_name,
            recommended_tier_id,
        }
    }

    pub fn summary_tr(&self) -> String {
        let ram_gb = (self.ram_mb + 512) / 1024;
        let rec_label = tier_by_id(self.recommended_tier_id)
            .map(|t| t.primary_tag)
            .unwrap_or("qwen3.5:9b");
        if self.vram_mb >= 1024 && !self.gpu_name.is_empty() {
            let vram_gb = (self.vram_mb + 512) / 1024;
            format!(
                "{} GB RAM · {} ({} GB VRAM) · Önerilen: {}",
                ram_gb, self.gpu_name, vram_gb, rec_label
            )
        } else if !self.gpu_name.is_empty() {
            format!(
                "{} GB RAM · {} · Önerilen: {}",
                ram_gb, self.gpu_name, rec_label
            )
        } else {
            format!("{} GB RAM · Önerilen: {}", ram_gb, rec_label)
        }
    }
}

fn read_total_ram_mb() -> u64 {
    let Ok(meminfo) = fs::read_to_string("/proc/meminfo") else {
        return 8192;
    };
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(8_388_608);
            return kb / 1024;
        }
    }
    8192
}

fn detect_gpu_and_vram() -> (String, u64) {
    // 1. NVIDIA proprietary driver (nvidia-smi)
    if let Ok(out) = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(first) = text.lines().next() {
                if let Some((name, mem)) = first.split_once(',') {
                    let vram_mb = mem.trim().parse::<u64>().unwrap_or(0);
                    let clean_name = name
                        .trim()
                        .replace("NVIDIA Corporation ", "")
                        .replace("NVIDIA ", "");
                    if !clean_name.is_empty() {
                        return (clean_name, vram_mb);
                    }
                }
            }
        }
    }

    // 2. AMD / DRM sysfs VRAM readout
    let mut sysfs_vram_mb = 0u64;
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let vram_path = entry.path().join("device/mem_info_vram_total");
            if let Ok(raw) = fs::read_to_string(vram_path) {
                if let Ok(bytes) = raw.trim().parse::<u64>() {
                    sysfs_vram_mb = sysfs_vram_mb.max(bytes / (1024 * 1024));
                }
            }
        }
    }

    // 3. lspci fallback for GPU display name (prefers discrete NVIDIA/AMD over integrated Intel)
    if let Ok(out) = Command::new("lspci").env("LC_ALL", "C").output() {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut best_gpu = String::new();
            for line in text.lines() {
                let lower = line.to_ascii_lowercase();
                if !(lower.contains("vga compatible") || lower.contains("3d controller")) {
                    continue;
                }
                let desc = line.split_once(": ").map(|(_, d)| d).unwrap_or(line).trim();
                if lower.contains("nvidia") || lower.contains("amd") || lower.contains("radeon") {
                    best_gpu = compact_gpu_name(desc);
                    break;
                }
                if best_gpu.is_empty() {
                    best_gpu = compact_gpu_name(desc);
                }
            }
            if !best_gpu.is_empty() {
                return (best_gpu, sysfs_vram_mb);
            }
        }
    }

    (String::new(), sysfs_vram_mb)
}

fn compact_gpu_name(raw: &str) -> String {
    if let Some(start) = raw.find('[') {
        if let Some(end) = raw[start + 1..].find(']') {
            let inner = raw[start + 1..start + 1 + end].trim();
            if !inner.is_empty() {
                return inner.chars().take(34).collect();
            }
        }
    }
    raw.chars().take(34).collect()
}

pub fn ollama_runtime_dir() -> PathBuf {
    LocalMemoryStore::default_data_dir().join("runtime/ollama")
}

pub fn find_ollama_binary() -> Option<PathBuf> {
    let user_bin = ollama_runtime_dir().join("bin/ollama");
    if user_bin.is_file() {
        return Some(user_bin);
    }
    for candidate in ["/usr/bin/ollama", "/usr/local/bin/ollama"] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let local_bin = PathBuf::from(home).join(".local/bin/ollama");
        if local_bin.is_file() {
            return Some(local_bin);
        }
    }
    None
}

pub fn is_ollama_serving(base_url: &str) -> bool {
    probe_ollama_models(base_url).is_ok()
}

/// Starts user-space `ollama serve` bound strictly to `127.0.0.1:11434` if not already listening.
pub fn ensure_ollama_serving(base_url: &str) -> Result<(), String> {
    if is_ollama_serving(base_url) {
        return Ok(());
    }
    let Some(bin) = find_ollama_binary() else {
        return Err("Ollama motoru kurulu değil".to_string());
    };
    let models_dir = LocalMemoryStore::default_data_dir().join("ollama-models");
    let _ = fs::create_dir_all(&models_dir);
    let _ = fs::set_permissions(&models_dir, fs::Permissions::from_mode(0o700));

    let lib_dir = ollama_runtime_dir().join("lib/ollama");
    let mut cmd = Command::new(&bin);
    cmd.arg("serve")
        .env("OLLAMA_HOST", "127.0.0.1:11434")
        .env("OLLAMA_MODELS", &models_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if lib_dir.is_dir() {
        let existing_ld = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        let next_ld = if existing_ld.is_empty() {
            lib_dir.to_string_lossy().to_string()
        } else {
            format!("{}:{}", lib_dir.to_string_lossy(), existing_ld)
        };
        cmd.env("LD_LIBRARY_PATH", next_ld);
    }
    cmd.spawn()
        .map_err(|e| format!("Yerel Ollama servisi başlatılamadı: {}", e))?;

    for _ in 0..25 {
        std::thread::sleep(Duration::from_millis(200));
        if is_ollama_serving(base_url) {
            return Ok(());
        }
    }
    Err("Yerel Ollama servisi zamanında yanıt vermedi (127.0.0.1:11434)".to_string())
}

/// Resolves the latest official Linux x86_64 Ollama release archive URL and optional SHA-256 digest.
pub fn resolve_official_ollama_release() -> Result<(String, Option<String>), String> {
    let out = Command::new("/usr/bin/curl")
        .args([
            "-fsSL",
            "--max-time",
            "15",
            "-A",
            "BlazeOS-SolarUI/5.05",
            "https://api.github.com/repos/ollama/ollama/releases/latest",
        ])
        .output()
        .map_err(|e| format!("Ollama sürüm bilgisi alınamadı: {}", e))?;
    if !out.status.success() {
        return Err("GitHub üzerinden Ollama sürüm listesi alınamadı".to_string());
    }
    let val: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| "Ollama sürüm JSON yanıtı ayrıştırılamadı".to_string())?;
    let assets = val
        .get("assets")
        .and_then(|a| a.as_array())
        .ok_or_else(|| "Ollama sürüm dosyaları bulunamadı".to_string())?;

    for preferred_name in ["ollama-linux-amd64.tar.zst", "ollama-linux-amd64.tgz"] {
        for asset in assets {
            if asset.get("name").and_then(|n| n.as_str()) == Some(preferred_name) {
                if let Some(url) = asset
                    .get("browser_download_url")
                    .and_then(|u| u.as_str())
                    .filter(|u| u.starts_with("https://github.com/ollama/ollama/releases/"))
                {
                    let digest = asset
                        .get("digest")
                        .and_then(|d| d.as_str())
                        .and_then(|d| d.strip_prefix("sha256:"))
                        .map(|s| s.to_ascii_lowercase());
                    return Ok((url.to_string(), digest));
                }
            }
        }
    }
    Err("x86_64 Linux için resmi Ollama arşivi bulunamadı".to_string())
}

fn verify_sha256_file(path: &Path, expected_hex: &str) -> Result<(), String> {
    let out = Command::new("/usr/bin/sha256sum")
        .arg(path)
        .output()
        .map_err(|e| format!("sha256sum çalıştırılamadı: {}", e))?;
    if !out.status.success() {
        return Err("SHA-256 doğrulama başarısız".to_string());
    }
    let actual = String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if actual != expected_hex.to_ascii_lowercase() {
        return Err("İndirilen arşivin SHA-256 özeti eşleşmedi".to_string());
    }
    Ok(())
}

/// Downloads and extracts official user-space Ollama into `~/.local/share/solarui/blazeai/runtime/ollama`
/// without requiring root privileges.
pub fn install_userspace_ollama<F>(cancel: &AtomicBool, on_status: F) -> Result<PathBuf, String>
where
    F: Fn(&str),
{
    if let Some(existing) = find_ollama_binary() {
        return Ok(existing);
    }
    on_status("Resmi Ollama sürümü sorgulanıyor...");
    let (url, expected_sha) = resolve_official_ollama_release()?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Kurulum iptal edildi".to_string());
    }

    let rt_dir = ollama_runtime_dir();
    let _ = fs::create_dir_all(&rt_dir);
    let _ = fs::set_permissions(&rt_dir, fs::Permissions::from_mode(0o700));
    let is_zst = url.ends_with(".tar.zst");
    let archive_path = rt_dir.join(if is_zst {
        "ollama-download.tar.zst"
    } else {
        "ollama-download.tgz"
    });
    let _ = fs::remove_file(&archive_path);

    on_status("Ollama motoru indiriliyor (kullanıcı alanı, sudo gerektirmez)...");
    let mut child = Command::new("/usr/bin/curl")
        .args(["-fsSL", "--max-time", "900", "-o"])
        .arg(&archive_path)
        .arg(&url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl başlatılamadı: {}", e))?;

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_file(&archive_path);
            return Err("İndirme iptal edildi".to_string());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let _ = fs::remove_file(&archive_path);
                    return Err("Ollama arşivi indirilemedi".to_string());
                }
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(e) => {
                let _ = fs::remove_file(&archive_path);
                return Err(format!("İndirme izlenemedi: {}", e));
            }
        }
    }

    if let Some(ref sha) = expected_sha {
        on_status("SHA-256 bütünlüğü doğrulanıyor...");
        if let Err(e) = verify_sha256_file(&archive_path, sha) {
            let _ = fs::remove_file(&archive_path);
            return Err(e);
        }
    }

    on_status("Ollama motoru açılıyor...");
    let mut tar_cmd = Command::new("/usr/bin/tar");
    if is_zst {
        tar_cmd.arg("--zstd");
    }
    let tar_status = tar_cmd
        .arg("-xf")
        .arg(&archive_path)
        .arg("-C")
        .arg(&rt_dir)
        .status()
        .map_err(|e| format!("tar çalıştırılamadı: {}", e))?;
    let _ = fs::remove_file(&archive_path);
    if !tar_status.success() {
        return Err("Ollama arşivi açılamadı".to_string());
    }

    let bin = rt_dir.join("bin/ollama");
    if !bin.is_file() {
        return Err("Ollama ikilisi arşiv içinde bulunamadı".to_string());
    }
    let _ = fs::set_permissions(&bin, fs::Permissions::from_mode(0o755));
    Ok(bin)
}

#[derive(Debug, Clone, PartialEq)]
pub struct PullProgress {
    pub status: String,
    pub completed: u64,
    pub total: u64,
    pub fraction: Option<f64>,
    pub done: bool,
    pub error: Option<String>,
}

pub fn validate_ollama_model_tag(tag: &str) -> Result<String, String> {
    let trimmed = tag.trim();
    if trimmed.is_empty() || trimmed.len() > 96 {
        return Err("Geçersiz yerel model etiketi".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ':' | '.' | '-' | '_' | '/'))
    {
        return Err("Model etiketinde geçersiz karakter var".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn parse_ollama_pull_line(line: &str) -> Option<PullProgress> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let val: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    if let Some(err) = val.get("error").and_then(|e| e.as_str()) {
        return Some(PullProgress {
            status: "Hata".to_string(),
            completed: 0,
            total: 0,
            fraction: None,
            done: true,
            error: Some(err.to_string()),
        });
    }
    let status = val
        .get("status")
        .and_then(|s| s.as_str())
        .unwrap_or("İndiriliyor")
        .to_string();
    let completed = val.get("completed").and_then(|v| v.as_u64()).unwrap_or(0);
    let total = val.get("total").and_then(|v| v.as_u64()).unwrap_or(0);
    let fraction = if total > 0 {
        Some((completed as f64 / total as f64).clamp(0.0, 1.0))
    } else if status == "success" {
        Some(1.0)
    } else {
        None
    };
    let done = status == "success";
    Some(PullProgress {
        status,
        completed,
        total,
        fraction,
        done,
        error: None,
    })
}

/// Streams a model download from local Ollama `/api/pull` with live progress & cancellation.
pub fn pull_ollama_model<F>(
    base_url: &str,
    model_tag: &str,
    cancel: &AtomicBool,
    on_progress: F,
) -> Result<(), String>
where
    F: Fn(PullProgress),
{
    let safe_tag = validate_ollama_model_tag(model_tag)?;
    let valid_base = validate_http_url(base_url)?;
    let url = format!("{}/api/pull", valid_base.trim_end_matches('/'));
    let payload = serde_json::json!({ "name": safe_tag, "stream": true }).to_string();

    let mut child = Command::new("/usr/bin/curl")
        .args([
            "-sS",
            "-N",
            "--no-buffer",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            &payload,
            &url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Model indirme isteği başlatılamadı: {}", e))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "curl çıktı akışı açılamadı".to_string())?;
    let reader = BufReader::new(stdout);
    let mut saw_success = false;
    let mut last_err: Option<String> = None;

    for line_res in reader.lines() {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Model indirme iptal edildi".to_string());
        }
        let Ok(line) = line_res else {
            break;
        };
        if let Some(prog) = parse_ollama_pull_line(&line) {
            if let Some(ref e) = prog.error {
                last_err = Some(e.clone());
            }
            if prog.done && prog.error.is_none() {
                saw_success = true;
            }
            on_progress(prog);
        }
    }

    let status = child
        .wait()
        .map_err(|e| format!("İndirme süreci tamamlanamadı: {}", e))?;
    if let Some(err) = last_err {
        return Err(err);
    }
    if !status.success() && !saw_success {
        return Err("Model indirilemedi (ağ veya Ollama hatası)".to_string());
    }
    Ok(())
}

pub fn delete_ollama_model(base_url: &str, model_tag: &str) -> Result<(), String> {
    let safe_tag = validate_ollama_model_tag(model_tag)?;
    let valid_base = validate_http_url(base_url)?;
    let url = format!("{}/api/delete", valid_base.trim_end_matches('/'));
    let payload = serde_json::json!({ "name": safe_tag }).to_string();

    let out = Command::new("/usr/bin/curl")
        .args([
            "-fsS",
            "--max-time",
            "15",
            "-X",
            "DELETE",
            "-H",
            "Content-Type: application/json",
            "-d",
            &payload,
            &url,
        ])
        .output()
        .map_err(|e| format!("Model silinemedi: {}", e))?;
    if !out.status.success() {
        return Err("Ollama model silme isteğini reddetti".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_tier_recommender_covers_all_ram_and_vram_bands() {
        assert_eq!(recommend_tier_for_hardware(4096, 0), "2b");
        assert_eq!(recommend_tier_for_hardware(8192, 0), "4b");
        assert_eq!(recommend_tier_for_hardware(16384, 0), "8b");
        assert_eq!(recommend_tier_for_hardware(63918, 8192), "8b");
        assert_eq!(recommend_tier_for_hardware(32768, 12288), "12b");
        assert_eq!(recommend_tier_for_hardware(65536, 16384), "14b");
        assert_eq!(LOCAL_MODEL_TIERS.len(), 5);
    }

    #[test]
    fn pull_ndjson_parser_and_tag_validation_work() {
        assert!(validate_ollama_model_tag("qwen3.5:9b").is_ok());
        assert!(validate_ollama_model_tag("gemma4:12b").is_ok());
        assert!(validate_ollama_model_tag("bad;rm -rf /").is_err());

        let p = parse_ollama_pull_line(
            r#"{"status":"pulling abc","digest":"sha256:123","total":1000,"completed":250}"#,
        )
        .unwrap();
        assert_eq!(p.fraction, Some(0.25));
        assert!(!p.done);

        let ok = parse_ollama_pull_line(r#"{"status":"success"}"#).unwrap();
        assert!(ok.done);
        assert_eq!(ok.fraction, Some(1.0));

        let err = parse_ollama_pull_line(r#"{"error":"model not found"}"#).unwrap();
        assert_eq!(err.error.as_deref(), Some("model not found"));
    }

    #[test]
    fn vision_and_thinking_capability_detection_is_accurate() {
        assert!(model_supports_vision("qwen3.5:2b"));
        assert!(model_supports_vision("qwen3.5:9b"));
        assert!(model_supports_vision("gemma4:12b"));
        assert!(model_supports_vision("gemini-3.8-flash"));
        assert!(!model_supports_vision("qwen3:14b"));

        assert!(model_supports_thinking("qwen3.5:9b"));
        assert!(model_supports_thinking("gemma4:12b"));
        assert!(model_supports_thinking("deepseek-r1:14b"));
        assert!(!model_supports_thinking("llama3.2"));
    }
}
