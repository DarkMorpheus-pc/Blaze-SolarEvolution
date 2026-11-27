// ==============================================================================
// BlazeAI Local Brain Folder (Antigravity-Style Persistent Local Memory)
// - Directory layout (~/.local/share/solarui/blazeai/brain/, 0700/0600):
//     memory.md    -> Human-editable persistent preferences & rules
//     tasks/       -> Timestamped completed action & task summaries
//     knowledge/   -> User-pinned project & system notes
// - Seamless lossless migration from legacy memory.json into memory.md
// - Fast pure-Rust Turkish-normalized full-text relevance recall (zero RAM bloat)
// ==============================================================================

use crate::blazeai::{LocalMemoryStore, MemoryFact};
use chrono::Local;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static BRAIN_TMP_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrainSearchHit {
    pub source: String,
    pub snippet: String,
    pub score: usize,
}

#[derive(Debug, Clone)]
pub struct BrainStore {
    pub root_dir: PathBuf,
}

impl BrainStore {
    pub fn default_brain_dir() -> PathBuf {
        LocalMemoryStore::default_data_dir().join("brain")
    }

    pub fn open_default() -> Self {
        Self::open_in_dir(Self::default_brain_dir())
    }

    pub fn open_in_dir(root_dir: PathBuf) -> Self {
        let _ = ensure_private_dir(&root_dir);
        let _ = ensure_private_dir(&root_dir.join("tasks"));
        let _ = ensure_private_dir(&root_dir.join("knowledge"));
        let store = Self { root_dir };
        if !store.memory_md_path().exists() {
            let _ = write_private_atomic(
                &store.memory_md_path(),
                b"# BlazeAI Yerel Hafiza\n\n<!-- Kalici tercihler, kurallar ve sistem notlari -->\n",
            );
        }
        store
    }

    pub fn memory_md_path(&self) -> PathBuf {
        self.root_dir.join("memory.md")
    }

    pub fn tasks_dir(&self) -> PathBuf {
        self.root_dir.join("tasks")
    }

    pub fn knowledge_dir(&self) -> PathBuf {
        self.root_dir.join("knowledge")
    }

    /// Synchronizes `LocalMemoryStore` facts bidirectionally with `brain/memory.md`
    /// so existing `memory.json` users migrate seamlessly and manual edits to `memory.md` are honored.
    pub fn sync_with_memory_facts(&self, facts: &[MemoryFact]) -> io::Result<()> {
        let existing = read_private_file(&self.memory_md_path()).unwrap_or_default();
        let mut lines: Vec<String> = existing
            .lines()
            .filter(|l| l.trim_start().starts_with("- "))
            .map(|l| l.trim_start().trim_start_matches("- ").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();

        for fact in facts {
            let clean = fact.text.trim();
            if clean.is_empty() {
                continue;
            }
            let norm = normalize_turkish(clean);
            if !lines.iter().any(|l| normalize_turkish(l) == norm) {
                lines.push(clean.to_string());
            }
        }

        let mut out = String::from(
            "# BlazeAI Yerel Hafiza\n\n<!-- Kalici tercihler, kurallar ve sistem notlari -->\n\n",
        );
        for item in &lines {
            out.push_str("- ");
            out.push_str(item);
            out.push('\n');
        }
        write_private_atomic(&self.memory_md_path(), out.as_bytes())
    }

    pub fn append_memory_note(&self, note: &str) -> io::Result<bool> {
        let cleaned = note.split_whitespace().collect::<Vec<_>>().join(" ");
        if cleaned.is_empty() {
            return Ok(false);
        }
        let existing = read_private_file(&self.memory_md_path()).unwrap_or_default();
        let norm_new = normalize_turkish(&cleaned);
        if existing
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("- "))
            .any(|l| normalize_turkish(l) == norm_new)
        {
            return Ok(false);
        }
        let mut updated = existing;
        if !updated.ends_with('\n') && !updated.is_empty() {
            updated.push('\n');
        }
        updated.push_str("- ");
        updated.push_str(&cleaned);
        updated.push('\n');
        write_private_atomic(&self.memory_md_path(), updated.as_bytes())?;
        Ok(true)
    }

    pub fn record_task_summary(&self, title: &str, details: &str) -> io::Result<PathBuf> {
        let slug = slugify(title);
        let stamp = Local::now().format("%Y-%m-%d_%H%M%S");
        let path = self.tasks_dir().join(format!("{}_{}.md", stamp, slug));
        let content = format!(
            "# {}\n\nTarih: {}\n\n{}\n",
            title.trim(),
            Local::now().to_rfc3339(),
            details.trim()
        );
        write_private_atomic(&path, content.as_bytes())?;
        Ok(path)
    }

    pub fn save_knowledge_note(&self, title: &str, content: &str) -> io::Result<PathBuf> {
        let slug = slugify(title);
        let path = self.knowledge_dir().join(format!("{}.md", slug));
        let body = format!("# {}\n\n{}\n", title.trim(), content.trim());
        write_private_atomic(&path, body.as_bytes())?;
        Ok(path)
    }

