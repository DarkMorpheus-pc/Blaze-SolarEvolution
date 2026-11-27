// ==============================================================================
// BlazeAI Safe Agent Tools (Free Web Research + User-Approved File Creation)
// - Free DuckDuckGo HTML web research (zero API keys, clean snippet extraction)
// - Structured file proposals rendered as inline Approval Cards in Omnibar
// - Strict $HOME path containment, symlink rejection, and sensitive path denylist
// - Never silently executes shell commands or modifies files without user click
// ==============================================================================

use crate::blazeai::url_encode_query;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const MAX_PROPOSED_FILE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchResult {
    pub title: String,
    pub snippet: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingFileProposal {
    pub display_path: String,
    pub content: String,
    #[serde(default)]
    pub applied: bool,
}

/// Executes a free, keyless DuckDuckGo HTML search via `/usr/bin/curl` and extracts
/// top clean snippets + destination URLs for grounded, hallucination-free replies.
pub fn execute_web_research(query: &str, max_results: usize) -> Result<Vec<WebSearchResult>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://html.duckduckgo.com/html/?q={}",
        url_encode_query(trimmed)
    );
    let out = Command::new("/usr/bin/curl")
        .args([
            "-fsSL",
            "--max-time",
            "8",
            "-A",
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) BlazeOS/5.05",
            &url,
        ])
        .output()
        .map_err(|e| format!("Web araştırması başlatılamadı: {}", e))?;
    if !out.status.success() {
        return Err("Web aramasına erişilemedi".to_string());
    }
    let html = String::from_utf8_lossy(&out.stdout);
    Ok(parse_duckduckgo_html(&html, max_results))
}

