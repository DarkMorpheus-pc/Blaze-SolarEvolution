// ==============================================================================
// BlazeAI Assistant Engine, 0600 Secret Store & Two-Layer Local Memory
// - 0600 per-provider API key storage + one-time legacy config.toml migration
// - Two-layer local memory (~/.local/share/solarui/blazeai/, 0700/0600)
// - Dynamic 2-layer System Prompt (immutable core + 16-expression persona)
// - Non-blocking curl -K - adapter (zero API keys in argv or ps)
// - Automatic web-search fallback when no key / Ollama is offline
// - Allowlisted 1-click desktop app suggestions (never arbitrary shell)
// ==============================================================================

use crate::agent_tools::{
    execute_web_research, extract_file_proposals_from_reply, extract_web_search_tag,
    format_web_research_context, PendingFileProposal,
};
use crate::brain_store::BrainStore;
use crate::local_ai::{ensure_ollama_serving, model_supports_thinking};
use crate::voice_pipeline::VisionAttachment;
use chrono::Local;
use serde::{Deserialize, Serialize};
use solar_common::{BlazeAiProvider, CompanionExpression, OmnibarConfig, SolarConfig};
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_SESSION_MESSAGES: usize = 40;
pub const MAX_MEMORY_FACTS: usize = 50;
pub const MAX_MEMORY_FACT_CHARS: usize = 240;

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);

pub const BLAZEOS_SYSTEM_GUIDE: &str = "\
BlazeOS / SolarEvolution sistem rehberi:
- Super+Boşluk: Solar Omnibar (uygulama başlatıcı, =hesaplama, kur/birim çevirici, cb pano geçmişi, kill süreç ve BlazeAI)
- Super+G: Blaze GameZone tam ekran oyun kabuğu (solar-shell gamezone)
- Super+I: SolarUI Ayarları (ekran, duvar kağıdı, kabuk motoru, Omnibar/BlazeAI)
- Masaüstü kabukları: Noctalia (varsayılan), Caelestia, end4-pC, We10X (solar-shell switch <kabuk>)
- Akrep: Güvenlik merkezi (akrep-ui: SELinux, güvenlik duvarı, Flatpak izinleri, ClamAV)
- FireHub: Uygulama mağazası (firehub: Portadora ve Flathub uygulamaları)
- BERP / BERE: Acil kurtarma ve Btrfs anlık görüntü konsolu (ptyxis -- blaze-recovery)";

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct BlazeAiSecrets {
    #[serde(default)]
    pub gemini_key: String,
    #[serde(default)]
    pub openai_key: String,
    #[serde(default)]
    pub anthropic_key: String,
    #[serde(default)]
    pub compatible_key: String,
}

