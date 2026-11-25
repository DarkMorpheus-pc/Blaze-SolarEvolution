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
    if spec.chars().any(char::is_control) {
        return Err(AkrepError::InvalidInput(
            "Flatpak override key specification must not contain control characters".to_string(),
        ));
    }

    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(AkrepError::InvalidInput(
            "Flatpak override key specification must not be empty".to_string(),
        ));
    }

    let (raw_sec, raw_key) = if let Some(rest) = trimmed.strip_prefix('[') {
        let Some((sec_part, key_part)) = rest.split_once("].") else {
            return Err(AkrepError::InvalidInput(format!(
                "Malformed bracketed override key '{}'",
                spec
            )));
        };
        (sec_part, key_part)
    } else if let Some((sec_part, key_part)) =
        [SECTION_CONTEXT, SECTION_SESSION_BUS, SECTION_SYSTEM_BUS]
            .into_iter()
            .find_map(|known| {
                let prefix = format!("{}.", known);
                trimmed.strip_prefix(&prefix).map(|k| (known, k))
            })
    {
        (sec_part, key_part)
    } else if CONTEXT_LIST_KEYS.contains(&trimmed) {
        (SECTION_CONTEXT, trimmed)
    } else if let Some((other_sec, _)) = trimmed.split_once('.') {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Unsupported or forbidden Flatpak override section '{}'",
            other_sec.trim()
        )));
    } else {
        return Err(AkrepError::InvalidInput(format!(
            "Unsupported Flatpak override key '{}' (use Context.<key>, Session Bus Policy.<bus>, or System Bus Policy.<bus>)",
            spec
        )));
    };

    let sec = raw_sec.trim();
    let key = raw_key.trim();

    if sec.is_empty()
        || key.is_empty()
        || sec
            .chars()
            .any(|c| matches!(c, '[' | ']' | '=' | ';') || c.is_control())
        || key
            .chars()
            .any(|c| matches!(c, '[' | ']' | '=' | ';') || c.is_control())
    {
        return Err(AkrepError::InvalidInput(format!(
            "Malformed Flatpak override section or key in '{}'",
            spec
        )));
    }

    if !matches!(
        sec,
        SECTION_CONTEXT | SECTION_SESSION_BUS | SECTION_SYSTEM_BUS
    ) {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Unsupported or forbidden Flatpak override section '{}'",
            sec
        )));
    }

    if sec == SECTION_CONTEXT && !CONTEXT_LIST_KEYS.contains(&key) {
        return Err(AkrepError::InvalidInput(format!(
            "Unsupported Flatpak [Context] key '{}'",
            key
        )));
    }

    if matches!(sec, SECTION_SESSION_BUS | SECTION_SYSTEM_BUS)
        && !key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'*'))
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid D-Bus name '{}' in Flatpak bus policy override",
            key
        )));
    }

    Ok((sec.to_string(), key.to_string()))
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

pub fn filesystem_token_base(token: &str) -> &str {
    let trimmed = token.trim();
    let without_bang = trimmed.strip_prefix('!').unwrap_or(trimmed);
    for suffix in [":rw", ":ro", ":create"] {
        if let Some(base) = without_bang.strip_suffix(suffix) {
            let normalized = base.trim_end_matches('/');
            return if normalized.is_empty() && base.starts_with('/') {
                "/"
            } else {
                normalized
            };
        }
    }
    let normalized = without_bang.trim_end_matches('/');
    if normalized.is_empty() && without_bang.starts_with('/') {
        "/"
    } else {
        normalized
    }
}