pub fn parse_duckduckgo_html(html: &str, max_results: usize) -> Vec<WebSearchResult> {
    let mut results = Vec::new();
    let mut cursor = 0;

    while results.len() < max_results {
        let Some(rel) = html[cursor..].find("result__a") else {
            break;
        };
        let marker_idx = cursor + rel;
        let anchor_start = html[..marker_idx].rfind("<a").unwrap_or(marker_idx);
        let Some(anchor_end_rel) = html[anchor_start..].find("</a>") else {
            break;
        };
        let anchor_block = &html[anchor_start..anchor_start + anchor_end_rel];
        let raw_href = extract_attr(anchor_block, "href").unwrap_or_default();
        let resolved_url = decode_ddg_redirect(&raw_href);
        let title_raw = anchor_block
            .split_once('>')
            .map(|(_, inner)| inner)
            .unwrap_or("");
        let title = clean_html_text(title_raw);

        let search_after = anchor_start + anchor_end_rel;
        let window_end = (search_after + 2200).min(html.len());
        let tail = &html[search_after..window_end];
        let snippet = if let Some(s_pos) = tail.find("result__snippet") {
            let s_tail = &tail[s_pos..];
            if let Some(gt) = s_tail.find('>') {
                let inner = &s_tail[gt + 1..];
                let end = inner.find("</a>").or_else(|| inner.find("</div>")).unwrap_or(inner.len());
                clean_html_text(&inner[..end])
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        if !title.is_empty() && resolved_url.starts_with("http") {
            results.push(WebSearchResult {
                title,
                snippet,
                url: resolved_url,
            });
        }
        cursor = search_after + 4;
    }

    results
}

pub fn format_web_research_context(query: &str, results: &[WebSearchResult]) -> String {
    if results.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "Canlı web araştırması sonuçları (sorgu: \"{}\") — yanıtında bu güncel bilgilere dayan ve uydurma:",
        query.trim()
    );
    for (idx, r) in results.iter().enumerate() {
        out.push_str(&format!("\n{}. {} ({})", idx + 1, r.title, r.url));
        if !r.snippet.is_empty() {
            out.push_str(&format!("\n   Özet: {}", r.snippet));
        }
    }
    out
}

/// Extracts explicit `[WEB_ARA: ...]` requests emitted by a model when it wants live web context.
pub fn extract_web_search_tag(reply: &str) -> Option<String> {
    let mut cursor = 0;
    while let Some(open_rel) = reply[cursor..].find('[') {
        let open_idx = cursor + open_rel;
        let Some(close_rel) = reply[open_idx..].find(']') else {
            break;
        };
        let inner = &reply[open_idx + 1..open_idx + close_rel];
        if let Some((tag, rest)) = inner.trim().split_once(':') {
            let norm = tag
                .trim()
                .to_lowercase()
                .replace("i\u{307}", "i")
                .replace('ı', "i");
            if matches!(norm.as_str(), "web_ara" | "web_search") {
                let q = rest.trim();
                if !q.is_empty() {
                    return Some(q.to_string());
                }
            }
        }
        cursor = open_idx + close_rel + 1;
    }
    None
}

/// Extracts optional structured file creation proposals wrapped in:
/// ```text
/// [DOSYA_ONER: ~/Documents/ornek.md]
/// ...file content...
/// [/DOSYA_ONER]
/// ```
/// Returning `(cleaned_reply_text, proposals)`. Proposals are NEVER written to disk automatically;
/// Omnibar displays an explicit `[Onayla ve Oluştur]` / `[Reddet]` card to the user.
pub fn extract_file_proposals_from_reply(reply: &str) -> (String, Vec<PendingFileProposal>) {
    let mut cleaned = String::with_capacity(reply.len());
    let mut proposals = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = reply[cursor..].find("[DOSYA_ONER:") {
        let start = cursor + rel;
        let Some(header_end_rel) = reply[start..].find(']') else {
            break;
        };
        let header_end = start + header_end_rel;
        let path_raw = reply[start + "[DOSYA_ONER:".len()..header_end].trim();

        let body_start = header_end + 1;
        let Some(close_rel) = reply[body_start..].find("[/DOSYA_ONER]") else {
            break;
        };
        let close_idx = body_start + close_rel;
        let content = reply[body_start..close_idx].trim_matches('\n').to_string();

        cleaned.push_str(&reply[cursor..start]);
        if !path_raw.is_empty() && content.len() <= MAX_PROPOSED_FILE_BYTES {
            proposals.push(PendingFileProposal {
                display_path: path_raw.to_string(),
                content,
                applied: false,
            });
        }
        cursor = close_idx + "[/DOSYA_ONER]".len();
    }

    cleaned.push_str(&reply[cursor..]);
    (cleaned.trim().to_string(), proposals)
}

/// Validates that a proposed file path stays strictly inside `home_dir`, contains no
/// traversal/symlinks, and never targets sensitive security/shell startup locations.
pub fn validate_safe_user_path(raw_path: &str, home_dir: &Path) -> Result<PathBuf, String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() || trimmed.contains('\0') {
        return Err("Geçersiz dosya yolu".to_string());
    }
    let candidate = if let Some(rest) = trimmed.strip_prefix("~/") {
        home_dir.join(rest)
    } else if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        home_dir.join(trimmed)
    };

    for comp in candidate.components() {
        if matches!(comp, Component::ParentDir) {
            return Err("Üst dizine (..) geçişe izin verilmiyor".to_string());
        }
    }

    let rel = candidate
        .strip_prefix(home_dir)
        .map_err(|_| "Dosya yalnızca kullanıcı ev dizini (~/) içinde oluşturulabilir".to_string())?;
    let rel_str = rel.to_string_lossy();
    if rel_str.is_empty() {
        return Err("Dosya adı belirtilmedi".to_string());
    }

    let forbidden_prefixes = [
        ".ssh",
        ".gnupg",
        ".blaze-signing",
        ".pki",
        ".local/share/keyrings",
        ".config/autostart",
        ".config/systemd",
        ".config/solarui/blazeai_secrets.json",
        ".bashrc",
        ".bash_profile",
        ".profile",
        ".zshrc",
    ];
    for bad in forbidden_prefixes {
        if rel_str == bad || rel_str.starts_with(&format!("{}/", bad)) {
            return Err(format!("Güvenlik politikası nedeniyle '{}' engellendi", bad));
        }
    }

    // Walk every existing ancestor inside home_dir and reject symlinks
    let mut walk = home_dir.to_path_buf();
    for comp in rel.components() {
        walk.push(comp);
        if let Ok(meta) = fs::symlink_metadata(&walk) {
            if meta.file_type().is_symlink() {
                return Err("Sembolik bağlantı (symlink) hedeflerine yazma engellendi".to_string());
            }
        }
    }

    Ok(candidate)
}