impl BlazeAiSecrets {
    pub fn secrets_path() -> PathBuf {
        if let Ok(p) = std::env::var("BLAZEAI_SECRETS_PATH") {
            let trimmed = p.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        solar_common::get_solar_config_dir().join("blazeai_secrets.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::secrets_path())
    }

    pub fn load_from(path: &Path) -> Self {
        match read_private_file(path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(&Self::secrets_path())
    }

    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(io::Error::other)?;
        write_private_atomic(path, json.as_bytes())
    }

    pub fn key_for(&self, provider: BlazeAiProvider) -> &str {
        match provider {
            BlazeAiProvider::Gemini => &self.gemini_key,
            BlazeAiProvider::OpenAi => &self.openai_key,
            BlazeAiProvider::Anthropic => &self.anthropic_key,
            BlazeAiProvider::OpenAiCompatible => &self.compatible_key,
            BlazeAiProvider::Ollama => "",
        }
    }

    pub fn set_key_for(&mut self, provider: BlazeAiProvider, key: &str) {
        let cleaned = key.trim().to_string();
        match provider {
            BlazeAiProvider::Gemini => self.gemini_key = cleaned,
            BlazeAiProvider::OpenAi => self.openai_key = cleaned,
            BlazeAiProvider::Anthropic => self.anthropic_key = cleaned,
            BlazeAiProvider::OpenAiCompatible => self.compatible_key = cleaned,
            BlazeAiProvider::Ollama => {}
        }
    }

    pub fn has_key_for(&self, provider: BlazeAiProvider) -> bool {
        !provider.requires_api_key() || !self.key_for(provider).trim().is_empty()
    }
}

/// Migrates any legacy `omnibar.ai_api_key` stored inside `config.toml` into the
/// `0600` `blazeai_secrets.json` store and clears `config.toml`'s plaintext field.
pub fn migrate_legacy_api_key_if_needed() -> BlazeAiSecrets {
    let secrets_path = BlazeAiSecrets::secrets_path();
    let mut cfg = SolarConfig::load();
    let legacy = cfg.omnibar.ai_api_key.trim().to_string();
    if legacy.is_empty() {
        return BlazeAiSecrets::load_from(&secrets_path);
    }

    let mut secrets = BlazeAiSecrets::load_from(&secrets_path);
    if secrets.key_for(cfg.omnibar.ai_provider).trim().is_empty() {
        secrets.set_key_for(cfg.omnibar.ai_provider, &legacy);
    }
    if secrets.save_to(&secrets_path).is_ok() {
        cfg.omnibar.ai_api_key.clear();
        let _ = SolarConfig::update(|c| {
            c.omnibar.ai_api_key.clear();
        });
    }
    secrets
}

/// Testable helper to migrate a `SolarConfig` instance to a specific secrets path.
pub fn migrate_legacy_api_key_in_memory(
    cfg: &mut SolarConfig,
    secrets_path: &Path,
) -> io::Result<bool> {
    let legacy = cfg.omnibar.ai_api_key.trim().to_string();
    if legacy.is_empty() {
        return Ok(false);
    }
    let mut secrets = BlazeAiSecrets::load_from(secrets_path);
    if secrets.key_for(cfg.omnibar.ai_provider).trim().is_empty() {
        secrets.set_key_for(cfg.omnibar.ai_provider, &legacy);
    }
    secrets.save_to(secrets_path)?;
    cfg.omnibar.ai_api_key.clear();
    Ok(true)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    SystemNote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SuggestedAction {
    pub label: String,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub timestamp: String,
    #[serde(default)]
    pub speaker_name: String,
    #[serde(default)]
    pub suggested_actions: Vec<SuggestedAction>,
    #[serde(default)]
    pub thinking_text: String,
    #[serde(default)]
    pub file_proposals: Vec<PendingFileProposal>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryFact {
    pub id: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct LocalMemoryStore {
    pub data_dir: PathBuf,
    pub messages: Vec<ChatMessage>,
    pub memory_facts: Vec<MemoryFact>,
}

impl LocalMemoryStore {
    pub fn default_data_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("BLAZEAI_DATA_DIR") {
            let trimmed = dir.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
            let trimmed = xdg_data.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join("solarui/blazeai");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            let trimmed = home.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed).join(".local/share/solarui/blazeai");
            }
        }
        PathBuf::from("/tmp/solarui/blazeai")
    }

    pub fn load(persist_chat: bool) -> Self {
        Self::open_in_dir(Self::default_data_dir(), persist_chat)
    }

    pub fn open_in_dir(data_dir: PathBuf, persist_chat: bool) -> Self {
        let _ = ensure_private_dir(&data_dir);
        let sessions_dir = data_dir.join("sessions");
        let _ = ensure_private_dir(&sessions_dir);

        let memory_facts = read_private_file(&data_dir.join("memory.json"))
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<MemoryFact>>(&raw).ok())
            .unwrap_or_default();

        let messages = if persist_chat {
            read_private_file(&sessions_dir.join("current.json"))
                .ok()
                .and_then(|raw| serde_json::from_str::<Vec<ChatMessage>>(&raw).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let store = Self {
            data_dir: data_dir.clone(),
            messages,
            memory_facts,
        };
        let brain = BrainStore::open_in_dir(data_dir.join("brain"));
        let _ = brain.sync_with_memory_facts(&store.memory_facts);
        store
    }

    pub fn session_path(&self) -> PathBuf {
        self.data_dir.join("sessions/current.json")
    }

    pub fn memory_path(&self) -> PathBuf {
        self.data_dir.join("memory.json")
    }

    pub fn append_message(&mut self, msg: ChatMessage, persist_chat: bool) -> io::Result<()> {
        self.messages.push(msg);
        if self.messages.len() > MAX_SESSION_MESSAGES {
            let drop_count = self.messages.len() - MAX_SESSION_MESSAGES;
            self.messages.drain(0..drop_count);
        }
        if persist_chat {
            self.save_session()?;
        }
        Ok(())
    }

    pub fn save_session(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.messages).map_err(io::Error::other)?;
        write_private_atomic(&self.session_path(), json.as_bytes())
    }

    pub fn clear_session(&mut self) -> io::Result<()> {
        self.messages.clear();
        let json = serde_json::to_string_pretty(&self.messages).map_err(io::Error::other)?;
        write_private_atomic(&self.session_path(), json.as_bytes())
    }

    pub fn save_memory(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.memory_facts).map_err(io::Error::other)?;
        write_private_atomic(&self.memory_path(), json.as_bytes())
    }

    pub fn add_memory_fact(&mut self, raw_text: &str) -> io::Result<bool> {
        let cleaned = truncate_fact_chars(raw_text.trim(), MAX_MEMORY_FACT_CHARS);
        if cleaned.is_empty() {
            return Ok(false);
        }
        let lower = cleaned.to_lowercase();
        if self
            .memory_facts
            .iter()
            .any(|f| f.text.trim().to_lowercase() == lower)
        {
            return Ok(false);
        }
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let fact = MemoryFact {
            id: format!("mem-{}-{}", Local::now().timestamp_millis(), seq),
            text: cleaned,
            created_at: Local::now().to_rfc3339(),
        };
        self.memory_facts.push(fact);
        if self.memory_facts.len() > MAX_MEMORY_FACTS {
            let drop_count = self.memory_facts.len() - MAX_MEMORY_FACTS;
            self.memory_facts.drain(0..drop_count);
        }
        self.save_memory()?;
        let brain = BrainStore::open_in_dir(self.data_dir.join("brain"));
        let _ = brain.append_memory_note(&self.memory_facts.last().map(|f| f.text.as_str()).unwrap_or(""));
        Ok(true)
    }

    pub fn remove_memory_fact(&mut self, id: &str) -> io::Result<bool> {
        let before = self.memory_facts.len();
        self.memory_facts.retain(|f| f.id != id);
        if self.memory_facts.len() != before {
            self.save_memory()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn clear_all_memory(&mut self) -> io::Result<()> {
        self.messages.clear();
        self.memory_facts.clear();
        let empty = "[]";
        write_private_atomic(&self.session_path(), empty.as_bytes())?;
        write_private_atomic(&self.memory_path(), empty.as_bytes())?;
        Ok(())
    }

    pub fn record_character_transition(
        &mut self,
        from_name: &str,
        to_name: &str,
        to_expr: CompanionExpression,
        persist_chat: bool,
    ) -> io::Result<bool> {
        if self.messages.is_empty() {
            return Ok(false);
        }
        let note = format!(
            "[Karakter geçişi: {} -> {} ({})]",
            from_name.trim(),
            to_name.trim(),
            to_expr.title_tr()
        );
        if self
            .messages
            .last()
            .is_some_and(|m| m.role == ChatRole::SystemNote && m.content == note)
        {
            return Ok(false);
        }
        self.append_message(
            ChatMessage {
                role: ChatRole::SystemNote,
                content: note,
                timestamp: Local::now().to_rfc3339(),
                speaker_name: to_name.trim().to_string(),
                suggested_actions: Vec::new(),
                thinking_text: String::new(),
                file_proposals: Vec::new(),
            },
            persist_chat,
        )?;
        Ok(true)
    }
}

fn truncate_fact_chars(s: &str, max_chars: usize) -> String {
    let normalized = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = normalized.chars().take(max_chars).collect();
    out = out.trim().to_string();
    out
}

/// Detects explicit user memory commands such as `"hatırla: ..."` or `"not al: ..."`.
pub fn extract_user_memory_command(input: &str) -> Option<String> {
    let (prefix, rest) = input.trim().split_once(':')?;
    let normalized = prefix
        .trim()
        .to_lowercase()
        .replace("i\u{307}", "i")
        .replace('ı', "i");
    if matches!(normalized.as_str(), "hatirla" | "not al" | "unutma") {
        let cleaned = rest.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}

/// Extracts optional `[HAFIZA: ...]` or `[MEMORY: ...]` tags from a model reply,
/// returning `(cleaned_reply_text, extracted_facts)`.
pub fn extract_memory_tags_from_reply(reply: &str) -> (String, Vec<String>) {
    let mut cleaned = String::with_capacity(reply.len());
    let mut facts = Vec::new();
    let mut cursor = 0;

    while let Some(open_rel) = reply[cursor..].find('[') {
        let open_idx = cursor + open_rel;
        if let Some(close_rel) = reply[open_idx..].find(']') {
            let close_idx = open_idx + close_rel;
            let inner = &reply[open_idx + 1..close_idx];
            if let Some((tag, rest)) = inner.trim().split_once(':') {
                let tag_norm = tag
                    .trim()
                    .to_lowercase()
                    .replace("i\u{307}", "i")
                    .replace('ı', "i");
                if tag_norm == "hafiza" || tag_norm == "memory" {
                    cleaned.push_str(&reply[cursor..open_idx]);
                    let fact = rest.trim();
                    if !fact.is_empty() {
                        facts.push(fact.to_string());
                    }
                    cursor = close_idx + 1;
                    continue;
                }
            }
        }
        cleaned.push_str(&reply[cursor..open_idx + 1]);
        cursor = open_idx + 1;
    }
    cleaned.push_str(&reply[cursor..]);

    let normalized_lines: Vec<&str> = cleaned
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>();
    let final_text = normalized_lines.join("\n").trim().to_string();
    (final_text, facts)
}

/// Builds the 2-layer system prompt combining immutable technical grounding,
/// optional BlazeOS guide + local Brain memory + safe tool instructions, and active persona.
pub fn build_system_prompt(cfg: &OmnibarConfig, memory_facts: &[MemoryFact]) -> String {
    build_system_prompt_with_brain(cfg, memory_facts, "")
}

pub fn build_system_prompt_with_brain(
    cfg: &OmnibarConfig,
    memory_facts: &[MemoryFact],
    brain_context: &str,
) -> String {
    let mut parts = Vec::new();

    // Layer 1: Immutable technical & safety core
    parts.push(
        "Sen BlazeOS / SolarEvolution masaüstünde çalışan yerel bir asistansın. \
Teknik olarak doğru, dürüst, kısa ve net yanıt ver. Bilmediğin bir bayrağı veya \
komutu uydurma; veri kaybı yaratabilecek komutlarda kullanıcıyı uyar. \
Emoji ve gereksiz süslü ifade kullanma. Kullanıcı Türkçe yazarsa Türkçe, \
İngilizce yazarsa İngilizce yanıtla."
            .to_string(),
    );

    if cfg.ai_local_memory_enabled {
        parts.push(
            "Kullanıcı kalıcı bir tercih veya sistem bilgisi paylaşırsa yanıtının sonuna \
en fazla bir adet [HAFIZA: kısa gerçek] etiketi ekleyebilirsin; gerekmiyorsa ekleme."
                .to_string(),
        );
    }

    if cfg.ai_web_research_enabled {
        parts.push(
            "Güncel internet bilgisi (haber, hava durumu, yeni yazılım sürümü vb.) gerektiren sorularda \
tahmin yürütmek yerine yalnızca şu tek etiketi üret: [WEB_ARA: kısa arama sorgusu]. Sistem aramayı \
yapıp sonuçları sana geri verecektir."
                .to_string(),
        );
    }

    if cfg.ai_agent_tools_enabled {
        parts.push(
            "Kullanıcı senden bilgisayarında bir metin/kod/belge dosyası oluşturmanı açıkça isterse \
dosya içeriğini şu blokla öner (kullanıcı onaylamadan diske yazılmaz): \
[DOSYA_ONER: ~/Documents/dosya.md]\\niçerik\\n[/DOSYA_ONER]"
                .to_string(),
        );
    }

    if cfg.ai_include_blazeos_guide {
        parts.push(BLAZEOS_SYSTEM_GUIDE.to_string());
    }

    if cfg.ai_local_memory_enabled && !memory_facts.is_empty() {
        let mut mem_block = String::from("Yerel kullanıcı hafızası:");
        for fact in memory_facts {
            mem_block.push_str("\n- ");
            mem_block.push_str(&fact.text);
        }
        parts.push(mem_block);
    }

    if cfg.ai_local_memory_enabled && !brain_context.trim().is_empty() {
        parts.push(brain_context.trim().to_string());
    }

    // Layer 2: Dynamic persona based on character name + expression archetype
    let name = cfg.effective_companion_name();
    let lower_name = name.to_lowercase();
    let expr = cfg.companion_expression;

    let persona_block = if lower_name == "persephone" {
        format!(
            "Karakter kimliği: Adın Persephone. Dengeli, dost canlısı ve korumacısın; \
riskli adımlarda nazikçe uyarır, net ve güven verici teknik rehberlik sunarsın. \
Aktif ifade tonu ({}): {}",
            expr.title_tr(),
            expr.persona_prompt_tr()
        )
    } else if lower_name == "köz" || lower_name == "koz" {
        format!(
            "Karakter kimliği: Adın Köz. Ciddi, doğrudan ve tavizsiz bir mühendislik \
eleştirmenisin; zayıf fikirleri, üşengeç çözümleri ve güvenlik hatalarını açıkça \
yüzüne vurur, hemen ardından en sağlam teknik çözümünü söylersin. \
Aktif ifade tonu ({}): {}",
            expr.title_tr(),
            expr.persona_prompt_tr()
        )
    } else if lower_name == "kıvılcım" || lower_name == "kivilcim" {
        format!(
            "Karakter kimliği: Adın Kıvılcım. Meraklı, çevik ve analitik bir araştırmacısın. \
Aktif ifade tonu ({}): {}",
            expr.title_tr(),
            expr.persona_prompt_tr()
        )
    } else {
        format!(
            "Karakter kimliği: Adın {}. Aktif arketip ({}): {}",
            name,
            expr.title_tr(),
            expr.persona_prompt_tr()
        )
    };
    parts.push(persona_block);

    parts.join("\n\n")
}

/// Suggests safe, strictly allowlisted 1-click desktop launcher buttons relevant
/// to the user's question and assistant reply. Never produces arbitrary shell commands.
pub fn suggest_safe_actions(user_prompt: &str, reply_text: &str) -> Vec<SuggestedAction> {
    let hay = format!("{}\n{}", user_prompt, reply_text).to_lowercase();
    let rules: &[(&[&str], &str, &str)] = &[
        (
            &[
                "akrep",
                "güvenlik",
                "guvenlik",
                "selinux",
                "güvenlik duvarı",
                "guvenlik duvari",
                "firewall",
                "clamav",
                "flatpak",
            ],
            "Akrep güvenlik merkezini aç",
            "akrep-ui",
        ),
        (
            &[
                "ayar",
                "settings",
                "ekran",
                "ölçek",
                "duvar kağıdı",
                "duvar kagidi",
                "kabuk",
                "noctalia",
                "caelestia",
                "end4",
                "we10x",
            ],
            "SolarUI ayarlarını aç",
            "solar-shell settings",
        ),
        (
            &["kısayol", "kisayol", "tuş", "shortcut", "rehber", "welcome"],
            "Kısayol rehberini aç",
            "solar-shell welcome",
        ),
        (
            &["firehub", "mağaza", "magaza", "portadora", "flathub", "uygulama kur"],
            "FireHub mağazasını aç",
            "firehub",
        ),
        (
            &["gamezone", "oyun", "steam", "lutris", "proton", "super+g"],
            "GameZone'u aç",
            "solar-shell gamezone",
        ),
    ];

    let mut out = Vec::new();
    for (keywords, label, cmd) in rules {
        if keywords.iter().any(|kw| hay.contains(kw)) {
            out.push(SuggestedAction {
                label: (*label).to_string(),
                command: (*cmd).to_string(),
            });
            if out.len() >= 2 {
                break;
            }
        }
    }
    out
}

/// Executes only the strictly allowlisted safe desktop application commands.
pub fn execute_suggested_action(command: &str) -> bool {
    match command.trim() {
        "solar-shell settings" => Command::new("solar-shell").arg("settings").spawn().is_ok(),
        "solar-shell welcome" => Command::new("solar-shell").arg("welcome").spawn().is_ok(),
        "akrep-ui" => Command::new("akrep-ui").spawn().is_ok(),
        "firehub" => Command::new("firehub").spawn().is_ok(),
        "solar-shell gamezone" => Command::new("solar-shell").arg("gamezone").spawn().is_ok(),
        _ => false,
    }
}

pub fn url_encode_query(input: &str) -> String {
    let mut encoded = String::new();
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{:02X}", b)),
        }
    }
    encoded
}

pub fn build_web_search_url(engine: &str, query: &str) -> String {
    let encoded = url_encode_query(query);
    match engine {
        "Google" => format!("https://www.google.com/search?q={}", encoded),
        "Brave" => format!("https://search.brave.com/search?q={}", encoded),
        "Bing" => format!("https://www.bing.com/search?q={}", encoded),
        _ => format!("https://duckduckgo.com/?q={}", encoded),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiDispatchMode {
    ApiCall,
    FallbackToWebSearch { url: String, engine: String },
}

pub fn resolve_dispatch_mode(
    cfg: &OmnibarConfig,
    secrets: &BlazeAiSecrets,
    query: &str,
) -> AiDispatchMode {
    if !cfg.ai_provider.requires_api_key() || secrets.has_key_for(cfg.ai_provider) {
        AiDispatchMode::ApiCall
    } else {
        AiDispatchMode::FallbackToWebSearch {
            url: build_web_search_url(&cfg.search_engine, query),
            engine: cfg.search_engine.clone(),
        }
    }
}

pub fn curated_models_for_provider(provider: BlazeAiProvider) -> &'static [&'static str] {
    match provider {
        BlazeAiProvider::Gemini => &[
            "gemini-3.8-flash",
            "gemini-3.8-pro",
            "gemini-3.5-flash",
            "gemini-2.5-pro",
        ],
        BlazeAiProvider::OpenAi => &["gpt-4o-mini", "gpt-4o", "o4-mini"],
        BlazeAiProvider::Anthropic => &[
            "claude-3-5-sonnet-latest",
            "claude-3-5-haiku-latest",
            "claude-3-7-sonnet-latest",
        ],
        BlazeAiProvider::OpenAiCompatible => &[
            "openai/gpt-4o-mini",
            "deepseek/deepseek-chat",
            "meta-llama/llama-3.3-70b-instruct",
        ],
        BlazeAiProvider::Ollama => &["llama3.2", "qwen2.5:7b", "mistral"],
    }
}

/// Extracts a recommended replacement model ID from Gemini HTTP 404 deprecation errors,
/// e.g. `"Please update your code to use models/gemini-3.8-flash for the latest features"`.
pub fn extract_recommended_gemini_model(err_msg: &str) -> Option<String> {
    let lower = err_msg.to_ascii_lowercase();
    let needle = "use models/";
    let idx = lower.find(needle)?;
    let tail = &err_msg[idx + needle.len()..];
    let raw: String = tail
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .collect();
    let cleaned = raw.trim_end_matches('.');
    if cleaned.starts_with("gemini-") && cleaned.len() >= 9 {
        Some(cleaned.to_string())
    } else {
        None
    }
}

/// Parses Google AI Studio `/v1beta/models` JSON and returns chat-capable Gemini models
/// supporting `generateContent`, excluding embedding/media/specialized endpoints.
pub fn parse_gemini_models_list(raw_body: &str) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_body) else {
        return Vec::new();
    };
    let Some(arr) = val.get("models").and_then(|m| m.as_array()) else {
        return Vec::new();
    };
    let mut models: Vec<String> = Vec::new();
    for item in arr {
        let supports_generate = item
            .get("supportedGenerationMethods")
            .and_then(|m| m.as_array())
            .is_some_and(|methods| {
                methods
                    .iter()
                    .any(|m| m.as_str() == Some("generateContent"))
            });
        if !supports_generate {
            continue;
        }
        let Some(full_name) = item.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let short = full_name.strip_prefix("models/").unwrap_or(full_name).trim();
        let lower = short.to_ascii_lowercase();
        if !lower.starts_with("gemini-") {
            continue;
        }
        if lower.contains("embedding")
            || lower.contains("imagen")
            || lower.contains("veo")
            || lower.contains("tts")
            || lower.contains("audio")
            || lower.contains("robotics")
            || lower.contains("lyria")
            || lower.contains("aqa")
            || lower.contains("bisect")
        {
            continue;
        }
        if !models.iter().any(|existing| existing == short) {
            models.push(short.to_string());
        }
    }
    models.sort_by(|a, b| b.cmp(a));
    models.truncate(12);
    models
}

/// Parses OpenAI or OpenAI-compatible `/models` JSON (`{"data": [{"id": "..."}]}`).
pub fn parse_openai_models_list(raw_body: &str, strict_openai_filter: bool) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_body) else {
        return Vec::new();
    };
    let Some(arr) = val.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut models: Vec<String> = Vec::new();
    for item in arr {
        let Some(id) = item.get("id").and_then(|s| s.as_str()).map(str::trim) else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let lower = id.to_ascii_lowercase();
        if lower.contains("embedding")
            || lower.contains("tts")
            || lower.contains("whisper")
            || lower.contains("dall-e")
            || lower.contains("moderation")
            || lower.contains("transcribe")
            || lower.contains("realtime")
            || lower.contains("audio")
            || lower.contains("image")
            || lower.contains("sora")
            || lower.contains("rerank")
            || lower.contains("davinci")
            || lower.contains("babbage")
            || lower.contains("veo")
        {
            continue;
        }
        if strict_openai_filter {
            let is_chat = lower.starts_with("gpt-")
                || lower.starts_with("chatgpt-")
                || lower.starts_with("o1")
                || lower.starts_with("o3")
                || lower.starts_with("o4")
                || lower.starts_with("o5");
            if !is_chat || lower.contains("-search") {
                continue;
            }
        }
        if !models.iter().any(|existing| existing == id) {
            models.push(id.to_string());
        }
    }
    if strict_openai_filter {
        models.sort_by(|a, b| b.cmp(a));
    }
    models.truncate(24);
    models
}

