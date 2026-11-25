use crate::errors::AkrepError;
use serde::{Deserialize, Serialize};

pub const SECTION_CONTEXT: &str = "Context";
pub const SECTION_SESSION_BUS: &str = "Session Bus Policy";
pub const SECTION_SYSTEM_BUS: &str = "System Bus Policy";

const CONTEXT_LIST_KEYS: &[&str] = &[
    "shared",
    "sockets",
    "devices",
    "filesystems",
    "features",
    "persistent",
    "unset-environment",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum RawIniLine {
    Trivia(String),
    SectionHeader { raw: String, name: String },
    Entry { key: String, value: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatpakOverrideDocument {
    lines: Vec<RawIniLine>,
    trailing_newline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideTokenDiff {
    pub section: String,
    pub key: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

pub fn resolve_override_spec(spec: &str) -> Result<(String, String), AkrepError> {
    let trimmed = spec.trim();
    if trimmed.is_empty()
        || trimmed.contains('\n')
        || trimmed.contains('\r')
        || trimmed.contains('\0')
    {
        return Err(AkrepError::InvalidInput(
            "Invalid Flatpak override key specification".to_string(),
        ));
    }

    if let Some(rest) = trimmed.strip_prefix('[') {
        if let Some((sec, key_part)) = rest.split_once("].") {
            if sec.is_empty() || key_part.is_empty() || key_part.contains('=') {
                return Err(AkrepError::InvalidInput(format!(
                    "Malformed bracketed override key '{}'",
                    spec
                )));
            }
            return Ok((sec.to_string(), key_part.to_string()));
        }
    }

    for known_sec in [SECTION_CONTEXT, SECTION_SESSION_BUS, SECTION_SYSTEM_BUS] {
        let prefix = format!("{}.", known_sec);
        if let Some(key_part) = trimmed.strip_prefix(&prefix) {
            if key_part.is_empty() || key_part.contains('=') {
                return Err(AkrepError::InvalidInput(format!(
                    "Malformed override key '{}'",
                    spec
                )));
            }
            return Ok((known_sec.to_string(), key_part.to_string()));
        }
    }

    if CONTEXT_LIST_KEYS.contains(&trimmed) {
        return Ok((SECTION_CONTEXT.to_string(), trimmed.to_string()));
    }

    Err(AkrepError::InvalidInput(format!(
        "Unsupported Flatpak override key '{}' (use Context.<key>, Session Bus Policy.<bus>, or System Bus Policy.<bus>)",
        spec
    )))
}

fn is_semicolon_list_entry(section: &str, key: &str) -> bool {
    section == SECTION_CONTEXT && CONTEXT_LIST_KEYS.contains(&key)
}

pub fn split_semicolon_tokens(raw_value: &str) -> Vec<String> {
    raw_value
        .split(';')
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(ToString::to_string)
        .collect()
}

pub fn join_semicolon_tokens(tokens: &[String]) -> String {
    if tokens.is_empty() {
        String::new()
    } else {
        format!("{};", tokens.join(";"))
    }
}

fn opposite_negation(token: &str) -> String {
    if let Some(stripped) = token.strip_prefix('!') {
        stripped.to_string()
    } else {
        format!("!{}", token)
    }
}

impl FlatpakOverrideDocument {
    pub fn parse(input: &str) -> Result<Self, AkrepError> {
        if input.contains('\0') {
            return Err(AkrepError::InvalidInput(
                "Flatpak override INI must not contain NUL bytes".to_string(),
            ));
        }
        if input.is_empty() {
            return Ok(Self {
                lines: Vec::new(),
                trailing_newline: true,
            });
        }

        let trailing_newline = input.ends_with('\n');
        let mut lines = Vec::new();
        let mut current_section: Option<String> = None;

        for raw_line in input.lines() {
            let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                lines.push(RawIniLine::Trivia(line.to_string()));
                continue;
            }
            if trimmed.starts_with('[') && trimmed.ends_with(']') && trimmed.len() >= 3 {
                let sec_name = trimmed[1..trimmed.len() - 1].trim().to_string();
                current_section = Some(sec_name.clone());
                lines.push(RawIniLine::SectionHeader {
                    raw: line.to_string(),
                    name: sec_name,
                });
                continue;
            }
            if current_section.is_none() {
                return Err(AkrepError::InvalidInput(format!(
                    "INI entry '{}' found outside any [Section]",
                    trimmed
                )));
            }
            let Some((k, v)) = line.split_once('=') else {
                return Err(AkrepError::InvalidInput(format!(
                    "Malformed INI line '{}'",
                    trimmed
                )));
            };
            lines.push(RawIniLine::Entry {
                key: k.trim().to_string(),
                value: v.to_string(),
            });
        }

        Ok(Self {
            lines,
            trailing_newline,
        })
    }

    pub fn get_tokens(&self, section: &str, key: &str) -> Vec<String> {
        let mut active_sec = "";
        for line in &self.lines {
            match line {
                RawIniLine::SectionHeader { name, .. } => active_sec = name.as_str(),
                RawIniLine::Entry { key: k, value } if active_sec == section && k == key => {
                    return if is_semicolon_list_entry(section, key) {
                        split_semicolon_tokens(value)
                    } else if value.trim().is_empty() {
                        Vec::new()
                    } else {
                        vec![value.trim().to_string()]
                    };
                }
                _ => {}
            }
        }
        Vec::new()
    }

    fn section_entry_bounds(&self, section: &str) -> Option<(usize, usize)> {
        let mut start_idx = None;
        for (idx, line) in self.lines.iter().enumerate() {
            if let RawIniLine::SectionHeader { name, .. } = line {
                if start_idx.is_some() {
                    return start_idx.map(|s| (s, idx));
                }
                if name == section {
                    start_idx = Some(idx);
                }
            }
        }
        start_idx.map(|s| (s, self.lines.len()))
    }

    fn find_entry_index(&self, section: &str, key: &str) -> Option<usize> {
        let (sec_start, sec_end) = self.section_entry_bounds(section)?;
        for (idx, line) in self
            .lines
            .iter()
            .enumerate()
            .take(sec_end)
            .skip(sec_start + 1)
        {
            if let RawIniLine::Entry { key: k, .. } = line {
                if k == key {
                    return Some(idx);
                }
            }
        }
        None
    }

    fn set_or_remove_entry(&mut self, section: &str, key: &str, new_value: Option<String>) {
        if let Some(entry_idx) = self.find_entry_index(section, key) {
            match new_value {
                Some(val) => {
                    self.lines[entry_idx] = RawIniLine::Entry {
                        key: key.to_string(),
                        value: val,
                    };
                }
                None => {
                    self.lines.remove(entry_idx);
                }
            }
            return;
        }

        let Some(val) = new_value else {
            return;
        };

        if let Some((sec_start, sec_end)) = self.section_entry_bounds(section) {
            let mut insert_at = sec_start + 1;
            for (idx, line) in self
                .lines
                .iter()
                .enumerate()
                .take(sec_end)
                .skip(sec_start + 1)
            {
                if matches!(line, RawIniLine::Entry { .. }) {
                    insert_at = idx + 1;
                }
            }
            self.lines.insert(
                insert_at,
                RawIniLine::Entry {
                    key: key.to_string(),
                    value: val,
                },
            );
        } else {
            if !self.lines.is_empty()
                && !matches!(self.lines.last(), Some(RawIniLine::Trivia(s)) if s.is_empty())
            {
                self.lines.push(RawIniLine::Trivia(String::new()));
            }
            self.lines.push(RawIniLine::SectionHeader {
                raw: format!("[{}]", section),
                name: section.to_string(),
            });
            self.lines.push(RawIniLine::Entry {
                key: key.to_string(),
                value: val,
            });
        }
    }

    pub fn apply_changes(
        &mut self,
        additions: &[(String, String)],
        removals: &[(String, String)],
    ) -> Result<Vec<OverrideTokenDiff>, AkrepError> {
        let before = self.clone();

        for (spec, token) in removals {
            let (section, key) = resolve_override_spec(spec)?;
            let tok = token.trim();
            if tok.is_empty() || tok.contains(';') || tok.contains('\n') || tok.contains('\r') {
                return Err(AkrepError::InvalidInput(format!(
                    "Invalid override removal token '{}'",
                    token
                )));
            }
            if is_semicolon_list_entry(&section, &key) {
                let mut tokens = self.get_tokens(&section, &key);
                tokens.retain(|t| t != tok);
                let new_val = if tokens.is_empty() {
                    None
                } else {
                    Some(join_semicolon_tokens(&tokens))
                };
                self.set_or_remove_entry(&section, &key, new_val);
            } else {
                let current = self.get_tokens(&section, &key);
                if tok == "*" || current.iter().any(|c| c == tok) {
                    self.set_or_remove_entry(&section, &key, None);
                }
            }
        }

        for (spec, token) in additions {
            let (section, key) = resolve_override_spec(spec)?;
            let tok = token.trim();
            if tok.is_empty() || tok.contains(';') || tok.contains('\n') || tok.contains('\r') {
                return Err(AkrepError::InvalidInput(format!(
                    "Invalid override addition token '{}'",
                    token
                )));
            }
            if is_semicolon_list_entry(&section, &key) {
                let mut tokens = self.get_tokens(&section, &key);
                let opp = opposite_negation(tok);
                tokens.retain(|t| t != &opp);
                if !tokens.iter().any(|t| t == tok) {
                    tokens.push(tok.to_string());
                }
                self.set_or_remove_entry(&section, &key, Some(join_semicolon_tokens(&tokens)));
            } else {
                self.set_or_remove_entry(&section, &key, Some(tok.to_string()));
            }
        }

        Ok(Self::diff(&before, self))
    }

    pub fn diff(before: &Self, after: &Self) -> Vec<OverrideTokenDiff> {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for doc in [before, after] {
            let mut active_sec = String::new();
            for line in &doc.lines {
                match line {
                    RawIniLine::SectionHeader { name, .. } => active_sec = name.clone(),
                    RawIniLine::Entry { key, .. } => {
                        let pair = (active_sec.clone(), key.clone());
                        if !pairs.contains(&pair) {
                            pairs.push(pair);
                        }
                    }
                    RawIniLine::Trivia(_) => {}
                }
            }
        }

        let mut diffs = Vec::new();
        for (sec, key) in pairs {
            let old_tokens = before.get_tokens(&sec, &key);
            let new_tokens = after.get_tokens(&sec, &key);
            let added: Vec<String> = new_tokens
                .iter()
                .filter(|t| !old_tokens.contains(t))
                .cloned()
                .collect();
            let removed: Vec<String> = old_tokens
                .iter()
                .filter(|t| !new_tokens.contains(t))
                .cloned()
                .collect();
            if !added.is_empty() || !removed.is_empty() {
                diffs.push(OverrideTokenDiff {
                    section: sec,
                    key,
                    added,
                    removed,
                });
            }
        }
        diffs
    }

    pub fn to_ini_string(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for (idx, line) in self.lines.iter().enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            match line {
                RawIniLine::Trivia(raw) => out.push_str(raw),
                RawIniLine::SectionHeader { raw, .. } => out.push_str(raw),
                RawIniLine::Entry { key, value } => {
                    out.push_str(key);
                    out.push('=');
                    out.push_str(value);
                }
            }
        }
        if self.trailing_newline {
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lossless_token_modification_preserves_unrelated_overrides() {
        let original = "\
# User custom override comment
[Context]
shared=network;ipc;
sockets=x11;wayland;pulseaudio;
filesystems=xdg-download;home;/srv/projects:ro;

[Session Bus Policy]
org.freedesktop.Notifications=talk
org.kde.StatusNotifierWatcher=talk

[Environment]
GTK_THEME=Adwaita:dark
CUSTOM_VAR=keep_me
";

        let mut doc = FlatpakOverrideDocument::parse(original).unwrap();
        let diffs = doc
            .apply_changes(
                &[
                    ("Context.filesystems".to_string(), "!home".to_string()),
                    (
                        "Session Bus Policy.org.freedesktop.secrets".to_string(),
                        "none".to_string(),
                    ),
                ],
                &[("Context.sockets".to_string(), "x11".to_string())],
            )
            .unwrap();

        assert_eq!(diffs.len(), 3);
        let serialized = doc.to_ini_string();
        assert!(serialized.contains("# User custom override comment\n"));
        assert!(serialized.contains("shared=network;ipc;\n"));
        assert!(serialized.contains("sockets=wayland;pulseaudio;\n"));
        assert!(serialized.contains("filesystems=xdg-download;/srv/projects:ro;!home;\n"));
        assert!(serialized.contains("org.kde.StatusNotifierWatcher=talk\n"));
        assert!(serialized.contains("org.freedesktop.secrets=none\n"));
        assert!(serialized.contains("[Environment]\nGTK_THEME=Adwaita:dark\nCUSTOM_VAR=keep_me\n"));
    }
}