/// Applies a user-approved file proposal safely inside `$HOME` without following symlinks.
pub fn apply_approved_file_proposal(
    proposal: &PendingFileProposal,
    home_dir: &Path,
) -> Result<PathBuf, String> {
    if proposal.content.len() > MAX_PROPOSED_FILE_BYTES {
        return Err("Önerilen dosya boyutu 256 KiB sınırını aşıyor".to_string());
    }
    let target = validate_safe_user_path(&proposal.display_path, home_dir)?;
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Klasör oluşturulamadı: {}", e))?;
    }
    let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&target)
        .map_err(|e| format!("Dosya yazılamadı: {}", e))?;
    f.write_all(proposal.content.as_bytes())
        .map_err(|e| format!("Dosya içeriği yazılamadı: {}", e))?;
    f.sync_all().map_err(|e| format!("Disk eşitleme hatası: {}", e))?;
    Ok(target)
}

fn extract_attr(tag_html: &str, attr_name: &str) -> Option<String> {
    let needle = format!("{}=\"", attr_name);
    let idx = tag_html.find(&needle)?;
    let tail = &tag_html[idx + needle.len()..];
    let end = tail.find('"')?;
    Some(tail[..end].to_string())
}

fn decode_ddg_redirect(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let tail = &href[pos + 5..];
        let encoded = tail.split('&').next().unwrap_or(tail);
        return percent_decode(encoded);
    }
    if href.starts_with("//") {
        return format!("https:{}", href);
    }
    href.to_string()
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn clean_html_text(raw: &str) -> String {
    let mut plain = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    plain
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ddg_html_parser_extracts_clean_results_and_unwraps_redirects() {
        let html = r#"
        <div class="result">
          <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fdocs.fedoraproject.org%2Fen-US%2F&rut=abc">Fedora <b>Docs</b></a>
          <a class="result__snippet">Official Fedora Linux 45 &amp; Niri documentation.</a>
        </div>
        "#;
        let res = parse_duckduckgo_html(html, 4);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].title, "Fedora Docs");
        assert_eq!(res[0].url, "https://docs.fedoraproject.org/en-US/");
        assert_eq!(res[0].snippet, "Official Fedora Linux 45 & Niri documentation.");
    }

    #[test]
    fn file_proposals_and_strict_home_security_guards_work() {
        let reply = "Notunu hazırladım:\n[DOSYA_ONER: ~/Documents/plan.md]\n# Plan\n- Adım 1\n[/DOSYA_ONER]\nOnaylarsan kaydederim.";
        let (clean, props) = extract_file_proposals_from_reply(reply);
        assert_eq!(clean, "Notunu hazırladım:\n\nOnaylarsan kaydederim.");
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].display_path, "~/Documents/plan.md");

        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();

        assert!(validate_safe_user_path("/etc/passwd", home).is_err());
        assert!(validate_safe_user_path("~/../../etc/shadow", home).is_err());
        assert!(validate_safe_user_path("~/.ssh/authorized_keys", home).is_err());
        assert!(validate_safe_user_path("~/.blaze-signing/key", home).is_err());
        assert!(validate_safe_user_path("~/.config/solarui/blazeai_secrets.json", home).is_err());
        assert!(validate_safe_user_path("~/.bashrc", home).is_err());

        let saved = apply_approved_file_proposal(&props[0], home).unwrap();
        assert_eq!(saved, home.join("Documents/plan.md"));
        assert_eq!(fs::read_to_string(saved).unwrap(), "# Plan\n- Adım 1");
    }

    #[test]
    fn web_search_tag_detector_handles_turkish_and_english_tags() {
        assert_eq!(
            extract_web_search_tag("Bakıyorum [WEB_ARA: Fedora 45 kernel sürümü]"),
            Some("Fedora 45 kernel sürümü".to_string())
        );
        assert_eq!(
            extract_web_search_tag("[web_search: qwen3.5 ollama]"),
            Some("qwen3.5 ollama".to_string())
        );
        assert_eq!(extract_web_search_tag("Normal yanıt"), None);
    }
}