/// Parses Anthropic `/v1/models` JSON (`{"data": [{"id": "claude-..."}]}`).
pub fn parse_anthropic_models_list(raw_body: &str) -> Vec<String> {
    let Ok(val) = serde_json::from_str::<serde_json::Value>(raw_body) else {
        return Vec::new();
    };
    let Some(arr) = val.get("data").and_then(|d| d.as_array()) else {
        return Vec::new();
    };
    let mut models: Vec<String> = Vec::new();
    for item in arr {
        let Some(id) = item.get("id").and_then(|s| s.as_str()).map(str::trim) else {
            continue;
        };
        if !id.is_empty() && !models.iter().any(|existing| existing == id) {
            models.push(id.to_string());
        }
    }
    models.truncate(20);
    models
}

fn execute_get_with_status(
    url: &str,
    headers: &[(&str, String)],
    timeout_secs: &str,
) -> Result<(u16, String), String> {
    let mut cfg_stdin = format!("url = \"{}\"\n", escape_curl_config_value(url));
    for (name, val) in headers {
        let header_line = format!("{}: {}", name.trim(), val);
        cfg_stdin.push_str(&format!(
            "header = \"{}\"\n",
            escape_curl_config_value(&header_line)
        ));
    }
    cfg_stdin.push_str("write-out = \"\\n__BLAZEAI_HTTP_STATUS:%{http_code}\"\n");

    let mut child = Command::new("/usr/bin/curl")
        .args(["-sS", "--max-time", timeout_secs, "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl başlatılamadı: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cfg_stdin.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("Model listesi isteği başarısız: {}", e))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Sağlayıcıya bağlanılamadı".to_string()
        } else {
            format!("Ağ hatası: {}", stderr)
        });
    }
    let full_stdout = String::from_utf8_lossy(&out.stdout);
    let (raw_body, http_code) =
        if let Some((body_part, code_part)) = full_stdout.rsplit_once("\n__BLAZEAI_HTTP_STATUS:") {
            (
                body_part.trim().to_string(),
                code_part.trim().parse::<u16>().unwrap_or(0),
            )
        } else {
            (full_stdout.trim().to_string(), 0)
        };
    Ok((http_code, raw_body))
}