    pub fn read_memory_bullets(&self) -> Vec<String> {
        let Ok(raw) = read_private_file(&self.memory_md_path()) else {
            return Vec::new();
        };
        raw.lines()
            .filter_map(|l| l.trim_start().strip_prefix("- "))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn recent_task_summaries(&self, max_count: usize) -> Vec<(String, String)> {
        let Ok(entries) = fs::read_dir(self.tasks_dir()) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
            .collect();
        files.sort();
        files.reverse();
        let mut out = Vec::new();
        for p in files.into_iter().take(max_count) {
            if let Ok(txt) = read_private_file(&p) {
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let preview: String = txt.chars().take(260).collect();
                out.push((name, preview.replace('\n', " ").trim().to_string()));
            }
        }
        out
    }

    /// Searches `memory.md`, `tasks/*.md`, and `knowledge/*.md` with Turkish-aware tokenization.
    pub fn search_brain(&self, query: &str, max_hits: usize) -> Vec<BrainSearchHit> {
        let tokens = tokenize_turkish(query);
        if tokens.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<BrainSearchHit> = Vec::new();

        // 1. memory.md lines
        for bullet in self.read_memory_bullets() {
            let score = score_text_against_tokens(&bullet, &tokens);
            if score > 0 {
                hits.push(BrainSearchHit {
                    source: "brain/memory.md".to_string(),
                    snippet: bullet,
                    score: score + 1,
                });
            }
        }

        // 2. tasks/ and knowledge/ markdown files
        for (dir_label, dir_path) in [
            ("brain/tasks", self.tasks_dir()),
            ("brain/knowledge", self.knowledge_dir()),
        ] {
            let Ok(entries) = fs::read_dir(&dir_path) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("md") {
                    continue;
                }
                let Ok(content) = read_private_file(&path) else {
                    continue;
                };
                let score = score_text_against_tokens(&content, &tokens);
                if score > 0 {
                    let fname = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    let snippet: String = content
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .chars()
                        .take(260)
                        .collect();
                    hits.push(BrainSearchHit {
                        source: format!("{}/{}", dir_label, fname),
                        snippet,
                        score,
                    });
                }
            }
        }

        hits.sort_by(|a, b| b.score.cmp(&a.score));
        hits.truncate(max_hits);
        hits
    }

    /// Builds a compact context block for system-prompt injection combining recent task logs
    /// and relevant knowledge/memory hits for the active user prompt.
    pub fn build_brain_context_block(&self, latest_prompt: &str) -> String {
        let mut sections: Vec<String> = Vec::new();
        let recent = self.recent_task_summaries(3);
        if !recent.is_empty() {
            let mut block = String::from("Son yerel beyin görev kayıtları:");
            for (name, prev) in recent {
                block.push_str(&format!("\n- [{}] {}", name, prev));
            }
            sections.push(block);
        }
        let hits = self.search_brain(latest_prompt, 3);
        let extra_hits: Vec<BrainSearchHit> = hits
            .into_iter()
            .filter(|h| h.source != "brain/memory.md")
            .collect();
        if !extra_hits.is_empty() {
            let mut block = String::from("İlgili yerel beyin notları:");
            for h in extra_hits {
                block.push_str(&format!("\n- ({}) {}", h.source, h.snippet));
            }
            sections.push(block);
        }
        sections.join("\n\n")
    }
}

pub fn normalize_turkish(s: &str) -> String {
    s.to_lowercase()
        .replace("i\u{307}", "i")
        .replace('ı', "i")
        .replace('ğ', "g")
        .replace('ü', "u")
        .replace('ş', "s")
        .replace('ö', "o")
        .replace('ç', "c")
}

fn tokenize_turkish(s: &str) -> Vec<String> {
    normalize_turkish(s)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 2)
        .map(|w| w.to_string())
        .collect()
}

fn score_text_against_tokens(text: &str, tokens: &[String]) -> usize {
    let norm = normalize_turkish(text);
    tokens.iter().filter(|tok| norm.contains(tok.as_str())).count()
}

fn slugify(raw: &str) -> String {
    let norm = normalize_turkish(raw);
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in norm.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
        if slug.len() >= 36 {
            break;
        }
    }
    let clean = slug.trim_matches('-').to_string();
    if clean.is_empty() {
        "gorev".to_string()
    } else {
        clean
    }
}

fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn reject_if_symlink(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Refusing symlink for private BlazeAI brain file",
        )),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn read_private_file(path: &Path) -> io::Result<String> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    reject_if_symlink(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing parent directory"))?;
    ensure_private_dir(parent)?;

    let seq = BRAIN_TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_path = parent.join(format!(".tmp.brain.{}.{}", std::process::id(), seq));
    let write_res = (|| -> io::Result<()> {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&tmp_path)?;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
        f.write_all(bytes)?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brain_store_migrates_facts_logs_tasks_and_searches_turkish() {
        let tmp = tempfile::tempdir().unwrap();
        let brain = BrainStore::open_in_dir(tmp.path().join("brain"));

        let legacy_facts = vec![MemoryFact {
            id: "1".to_string(),
            text: "Kullanıcı Btrfs dosya sistemi ve Ptyxis terminali seviyor".to_string(),
            created_at: "2026-11-27".to_string(),
        }];
        brain.sync_with_memory_facts(&legacy_facts).unwrap();
        assert!(brain.append_memory_note("Rust projelerinde Fedora 45 konteyneri kullan").unwrap());
        assert!(!brain.append_memory_note("Rust projelerinde Fedora 45 konteyneri kullan").unwrap());

        let task_path = brain
            .record_task_summary("RTX 4000 Sürücü Notu", "Quadro RTX 4000 için akmod-nvidia kurulumu planlandı.")
            .unwrap();
        assert!(task_path.is_file());
        let mode = fs::metadata(&task_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);

        brain
            .save_knowledge_note("BlazeOS Mimari", "SolarUI Niri üzerinde çalışır ve Noctalia varsayılan kabuktur.")
            .unwrap();

        let hits = brain.search_brain("rtx 4000 surucu", 5);
        assert!(!hits.is_empty());
        assert!(hits[0].source.starts_with("brain/tasks/"));

        let ctx = brain.build_brain_context_block("Niri Noctalia kabuk nedir?");
        assert!(ctx.contains("Noctalia"));
    }
}