impl FlatpakOverrideDocument {
    pub fn parse(input: &str) -> Result<Self, AkrepError> {
        if input.contains('\0') {
            return Err(AkrepError::InvalidInput(
                "Flatpak override INI must not contain NUL bytes".to_string(),
            ));
        }
        let cleaned = input.strip_prefix('\u{feff}').unwrap_or(input);
        if cleaned.is_empty() {
            return Ok(Self {
                lines: Vec::new(),
                trailing_newline: true,
            });
        }

        let trailing_newline = cleaned.ends_with('\n');
        let mut lines = Vec::new();
        let mut current_section: Option<String> = None;

        for raw_line in cleaned.lines() {
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

    pub fn sections(&self) -> Vec<String> {
        let mut out = Vec::new();
        for line in &self.lines {
            if let RawIniLine::SectionHeader { name, .. } = line {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
        }
        out
    }

    pub fn section_entries(&self, section: &str) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut active_sec = "";
        for line in &self.lines {
            match line {
                RawIniLine::SectionHeader { name, .. } => active_sec = name.as_str(),
                RawIniLine::Entry { key, value } if active_sec == section => {
                    out.push((key.clone(), value.trim().to_string()));
                }
                _ => {}
            }
        }
        out
    }

    pub fn get_scalar(&self, section: &str, key: &str) -> Option<String> {
        let mut active_sec = "";
        for line in &self.lines {
            match line {
                RawIniLine::SectionHeader { name, .. } => active_sec = name.as_str(),
                RawIniLine::Entry { key: k, value } if active_sec == section && k == key => {
                    return Some(value.trim().to_string());
                }
                _ => {}
            }
        }
        None
    }

    pub fn has_any_entries(&self) -> bool {
        self.lines
            .iter()
            .any(|l| matches!(l, RawIniLine::Entry { .. }))
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
                if section == SECTION_CONTEXT && key == "filesystems" {
                    let target_base = filesystem_token_base(tok);
                    let plain_base_removal = !tok.starts_with('!') && !tok.contains(':');
                    tokens.retain(|t| {
                        if t == tok {
                            return false;
                        }
                        if plain_base_removal
                            && !t.starts_with('!')
                            && filesystem_token_base(t) == target_base
                        {
                            return false;
                        }
                        true
                    });
                } else {
                    tokens.retain(|t| t != tok);
                }
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
                if section == SECTION_CONTEXT && key == "filesystems" {
                    let new_base = filesystem_token_base(tok);
                    tokens.retain(|t| t == tok || filesystem_token_base(t) != new_base);
                } else {
                    let opp = opposite_negation(tok);
                    tokens.retain(|t| t != &opp);
                }
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

    pub fn prune_empty_sections(&mut self, removable_sections: &[String]) {
        for sec in removable_sections {
            let Some((sec_start, sec_end)) = self.section_entry_bounds(sec) else {
                continue;
            };
            let has_non_blank = self
                .lines
                .iter()
                .take(sec_end)
                .skip(sec_start + 1)
                .any(|line| match line {
                    RawIniLine::Entry { .. } => true,
                    RawIniLine::Trivia(t) => !t.trim().is_empty(),
                    RawIniLine::SectionHeader { .. } => true,
                });
            if !has_non_blank {
                self.lines.drain(sec_start..sec_end);
                while matches!(self.lines.last(), Some(RawIniLine::Trivia(s)) if s.is_empty()) {
                    self.lines.pop();
                }
            }
        }
    }

    pub fn revert_diffs(
        &mut self,
        diffs: &[OverrideTokenDiff],
        pre_sections: &[String],
    ) -> Result<(), AkrepError> {
        let mut newly_added_sections = Vec::new();
        for sec in self.sections() {
            if !pre_sections.contains(&sec) {
                newly_added_sections.push(sec);
            }
        }

        for diff in diffs {
            let section = &diff.section;
            let key = &diff.key;
            if is_semicolon_list_entry(section, key) {
                let mut tokens = self.get_tokens(section, key);
                tokens.retain(|t| !diff.added.contains(t));
                for restored in &diff.removed {
                    if !tokens.contains(restored) {
                        tokens.push(restored.clone());
                    }
                }
                let new_val = if tokens.is_empty() {
                    None
                } else {
                    Some(join_semicolon_tokens(&tokens))
                };
                self.set_or_remove_entry(section, key, new_val);
            } else if let Some(prev_val) = diff.removed.first() {
                self.set_or_remove_entry(section, key, Some(prev_val.clone()));
            } else if !diff.added.is_empty() {
                self.set_or_remove_entry(section, key, None);
            }
        }

        self.prune_empty_sections(&newly_added_sections);
        Ok(())
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

    #[test]
    fn test_resolve_override_spec_normalizes_brackets_and_enforces_section_allowlist() {
        assert_eq!(
            resolve_override_spec("[ Context ].filesystems").unwrap(),
            ("Context".to_string(), "filesystems".to_string())
        );
        assert_eq!(
            resolve_override_spec("[System Bus Policy].org.freedesktop.NetworkManager").unwrap(),
            (
                "System Bus Policy".to_string(),
                "org.freedesktop.NetworkManager".to_string()
            )
        );

        assert!(resolve_override_spec("[Environment].LD_PRELOAD").is_err());
        assert!(resolve_override_spec("Environment.LD_PRELOAD").is_err());
        assert!(resolve_override_spec("[Context].unknown_key").is_err());
        assert!(resolve_override_spec("[Context].[injected]").is_err());
        assert!(resolve_override_spec("[Context].filesystems=host").is_err());
    }
}