/// Probes available Google AI Studio models for the configured API key via stdin curl config.
pub fn probe_gemini_models(api_key: &str) -> Result<Vec<String>, String> {
    let clean_key = api_key.trim().replace(['\r', '\n'], "");
    if clean_key.is_empty() {
        return Ok(Vec::new());
    }
    let url = "https://generativelanguage.googleapis.com/v1beta/models?pageSize=100";
    let (http_code, body) =
        execute_get_with_status(url, &[("x-goog-api-key", clean_key)], "6")?;
    if http_code >= 400 {
        let api_err = parse_provider_response(BlazeAiProvider::Gemini, &body)
            .err()
            .unwrap_or_else(|| format!("HTTP {} hatası", http_code));
        return Err(format!("HTTP {}: {}", http_code, api_err));
    }
    Ok(parse_gemini_models_list(&body))
}

/// Probes live supported models for any configured provider using its real `/models` or `/api/tags` endpoint.
pub fn probe_provider_models(
    provider: BlazeAiProvider,
    api_key: &str,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let clean_key = api_key.trim().replace(['\r', '\n'], "");
    match provider {
        BlazeAiProvider::Gemini => probe_gemini_models(&clean_key),
        BlazeAiProvider::OpenAi => {
            if clean_key.is_empty() {
                return Ok(Vec::new());
            }
            let (http_code, body) = execute_get_with_status(
                "https://api.openai.com/v1/models",
                &[("Authorization", format!("Bearer {}", clean_key))],
                "6",
            )?;
            if http_code >= 400 {
                let api_err = parse_provider_response(BlazeAiProvider::OpenAi, &body)
                    .err()
                    .unwrap_or_else(|| format!("HTTP {} hatası", http_code));
                return Err(format!("HTTP {}: {}", http_code, api_err));
            }
            Ok(parse_openai_models_list(&body, true))
        }
        BlazeAiProvider::Anthropic => {
            if clean_key.is_empty() {
                return Ok(Vec::new());
            }
            let (http_code, body) = execute_get_with_status(
                "https://api.anthropic.com/v1/models?limit=100",
                &[
                    ("x-api-key", clean_key),
                    ("anthropic-version", "2023-06-01".to_string()),
                ],
                "6",
            )?;
            if http_code >= 400 {
                let api_err = parse_provider_response(BlazeAiProvider::Anthropic, &body)
                    .err()
                    .unwrap_or_else(|| format!("HTTP {} hatası", http_code));
                return Err(format!("HTTP {}: {}", http_code, api_err));
            }
            Ok(parse_anthropic_models_list(&body))
        }
        BlazeAiProvider::OpenAiCompatible => {
            if clean_key.is_empty() {
                return Ok(Vec::new());
            }
            let raw_base = if base_url.trim().is_empty() {
                provider.default_base_url()
            } else {
                base_url.trim()
            };
            let valid_base = validate_http_url(raw_base)?;
            let trimmed = valid_base.trim_end_matches('/');
            let root = trimmed
                .strip_suffix("/chat/completions")
                .unwrap_or(trimmed)
                .trim_end_matches('/');
            let url = if root.ends_with("/models") {
                root.to_string()
            } else {
                format!("{}/models", root)
            };
            let (http_code, body) = execute_get_with_status(
                &url,
                &[("Authorization", format!("Bearer {}", clean_key))],
                "6",
            )?;
            if http_code >= 400 {
                let api_err = parse_provider_response(BlazeAiProvider::OpenAiCompatible, &body)
                    .err()
                    .unwrap_or_else(|| format!("HTTP {} hatası", http_code));
                return Err(format!("HTTP {}: {}", http_code, api_err));
            }
            Ok(parse_openai_models_list(&body, false))
        }
        BlazeAiProvider::Ollama => {
            let raw_base = if base_url.trim().is_empty() {
                provider.default_base_url()
            } else {
                base_url.trim()
            };
            probe_ollama_models(raw_base)
        }
    }
}

/// Picks the best model from a live-discovered list while preserving the user's current choice if still valid.
pub fn pick_preferred_live_model(
    provider: BlazeAiProvider,
    current_model: &str,
    live_models: &[String],
) -> Option<String> {
    if live_models.is_empty() {
        return None;
    }
    let cur = current_model.trim();
    if !cur.is_empty() && live_models.iter().any(|m| m == cur) {
        return Some(cur.to_string());
    }
    match provider {
        BlazeAiProvider::Gemini => live_models
            .iter()
            .find(|m| {
                let l = m.to_ascii_lowercase();
                l.contains("flash")
                    && !l.contains("lite")
                    && !l.contains("preview")
                    && !l.contains("exp")
            })
            .or_else(|| {
                live_models
                    .iter()
                    .find(|m| m.to_ascii_lowercase().contains("flash"))
            })
            .or_else(|| live_models.first())
            .cloned(),
        BlazeAiProvider::OpenAi => live_models
            .iter()
            .find(|m| {
                let l = m.to_ascii_lowercase();
                l.starts_with("gpt-4o-mini") || l.starts_with("gpt-4.1-mini") || l.starts_with("gpt-5-mini")
            })
            .or_else(|| live_models.iter().find(|m| m.to_ascii_lowercase().starts_with("gpt-")))
            .or_else(|| live_models.first())
            .cloned(),
        BlazeAiProvider::Anthropic => live_models
            .iter()
            .find(|m| m.to_ascii_lowercase().contains("sonnet"))
            .or_else(|| live_models.first())
            .cloned(),
        BlazeAiProvider::OpenAiCompatible | BlazeAiProvider::Ollama => {
            live_models.first().cloned()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderEndpoint {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

pub fn validate_http_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Boş API adresi".to_string());
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("API adresinde geçersiz boşluk veya kontrol karakteri var".to_string());
    }
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
        return Err("Yalnızca http:// veya https:// API adresleri desteklenir".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn build_provider_endpoint(
    cfg: &OmnibarConfig,
    api_key: &str,
) -> Result<ProviderEndpoint, String> {
    let model = cfg.effective_ai_model();
    let clean_key = api_key.trim().replace(['\r', '\n'], "");
    match cfg.ai_provider {
        BlazeAiProvider::Gemini => {
            if model.chars().any(|c| c.is_whitespace() || c == '/' || c == '?' || c == '#') {
                return Err("Geçersiz Gemini model adı".to_string());
            }
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                model
            );
            Ok(ProviderEndpoint {
                url,
                headers: vec![
                    ("Content-Type".to_string(), "application/json".to_string()),
                    ("x-goog-api-key".to_string(), clean_key),
                ],
            })
        }
        BlazeAiProvider::OpenAi => Ok(ProviderEndpoint {
            url: "https://api.openai.com/v1/chat/completions".to_string(),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Authorization".to_string(), format!("Bearer {}", clean_key)),
            ],
        }),
        BlazeAiProvider::Anthropic => Ok(ProviderEndpoint {
            url: "https://api.anthropic.com/v1/messages".to_string(),
            headers: vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("x-api-key".to_string(), clean_key),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
        }),
        BlazeAiProvider::OpenAiCompatible => {
            let base = validate_http_url(&cfg.effective_ai_base_url())?;
            let base_trimmed = base.trim_end_matches('/');
            let url = if base_trimmed.ends_with("/chat/completions") {
                base_trimmed.to_string()
            } else {
                format!("{}/chat/completions", base_trimmed)
            };
            let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
            if !clean_key.is_empty() {
                headers.push(("Authorization".to_string(), format!("Bearer {}", clean_key)));
            }
            Ok(ProviderEndpoint { url, headers })
        }
        BlazeAiProvider::Ollama => {
            let base = validate_http_url(&cfg.effective_ai_base_url())?;
            let base_trimmed = base.trim_end_matches('/');
            let url = if base_trimmed.ends_with("/api/chat") {
                base_trimmed.to_string()
            } else {
                format!("{}/api/chat", base_trimmed)
            };
            Ok(ProviderEndpoint {
                url,
                headers: vec![("Content-Type".to_string(), "application/json".to_string())],
            })
        }
    }
}

fn normalized_dialogue_turns(history: &[ChatMessage]) -> Vec<(&'static str, String)> {
    let mut turns: Vec<(&'static str, String)> = Vec::new();
    for msg in history {
        let trimmed = msg.content.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (role, text) = match msg.role {
            ChatRole::User => ("user", trimmed.to_string()),
            ChatRole::Assistant => ("assistant", trimmed.to_string()),
            ChatRole::SystemNote => ("user", format!("[Sistem notu] {}", trimmed)),
        };
        if let Some((prev_role, prev_text)) = turns.last_mut() {
            if *prev_role == role {
                prev_text.push_str("\n\n");
                prev_text.push_str(&text);
                continue;
            }
        }
        turns.push((role, text));
    }
    turns
}

pub fn extract_thinking_tags(raw: &str) -> (String, String) {
    let mut cleaned = String::with_capacity(raw.len());
    let mut thoughts = Vec::new();
    let mut cursor = 0;

    while let Some(open_rel) = raw[cursor..].find("<think>") {
        let open_idx = cursor + open_rel;
        cleaned.push_str(&raw[cursor..open_idx]);
        let body_start = open_idx + "<think>".len();
        if let Some(close_rel) = raw[body_start..].find("</think>") {
            let close_idx = body_start + close_rel;
            let t = raw[body_start..close_idx].trim();
            if !t.is_empty() {
                thoughts.push(t.to_string());
            }
            cursor = close_idx + "</think>".len();
        } else {
            let t = raw[body_start..].trim();
            if !t.is_empty() {
                thoughts.push(t.to_string());
            }
            cursor = raw.len();
            break;
        }
    }
    cleaned.push_str(&raw[cursor..]);
    (cleaned.trim().to_string(), thoughts.join("\n\n"))
}

pub fn build_provider_request_payload(
    cfg: &OmnibarConfig,
    system_prompt: &str,
    history: &[ChatMessage],
) -> serde_json::Value {
    build_provider_request_payload_with_vision(cfg, system_prompt, history, None)
}

pub fn build_provider_request_payload_with_vision(
    cfg: &OmnibarConfig,
    system_prompt: &str,
    history: &[ChatMessage],
    vision: Option<&VisionAttachment>,
) -> serde_json::Value {
    let model = cfg.effective_ai_model();
    let turns = normalized_dialogue_turns(history);
    let last_user_idx = turns.iter().rposition(|(role, _)| *role == "user");

    match cfg.ai_provider {
        BlazeAiProvider::Gemini => {
            let contents: Vec<serde_json::Value> = turns
                .into_iter()
                .enumerate()
                .map(|(idx, (role, text))| {
                    let g_role = if role == "assistant" { "model" } else { "user" };
                    let mut parts = vec![serde_json::json!({ "text": text })];
                    if Some(idx) == last_user_idx {
                        if let Some(img) = vision {
                            parts.push(serde_json::json!({
                                "inline_data": {
                                    "mime_type": img.mime_type,
                                    "data": img.base64_data
                                }
                            }));
                        }
                    }
                    serde_json::json!({
                        "role": g_role,
                        "parts": parts
                    })
                })
                .collect();
            serde_json::json!({
                "system_instruction": {
                    "parts": [{ "text": system_prompt }]
                },
                "contents": contents,
                "generationConfig": {
                    "temperature": 0.6
                }
            })
        }
        BlazeAiProvider::OpenAi | BlazeAiProvider::OpenAiCompatible => {
            let mut msgs = vec![serde_json::json!({
                "role": "system",
                "content": system_prompt
            })];
            for (idx, (role, text)) in turns.into_iter().enumerate() {
                if Some(idx) == last_user_idx {
                    if let Some(img) = vision {
                        msgs.push(serde_json::json!({
                            "role": role,
                            "content": [
                                { "type": "text", "text": text },
                                {
                                    "type": "image_url",
                                    "image_url": {
                                        "url": format!("data:{};base64,{}", img.mime_type, img.base64_data)
                                    }
                                }
                            ]
                        }));
                        continue;
                    }
                }
                msgs.push(serde_json::json!({
                    "role": role,
                    "content": text
                }));
            }
            serde_json::json!({
                "model": model,
                "messages": msgs
            })
        }
        BlazeAiProvider::Anthropic => {
            let mut msgs = Vec::new();
            for (idx, (role, text)) in turns.into_iter().enumerate() {
                if msgs.is_empty() && role != "user" {
                    continue;
                }
                if Some(idx) == last_user_idx {
                    if let Some(img) = vision {
                        msgs.push(serde_json::json!({
                            "role": role,
                            "content": [
                                {
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": img.mime_type,
                                        "data": img.base64_data
                                    }
                                },
                                { "type": "text", "text": text }
                            ]
                        }));
                        continue;
                    }
                }
                msgs.push(serde_json::json!({
                    "role": role,
                    "content": text
                }));
            }
            serde_json::json!({
                "model": model,
                "max_tokens": 1024,
                "system": system_prompt,
                "messages": msgs
            })
        }
        BlazeAiProvider::Ollama => {
            let mut msgs = vec![serde_json::json!({
                "role": "system",
                "content": system_prompt
            })];
            for (idx, (role, text)) in turns.into_iter().enumerate() {
                if Some(idx) == last_user_idx {
                    if let Some(img) = vision {
                        msgs.push(serde_json::json!({
                            "role": role,
                            "content": text,
                            "images": [img.base64_data]
                        }));
                        continue;
                    }
                }
                msgs.push(serde_json::json!({
                    "role": role,
                    "content": text
                }));
            }
            let mut body = serde_json::json!({
                "model": model,
                "stream": false,
                "keep_alive": cfg.effective_local_keep_alive(),
                "messages": msgs
            });
            if model_supports_thinking(&model) {
                body["think"] = serde_json::Value::Bool(cfg.ai_thinking_mode);
            }
            body
        }
    }
}

pub fn parse_provider_response(provider: BlazeAiProvider, raw_body: &str) -> Result<String, String> {
    parse_provider_response_with_thinking(provider, raw_body).map(|(text, _)| text)
}

pub fn parse_provider_response_with_thinking(
    provider: BlazeAiProvider,
    raw_body: &str,
) -> Result<(String, String), String> {
    let val: serde_json::Value = serde_json::from_str(raw_body)
        .map_err(|e| format!("Geçersiz JSON yanıtı: {}", e))?;

    if let Some(err_msg) = val
        .get("error")
        .and_then(|e| e.get("message").or(Some(e)))
        .and_then(|m| m.as_str())
    {
        if !err_msg.trim().is_empty() {
            return Err(err_msg.trim().to_string());
        }
    }

    let mut native_thinking = String::new();
    let extracted = match provider {
        BlazeAiProvider::Gemini => val
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        BlazeAiProvider::OpenAi | BlazeAiProvider::OpenAiCompatible => val
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|choice| choice.get("message"))
            .and_then(|m| {
                if let Some(r) = m.get("reasoning_content").and_then(|t| t.as_str()) {
                    native_thinking = r.trim().to_string();
                }
                m.get("content")
            })
            .and_then(|t| t.as_str())
            .map(|s| s.to_string()),
        BlazeAiProvider::Anthropic => val
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
        BlazeAiProvider::Ollama => {
            if let Some(t) = val
                .get("message")
                .and_then(|m| m.get("thinking"))
                .and_then(|t| t.as_str())
            {
                native_thinking = t.trim().to_string();
            }
            val.get("message")
                .and_then(|m| m.get("content"))
                .and_then(|t| t.as_str())
                .or_else(|| val.get("response").and_then(|t| t.as_str()))
                .map(|s| s.to_string())
        }
    };

    match extracted {
        Some(text) if !text.trim().is_empty() => {
            let (clean_text, inline_think) = extract_thinking_tags(text.trim());
            let final_think = if !native_thinking.is_empty() {
                native_thinking
            } else {
                inline_think
            };
            let final_text = if clean_text.is_empty() {
                text.trim().to_string()
            } else {
                clean_text
            };
            Ok((final_text, final_think))
        }
        _ => Err("Sağlayıcı boş veya beklenmeyen formatta yanıt döndürdü".to_string()),
    }
}

pub fn escape_curl_config_value(val: &str) -> String {
    let mut out = String::with_capacity(val.len());
    for ch in val.chars() {
        match ch {
            '\r' | '\n' | '\0' => {}
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out
}

pub fn format_curl_stdin_config(endpoint: &ProviderEndpoint, payload_file: &Path) -> String {
    let mut cfg_text = String::new();
    cfg_text.push_str(&format!(
        "url = \"{}\"\n",
        escape_curl_config_value(&endpoint.url)
    ));
    for (name, val) in &endpoint.headers {
        let header_line = format!("{}: {}", name.trim(), val);
        cfg_text.push_str(&format!(
            "header = \"{}\"\n",
            escape_curl_config_value(&header_line)
        ));
    }
    let payload_arg = format!("@{}", payload_file.to_string_lossy());
    cfg_text.push_str(&format!(
        "data-binary = \"{}\"\n",
        escape_curl_config_value(&payload_arg)
    ));
    cfg_text.push_str("write-out = \"\\n__BLAZEAI_HTTP_STATUS:%{http_code}\"\n");
    cfg_text
}

pub enum ChatCompletionOutcome {
    Reply {
        raw_text: String,
        cleaned_text: String,
        thinking_text: String,
        extracted_facts: Vec<String>,
        suggested_actions: Vec<SuggestedAction>,
        file_proposals: Vec<PendingFileProposal>,
        upgraded_model: Option<String>,
    },
    FallbackToWebSearch {
        url: String,
        engine: String,
    },
}

struct RuntimePayloadGuard {
    path: PathBuf,
}

impl Drop for RuntimePayloadGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn runtime_tmp_dir() -> PathBuf {
    solar_common::get_solar_runtime_dir().join("blazeai")
}

/// Probes locally running Ollama (`/api/tags`) with a fast 2-second timeout.
pub fn probe_ollama_models(base_url: &str) -> Result<Vec<String>, String> {
    let valid_base = validate_http_url(base_url)?;
    let url = format!("{}/api/tags", valid_base.trim_end_matches('/'));
    let cfg_stdin = format!("url = \"{}\"\n", escape_curl_config_value(&url));

    let mut child = Command::new("/usr/bin/curl")
        .args(["-sS", "--max-time", "2", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl başlatılamadı: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(cfg_stdin.as_bytes());
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("Ollama erişim hatası: {}", e))?;
    if !out.status.success() {
        return Err("Ollama servisine ulaşılamadı".to_string());
    }
    let body = String::from_utf8_lossy(&out.stdout);
    let val: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| "Geçersiz Ollama etiket yanıtı".to_string())?;
    let models = val
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.get("name").and_then(|n| n.as_str()))
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(models)
}

fn execute_provider_http(
    cfg: &OmnibarConfig,
    api_key: &str,
    system_prompt: &str,
    history: &[ChatMessage],
    vision: Option<&VisionAttachment>,
) -> Result<(u16, String), String> {
    let endpoint = build_provider_endpoint(cfg, api_key)?;
    let payload = build_provider_request_payload_with_vision(cfg, system_prompt, history, vision);
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|e| format!("İstek JSON hazırlanamadı: {}", e))?;

    let tmp_dir = runtime_tmp_dir();
    ensure_private_dir(&tmp_dir)
        .map_err(|e| format!("Geçici çalışma dizini oluşturulamadı: {}", e))?;
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let payload_path = tmp_dir.join(format!("req-{}-{}.json", std::process::id(), seq));
    write_private_atomic(&payload_path, &payload_bytes)
        .map_err(|e| format!("Geçici istek dosyası yazılamadı: {}", e))?;
    let _guard = RuntimePayloadGuard {
        path: payload_path.clone(),
    };

    let timeout_secs = if cfg.ai_provider == BlazeAiProvider::Ollama {
        "120"
    } else {
        "45"
    };
    let stdin_cfg = format_curl_stdin_config(&endpoint, &payload_path);
    let mut child = Command::new("/usr/bin/curl")
        .args(["-sS", "--max-time", timeout_secs, "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl çalıştırılamadı: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(stdin_cfg.as_bytes())
            .map_err(|e| format!("curl yapılandırması aktarılamadı: {}", e))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("Ağ isteği tamamlanamadı: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Sağlayıcıya bağlanılamadı".to_string()
        } else {
            format!("Ağ hatası: {}", stderr)
        });
    }

    let full_stdout = String::from_utf8_lossy(&output.stdout);
    let (raw_body, http_code) =
        if let Some((body_part, code_part)) = full_stdout.rsplit_once("\n__BLAZEAI_HTTP_STATUS:") {
            (
                body_part.trim().to_string(),
                code_part.trim().parse::<u16>().unwrap_or(0),
            )
        } else {
            (full_stdout.trim().to_string(), 0)
        };
    Ok((http_code, raw_body))
}

/// Executes a non-blocking HTTP request (intended to run inside a background thread)
/// via `/usr/bin/curl -sS -K -`, keeping API keys out of argv/disk.
pub fn send_chat_completion(
    cfg: &OmnibarConfig,
    secrets: &BlazeAiSecrets,
    memory_facts: &[MemoryFact],
    history: &[ChatMessage],
    latest_prompt: &str,
) -> Result<ChatCompletionOutcome, String> {
    send_chat_completion_with_options(
        cfg,
        secrets,
        memory_facts,
        history,
        latest_prompt,
        None,
        false,
    )
}

pub fn send_chat_completion_with_options(
    cfg: &OmnibarConfig,
    secrets: &BlazeAiSecrets,
    memory_facts: &[MemoryFact],
    history: &[ChatMessage],
    latest_prompt: &str,
    vision: Option<&VisionAttachment>,
    force_web_research: bool,
) -> Result<ChatCompletionOutcome, String> {
    match resolve_dispatch_mode(cfg, secrets, latest_prompt) {
        AiDispatchMode::FallbackToWebSearch { url, engine } => {
            return Ok(ChatCompletionOutcome::FallbackToWebSearch { url, engine });
        }
        AiDispatchMode::ApiCall => {}
    }

    // If Ollama is selected, auto-start user-space Ollama if installed; otherwise fall back cleanly
    if cfg.ai_provider == BlazeAiProvider::Ollama {
        let base_url = cfg.effective_ai_base_url();
        if probe_ollama_models(&base_url).is_err() && ensure_ollama_serving(&base_url).is_err() {
            if cfg.enable_web_search {
                return Ok(ChatCompletionOutcome::FallbackToWebSearch {
                    url: build_web_search_url(&cfg.search_engine, latest_prompt),
                    engine: cfg.search_engine.clone(),
                });
            }
            return Err("Yerel Ollama servisi çalışmıyor (127.0.0.1:11434)".to_string());
        }
    }

    let api_key = secrets.key_for(cfg.ai_provider);
    let mut extra_context = if cfg.ai_local_memory_enabled {
        BrainStore::open_default().build_brain_context_block(latest_prompt)
    } else {
        String::new()
    };

    if force_web_research && cfg.ai_web_research_enabled {
        if let Ok(results) = execute_web_research(latest_prompt, 4) {
            let web_block = format_web_research_context(latest_prompt, &results);
            if !web_block.is_empty() {
                if !extra_context.is_empty() {
                    extra_context.push_str("\n\n");
                }
                extra_context.push_str(&web_block);
            }
        }
    }

    let system_prompt = build_system_prompt_with_brain(cfg, memory_facts, &extra_context);
    let (mut http_code, mut raw_body) =
        execute_provider_http(cfg, api_key, &system_prompt, history, vision)?;
    let mut upgraded_model: Option<String> = None;

    // If Google AI Studio reports HTTP 404 for a deprecated Gemini model and suggests a replacement,
    // automatically retry once with the recommended model so chat never silently breaks on deprecation.
    if http_code == 404 && cfg.ai_provider == BlazeAiProvider::Gemini {
        let first_err = parse_provider_response(cfg.ai_provider, &raw_body)
            .err()
            .unwrap_or_default();
        if let Some(recommended) = extract_recommended_gemini_model(&first_err) {
            if recommended != cfg.effective_ai_model() {
                let mut retry_cfg = cfg.clone();
                retry_cfg.ai_model = recommended.clone();
                if let Ok((retry_code, retry_body)) =
                    execute_provider_http(&retry_cfg, api_key, &system_prompt, history, vision)
                {
                    http_code = retry_code;
                    raw_body = retry_body;
                    if retry_code < 400 {
                        upgraded_model = Some(recommended);
                    }
                }
            }
        }
    }

    if http_code >= 400 {
        let api_err = parse_provider_response(cfg.ai_provider, &raw_body)
            .err()
            .unwrap_or_else(|| format!("HTTP {} hatası", http_code));
        return Err(format!("HTTP {}: {}", http_code, api_err));
    }

    let (mut raw_reply, mut thinking_text) =
        parse_provider_response_with_thinking(cfg.ai_provider, &raw_body)?;

    // Single grounded web-research follow-up when model explicitly emits [WEB_ARA: query]
    if cfg.ai_web_research_enabled && !force_web_research {
        if let Some(web_query) = extract_web_search_tag(&raw_reply) {
            if let Ok(results) = execute_web_research(&web_query, 4) {
                let web_block = format_web_research_context(&web_query, &results);
                if !web_block.is_empty() {
                    let grounded_prompt = format!("{}\n\n{}", system_prompt, web_block);
                    if let Ok((code2, body2)) =
                        execute_provider_http(cfg, api_key, &grounded_prompt, history, vision)
                    {
                        if code2 < 400 {
                            if let Ok((reply2, think2)) =
                                parse_provider_response_with_thinking(cfg.ai_provider, &body2)
                            {
                                raw_reply = reply2;
                                if !think2.is_empty() {
                                    thinking_text = think2;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let (no_mem_text, extracted_facts) = extract_memory_tags_from_reply(&raw_reply);
    let (cleaned_text, file_proposals) = if cfg.ai_agent_tools_enabled {
        extract_file_proposals_from_reply(&no_mem_text)
    } else {
        (no_mem_text, Vec::new())
    };
    let final_reply = if cleaned_text.is_empty() && file_proposals.is_empty() {
        raw_reply.clone()
    } else {
        cleaned_text
    };
    let suggested_actions = suggest_safe_actions(latest_prompt, &final_reply);

    Ok(ChatCompletionOutcome::Reply {
        raw_text: raw_reply,
        cleaned_text: final_reply,
        thinking_text,
        extracted_facts,
        suggested_actions,
        file_proposals,
        upgraded_model,
    })
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
            "Refusing symlink for private BlazeAI file",
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

    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".tmp.{}.{}.blazeai", std::process::id(), seq);
    let tmp_path = parent.join(tmp_name);

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

    if let Err(err) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn unique_test_dir(tag: &str) -> PathBuf {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "solar_blazeai_test_{}_{}_{}",
            tag,
            std::process::id(),
            seq
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn secrets_store_uses_0600_permissions_and_migrates_legacy_config_key() {
        let root = unique_test_dir("secrets");
        let secrets_path = root.join("config/solarui/blazeai_secrets.json");

        let mut cfg = SolarConfig::default();
        cfg.omnibar.ai_provider = BlazeAiProvider::Gemini;
        cfg.omnibar.ai_api_key = "AIza-test-secret-key".to_string();

        let migrated = migrate_legacy_api_key_in_memory(&mut cfg, &secrets_path).unwrap();
        assert!(migrated);
        assert!(cfg.omnibar.ai_api_key.is_empty());

        let loaded = BlazeAiSecrets::load_from(&secrets_path);
        assert_eq!(loaded.key_for(BlazeAiProvider::Gemini), "AIza-test-secret-key");

        let file_mode = fs::metadata(&secrets_path).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
        let dir_mode = fs::metadata(secrets_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);

        // Switching providers retains previously saved keys
        let mut updated = loaded.clone();
        updated.set_key_for(BlazeAiProvider::OpenAi, "sk-openai-test");
        updated.save_to(&secrets_path).unwrap();
        let reloaded = BlazeAiSecrets::load_from(&secrets_path);
        assert_eq!(reloaded.key_for(BlazeAiProvider::Gemini), "AIza-test-secret-key");
        assert_eq!(reloaded.key_for(BlazeAiProvider::OpenAi), "sk-openai-test");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn local_memory_persists_with_0600_caps_history_and_extracts_tags() {
        let root = unique_test_dir("memory");
        let mut store = LocalMemoryStore::open_in_dir(root.join("blazeai"), true);

        // User command detection
        assert_eq!(
            extract_user_memory_command("hatırla: ptyxis tercih ediyorum"),
            Some("ptyxis tercih ediyorum".to_string())
        );
        assert_eq!(
            extract_user_memory_command("Not al: Rust projeleri için --offline kullan"),
            Some("Rust projeleri için --offline kullan".to_string())
        );
        assert_eq!(extract_user_memory_command("Merhaba nasılsın"), None);

        // Model reply tag extraction
        let (clean, facts) = extract_memory_tags_from_reply(
            "Tamam, ptyxis komutunu hazırladım.\n[HAFIZA: Kullanıcı terminal olarak ptyxis kullanıyor]\n[MEMORY: Fedora 45 üzerinde Niri aktif]",
        );
        assert_eq!(clean, "Tamam, ptyxis komutunu hazırladım.");
        assert_eq!(facts.len(), 2);

        for fact in facts {
            assert!(store.add_memory_fact(&fact).unwrap());
        }
        // Duplicate fact should be ignored
        assert!(!store
            .add_memory_fact("Kullanıcı terminal olarak ptyxis kullanıyor")
            .unwrap());

        for idx in 0..45 {
            store
                .append_message(
                    ChatMessage {
                        role: ChatRole::User,
                        content: format!("Mesaj {}", idx),
                        timestamp: "2026-11-26T18:00:00Z".to_string(),
                        speaker_name: "Kullanıcı".to_string(),
                        suggested_actions: Vec::new(),
                        thinking_text: String::new(),
                        file_proposals: Vec::new(),
                    },
                    true,
                )
                .unwrap();
        }
        store
            .record_character_transition("Persephone", "Köz", CompanionExpression::Angry, true)
            .unwrap();

        let reloaded = LocalMemoryStore::open_in_dir(root.join("blazeai"), true);
        assert_eq!(reloaded.messages.len(), MAX_SESSION_MESSAGES);
        assert_eq!(reloaded.memory_facts.len(), 2);
        assert!(reloaded
            .messages
            .last()
            .unwrap()
            .content
            .contains("Persephone -> Köz"));

        let mem_mode = fs::metadata(reloaded.memory_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        let ses_mode = fs::metadata(reloaded.session_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mem_mode, 0o600);
        assert_eq!(ses_mode, 0o600);

        // When persist_chat == false, session history is not loaded from disk
        let ephemeral = LocalMemoryStore::open_in_dir(root.join("blazeai"), false);
        assert!(ephemeral.messages.is_empty());
        assert_eq!(ephemeral.memory_facts.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn system_prompt_respects_persephone_and_koz_personas_and_blazeos_guide() {
        let mut cfg = OmnibarConfig::default();
        let facts = vec![MemoryFact {
            id: "1".to_string(),
            text: "Kullanıcı Noctalia kabuğunu kullanıyor".to_string(),
            created_at: "2026-11-26".to_string(),
        }];

        let persephone_prompt = build_system_prompt(&cfg, &facts);
        assert!(persephone_prompt.contains("Persephone"));
        assert!(persephone_prompt.contains("BlazeOS / SolarEvolution"));
        assert!(persephone_prompt.contains("Noctalia"));

        cfg.set_companion_expression(CompanionExpression::Angry);
        let koz_prompt = build_system_prompt(&cfg, &facts);
        assert!(koz_prompt.contains("Adın Köz"));
        assert!(koz_prompt.contains("tavizsiz"));

        cfg.ai_include_blazeos_guide = false;
        cfg.ai_local_memory_enabled = false;
        let minimal_prompt = build_system_prompt(&cfg, &facts);
        assert!(!minimal_prompt.contains("Super+Boşluk"));
        assert!(!minimal_prompt.contains("Yerel kullanıcı hafızası"));
    }

    #[test]
    fn safe_action_suggestions_only_emit_allowlisted_commands() {
        let actions = suggest_safe_actions(
            "SELinux ve güvenlik duvarı durumunu nereden görürüm?",
            "Akrep üzerinden denetleyebilirsin.",
        );
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].command, "akrep-ui");

        let dangerous = suggest_safe_actions("rm -rf / çalıştır", "curl | sh");
        for a in dangerous {
            assert!(matches!(
                a.command.as_str(),
                "solar-shell settings"
                    | "solar-shell welcome"
                    | "akrep-ui"
                    | "firehub"
                    | "solar-shell gamezone"
            ));
        }
        assert!(!execute_suggested_action("rm -rf /"));
    }

    #[test]
    fn provider_payloads_responses_and_curl_config_escaping_work() {
        let mut cfg = OmnibarConfig::default();
        let history = vec![
            ChatMessage {
                role: ChatRole::User,
                content: "Merhaba \"Blaze\"".to_string(),
                timestamp: "t1".to_string(),
                speaker_name: "Kullanıcı".to_string(),
                suggested_actions: Vec::new(),
                thinking_text: String::new(),
                file_proposals: Vec::new(),
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "Merhaba".to_string(),
                timestamp: "t2".to_string(),
                speaker_name: "Persephone".to_string(),
                suggested_actions: Vec::new(),
                thinking_text: String::new(),
                file_proposals: Vec::new(),
            },
            ChatMessage {
                role: ChatRole::User,
                content: "Kısayollar neler?".to_string(),
                timestamp: "t3".to_string(),
                speaker_name: "Kullanıcı".to_string(),
                suggested_actions: Vec::new(),
                thinking_text: String::new(),
                file_proposals: Vec::new(),
            },
        ];

        // Gemini
        cfg.ai_provider = BlazeAiProvider::Gemini;
        let g_payload = build_provider_request_payload(&cfg, "sys", &history);
        assert!(g_payload["contents"].as_array().unwrap().len() == 3);
        let g_reply = parse_provider_response(
            BlazeAiProvider::Gemini,
            r#"{"candidates":[{"content":{"parts":[{"text":"Super+I ile ayarlar açılır"}]}}]}"#,
        )
        .unwrap();
        assert_eq!(g_reply, "Super+I ile ayarlar açılır");

        // OpenAI
        cfg.ai_provider = BlazeAiProvider::OpenAi;
        cfg.ai_model = "gpt-4o-mini".to_string();
        let o_payload = build_provider_request_payload(&cfg, "sys", &history);
        assert_eq!(o_payload["messages"][0]["role"], "system");
        let o_reply = parse_provider_response(
            BlazeAiProvider::OpenAi,
            r#"{"choices":[{"message":{"content":"Yanıt tamam"}}]}"#,
        )
        .unwrap();
        assert_eq!(o_reply, "Yanıt tamam");

        // Anthropic
        cfg.ai_provider = BlazeAiProvider::Anthropic;
        cfg.ai_model = "claude-3-5-sonnet-latest".to_string();
        let a_payload = build_provider_request_payload(&cfg, "sys", &history);
        assert_eq!(a_payload["system"], "sys");
        let a_reply = parse_provider_response(
            BlazeAiProvider::Anthropic,
            r#"{"content":[{"type":"text","text":"Claude yanıtı"}]}"#,
        )
        .unwrap();
        assert_eq!(a_reply, "Claude yanıtı");

        // Ollama
        cfg.ai_provider = BlazeAiProvider::Ollama;
        cfg.ai_model = "llama3.2".to_string();
        let l_payload = build_provider_request_payload(&cfg, "sys", &history);
        assert_eq!(l_payload["stream"], false);
        let l_reply = parse_provider_response(
            BlazeAiProvider::Ollama,
            r#"{"message":{"role":"assistant","content":"Ollama yerel yanıtı"}}"#,
        )
        .unwrap();
        assert_eq!(l_reply, "Ollama yerel yanıtı");

        // Curl -K - config escaping against quotes, backslashes & newlines
        let ep = ProviderEndpoint {
            url: "https://api.openai.com/v1/chat/completions".to_string(),
            headers: vec![(
                "Authorization".to_string(),
                "Bearer sk-\"quote\"\\slash\ninjected".to_string(),
            )],
        };
        let stdin_cfg = format_curl_stdin_config(&ep, Path::new("/run/user/1000/req.json"));
        assert!(!stdin_cfg.contains("injected\nheader"));
        assert!(stdin_cfg.contains("\\\"quote\\\"\\\\slashinjected"));
    }

    #[test]
    fn missing_api_key_dispatches_to_configured_web_search_engine() {
        let mut cfg = OmnibarConfig::default();
        cfg.ai_provider = BlazeAiProvider::Gemini;
        cfg.search_engine = "Google".to_string();
        let secrets = BlazeAiSecrets::default();

        match resolve_dispatch_mode(&cfg, &secrets, "fedora 45 niri") {
            AiDispatchMode::FallbackToWebSearch { url, engine } => {
                assert_eq!(engine, "Google");
                assert_eq!(url, "https://www.google.com/search?q=fedora+45+niri");
            }
            other => panic!("Expected web fallback, got {:?}", other),
        }

        // Ollama does not require an API key
        cfg.ai_provider = BlazeAiProvider::Ollama;
        assert_eq!(
            resolve_dispatch_mode(&cfg, &secrets, "test"),
            AiDispatchMode::ApiCall
        );
    }

    #[test]
    fn turkish_dotted_i_memory_tags_and_loopback_curl_dispatch_work() {
        let (clean, facts) =
            extract_memory_tags_from_reply("Hazır.\n[HAFİZA: Alacritty yerine ptyxis kullan]");
        assert_eq!(clean, "Hazır.");
        assert_eq!(facts, vec!["Alacritty yerine ptyxis kullan".to_string()]);
        assert_eq!(
            extract_user_memory_command("HATİRLA: Btrfs kullanıyorum"),
            Some("Btrfs kullanıyorum".to_string())
        );

        // Real loopback HTTP mock server verifying curl -K - stdin auth header + JSON payload cleanup
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server_handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            assert!(req.contains("Authorization: Bearer test-loopback-token"));
            assert!(req.contains("Akrep nedir"));
            let body = r#"{"choices":[{"message":{"content":"Akrep güvenlik merkezidir. [HAFIZA: Güvenlik odaklı kullanıcı]"}}]}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        });

        let mut cfg = OmnibarConfig::default();
        cfg.ai_provider = BlazeAiProvider::OpenAiCompatible;
        cfg.ai_model = "test-model".to_string();
        cfg.ai_custom_base_url = format!("http://127.0.0.1:{}/v1", port);

        let mut secrets = BlazeAiSecrets::default();
        secrets.set_key_for(BlazeAiProvider::OpenAiCompatible, "test-loopback-token");

        let history = vec![ChatMessage {
            role: ChatRole::User,
            content: "Akrep nedir?".to_string(),
            timestamp: "t1".to_string(),
            speaker_name: "Sen".to_string(),
            suggested_actions: Vec::new(),
            thinking_text: String::new(),
            file_proposals: Vec::new(),
        }];

        let outcome =
            send_chat_completion(&cfg, &secrets, &[], &history, "Akrep nedir?").unwrap();
        server_handle.join().unwrap();

        match outcome {
            ChatCompletionOutcome::Reply {
                cleaned_text,
                extracted_facts,
                suggested_actions,
                ..
            } => {
                assert_eq!(cleaned_text, "Akrep güvenlik merkezidir.");
                assert_eq!(extracted_facts, vec!["Güvenlik odaklı kullanıcı".to_string()]);
                assert!(suggested_actions.iter().any(|a| a.command == "akrep-ui"));
            }
            ChatCompletionOutcome::FallbackToWebSearch { .. } => {
                panic!("Expected Reply from loopback server")
            }
        }

        // Verify no stray temp payload files remain in runtime_tmp_dir
        let tmp_dir = runtime_tmp_dir();
        if tmp_dir.is_dir() {
            let leftovers: Vec<_> = fs::read_dir(&tmp_dir)
                .unwrap()
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("req-"))
                .collect();
            assert!(leftovers.is_empty(), "Leftover temp payload files: {:?}", leftovers);
        }

        // Closed Ollama port with enable_web_search = true falls back cleanly
        let closed_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_port = closed_listener.local_addr().unwrap().port();
        drop(closed_listener);
        cfg.ai_provider = BlazeAiProvider::Ollama;
        cfg.ai_custom_base_url = format!("http://127.0.0.1:{}", closed_port);
        cfg.search_engine = "DuckDuckGo".to_string();
        let offline_outcome =
            send_chat_completion(&cfg, &secrets, &[], &history, "niri keybindings").unwrap();
        match offline_outcome {
            ChatCompletionOutcome::FallbackToWebSearch { url, engine } => {
                assert_eq!(engine, "DuckDuckGo");
                assert!(url.contains("duckduckgo.com/?q=niri+keybindings"));
            }
            ChatCompletionOutcome::Reply { .. } => panic!("Expected fallback when Ollama is offline"),
        }
    }

    #[test]
    fn gemini_deprecation_extractor_and_models_list_parser_work() {
        let err = "This model models/gemini-2.5-flash is no longer available to new users. Please update your code to use models/gemini-3.8-flash for the latest features and improvements.";
        assert_eq!(
            extract_recommended_gemini_model(err),
            Some("gemini-3.8-flash".to_string())
        );
        assert_eq!(extract_recommended_gemini_model("API key not valid"), None);

        let raw_models = r#"{
            "models": [
                {"name": "models/gemini-3.8-flash", "supportedGenerationMethods": ["generateContent"]},
                {"name": "models/gemini-3.8-pro", "supportedGenerationMethods": ["generateContent"]},
                {"name": "models/text-embedding-004", "supportedGenerationMethods": ["embedContent"]},
                {"name": "models/gemini-embedding-001", "supportedGenerationMethods": ["generateContent"]},
                {"name": "models/gemini-2.5-flash-tts", "supportedGenerationMethods": ["generateContent"]},
                {"name": "models/gemini-3.5-flash", "supportedGenerationMethods": ["generateContent"]}
            ]
        }"#;
        let parsed = parse_gemini_models_list(raw_models);
        assert_eq!(
            parsed,
            vec![
                "gemini-3.8-pro".to_string(),
                "gemini-3.8-flash".to_string(),
                "gemini-3.5-flash".to_string(),
            ]
        );
        assert_eq!(
            pick_preferred_live_model(BlazeAiProvider::Gemini, "gemini-2.5-flash", &parsed),
            Some("gemini-3.8-flash".to_string())
        );
        assert_eq!(
            pick_preferred_live_model(BlazeAiProvider::Gemini, "gemini-3.8-pro", &parsed),
            Some("gemini-3.8-pro".to_string())
        );

        let openai_json = r#"{"data":[
            {"id":"gpt-4o-mini"},
            {"id":"gpt-4o"},
            {"id":"text-embedding-3-large"},
            {"id":"whisper-1"},
            {"id":"o4-mini"}
        ]}"#;
        let o_models = parse_openai_models_list(openai_json, true);
        assert!(o_models.contains(&"gpt-4o-mini".to_string()));
        assert!(o_models.contains(&"o4-mini".to_string()));
        assert!(!o_models.iter().any(|m| m.contains("embedding") || m.contains("whisper")));

        let anthropic_json = r#"{"data":[
            {"id":"claude-sonnet-4-5"},
            {"id":"claude-haiku-4-5"}
        ]}"#;
        let a_models = parse_anthropic_models_list(anthropic_json);
        assert_eq!(a_models, vec!["claude-sonnet-4-5".to_string(), "claude-haiku-4-5".to_string()]);
        assert_eq!(
            pick_preferred_live_model(BlazeAiProvider::Anthropic, "", &a_models),
            Some("claude-sonnet-4-5".to_string())
        );

        // Thinking tag extraction + Ollama native thinking + Vision multimodal payloads
        let (clean_t, thought) =
            extract_thinking_tags("<think>Adım 1 analiz</think>\nSonuç hazır.");
        assert_eq!(clean_t, "Sonuç hazır.");
        assert_eq!(thought, "Adım 1 analiz");

        let (ollama_text, ollama_think) = parse_provider_response_with_thinking(
            BlazeAiProvider::Ollama,
            r#"{"message":{"role":"assistant","thinking":"İç akıl yürütme","content":"Kısa yanıt"}}"#,
        )
        .unwrap();
        assert_eq!(ollama_text, "Kısa yanıt");
        assert_eq!(ollama_think, "İç akıl yürütme");

        let vis = VisionAttachment {
            mime_type: "image/jpeg".to_string(),
            base64_data: "AQID".to_string(),
            width: 1280,
            height: 720,
        };
        let hist = vec![ChatMessage {
            role: ChatRole::User,
            content: "Ekranda ne var?".to_string(),
            timestamp: "t1".to_string(),
            speaker_name: "Sen".to_string(),
            suggested_actions: Vec::new(),
            thinking_text: String::new(),
            file_proposals: Vec::new(),
        }];
        let mut o_cfg = OmnibarConfig::default();
        o_cfg.ai_provider = BlazeAiProvider::Ollama;
        o_cfg.ai_model = "qwen3.5:9b".to_string();
        o_cfg.ai_thinking_mode = true;
        let o_pay = build_provider_request_payload_with_vision(&o_cfg, "sys", &hist, Some(&vis));
        assert_eq!(o_pay["think"], true);
        assert_eq!(o_pay["keep_alive"], "5m");
        assert_eq!(o_pay["messages"][1]["images"][0], "AQID");

        let mut g_cfg = OmnibarConfig::default();
        g_cfg.ai_provider = BlazeAiProvider::Gemini;
        let g_pay = build_provider_request_payload_with_vision(&g_cfg, "sys", &hist, Some(&vis));
        assert_eq!(g_pay["contents"][0]["parts"][1]["inline_data"]["data"], "AQID");
    }
}
