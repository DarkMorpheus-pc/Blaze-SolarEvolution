// ==============================================================================
// SolarUI Omnibar & BlazeAI Vector Companion
// - Side-by-side two-pane layout (1040x520 expanded / 680x492 collapsed)
// - Left pane: Persephone/Köz Cairo vector companion + BlazeAI local-memory chat
// - Right pane: Instant Math, Unit/Currency/Crypto, Clipboard, Process, Apps
// ==============================================================================

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, DropDown, Entry, EventControllerKey,
    EventControllerMotion, Expander, IconTheme, Image, Label, ListBox, ListBoxRow, Orientation,
    PasswordEntry, Popover, ProgressBar, ScrolledWindow, Separator, Stack, StringList,
    ToggleButton, Window,
};
use solar_common::{BlazeAiProvider, CompanionExpression, OmnibarConfig, SolarConfig};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::agent_tools::apply_approved_file_proposal;
use crate::apps::{scan_desktop_applications, AppInfo};
use crate::blazeai::{
    build_web_search_url, execute_suggested_action, extract_user_memory_command,
    migrate_legacy_api_key_if_needed, pick_preferred_live_model, probe_ollama_models,
    probe_provider_models, resolve_dispatch_mode, send_chat_completion_with_options,
    AiDispatchMode, BlazeAiSecrets, ChatCompletionOutcome, ChatMessage, ChatRole,
    LocalMemoryStore,
};
use crate::brain_store::BrainStore;
use crate::companion_avatar::{
    build_companion_customizer_popover, CompanionAvatarWidget, CompanionCustomizerCallbacks,
};
use crate::living_flow::{apply_living_flow_theme, FluidCloud, FluidCloudTone};
use crate::local_ai::{
    delete_ollama_model, ensure_ollama_serving, find_ollama_binary, install_userspace_ollama,
    is_ollama_serving, pull_ollama_model, HardwareProfile, PullProgress, LOCAL_MODEL_TIERS,
};
use crate::voice_pipeline::{
    capture_wayland_screen_for_vision, install_turkish_tts_pack, install_whisper_stt_model,
    ActiveRecording, VisionAttachment, VoicePackStatus, VoiceSpeaker, PIPER_VOICE_KOZ,
    PIPER_VOICE_PERSEPHONE,
};

pub const OMNIBAR_OPEN_WIDTH: i32 = 1040;
pub const OMNIBAR_OPEN_HEIGHT: i32 = 520;
pub const OMNIBAR_COLLAPSED_WIDTH: i32 = 680;
pub const OMNIBAR_COLLAPSED_HEIGHT: i32 = 492;

pub const OMNIBAR_EXTRA_CSS: &str = r#"
    .omnibar-card {
        background-color: #141415;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 16px;
        padding: 0px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    }

    .omnibar-left-pane {
        background-color: #0f0f10;
        background-image: none;
        border-right: 1px solid rgba(237, 237, 237, 0.08);
        border-top-left-radius: 16px;
        border-bottom-left-radius: 16px;
        padding: 16px;
    }

    .omnibar-right-pane {
        background-color: transparent;
        padding: 16px 20px;
    }

    .omnibar-header {
        margin-bottom: 12px;
    }

    .omnibar-header-icon {
        color: #85858b;
        -gtk-icon-size: 16px;
    }

    .omnibar-brand-title {
        color: #ededed;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .omnibar-brand-sub {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .companion-name-title {
        color: #ededed;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .companion-archetype-sub {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .model-pill-btn {
        background-color: #19191b;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 4px 10px;
        min-height: 26px;
        font-size: 12px;
        font-weight: 500;
        letter-spacing: 0px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .model-pill-btn:hover {
        background-color: #232325;
        border-color: rgba(237, 237, 237, 0.18);
    }

    .companion-popover-box,
    .blazeai-gear-box {
        padding: 12px;
    }

    .companion-section-lbl {
        color: #a1a1a6;
        font-size: 12px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .companion-meaning-banner {
        background-color: #141415;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 8px;
        padding: 8px 10px;
    }

    .companion-banner-title {
        color: #ededed;
        font-size: 13px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .companion-banner-desc {
        color: #a1a1a6;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .companion-opt-btn {
        background-color: #141415;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 8px;
        padding: 6px 4px;
        min-width: 64px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .companion-opt-btn:hover {
        background-color: #232325;
        border-color: rgba(237, 237, 237, 0.16);
    }

    .companion-opt-btn.companion-opt-active {
        background-color: #232325;
        border-color: rgba(224, 87, 60, 0.85);
    }

    .companion-opt-lbl {
        color: #a1a1a6;
        font-size: 11px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .companion-color-btn {
        background-color: transparent;
        background-image: none;
        border: 2px solid transparent;
        border-radius: 999px;
        min-width: 30px;
        min-height: 30px;
        padding: 2px;
        box-shadow: none;
    }

    .companion-color-btn:hover {
        background-color: #232325;
    }

    .companion-color-btn.companion-color-active {
        border-color: #e0573c;
        background-color: #232325;
    }

    .companion-reset-btn {
        background-color: #232325;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 6px 10px;
        min-height: 32px;
        font-size: 12px;
        font-weight: 500;
        box-shadow: none;
    }

    .companion-reset-btn:hover {
        background-color: #2c2c2f;
    }

    .blazeai-chat-scroll {
        background-color: #141415;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 12px;
        padding: 8px;
    }

    .blazeai-empty-card {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 10px;
        padding: 12px;
    }

    .blazeai-msg-user {
        background-color: #232325;
        border: 1px solid rgba(237, 237, 237, 0.10);
        border-radius: 10px;
        padding: 10px 12px;
    }

    .blazeai-msg-assistant {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 10px;
        padding: 10px 12px;
    }

    .blazeai-msg-speaker {
        color: #ededed;
        font-size: 12px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .blazeai-msg-text {
        color: #ededed;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .blazeai-msg-note {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        padding: 4px 8px;
        letter-spacing: 0px;
    }

    .blazeai-copy-btn {
        background-color: transparent;
        background-image: none;
        color: #85858b;
        border: 1px solid transparent;
        border-radius: 6px;
        min-width: 24px;
        min-height: 24px;
        padding: 0px;
        box-shadow: none;
    }

    .blazeai-tool-bar {
        padding: 2px 0px;
    }

    .blazeai-chip-toggle {
        background-color: #19191b;
        background-image: none;
        color: #a1a1a6;
        border: 1px solid rgba(237, 237, 237, 0.10);
        border-radius: 7px;
        padding: 2px 8px;
        min-height: 26px;
        font-size: 11px;
        font-weight: 500;
        box-shadow: none;
    }

    .blazeai-chip-toggle:hover {
        background-color: #232325;
        color: #ededed;
    }

    .blazeai-chip-toggle:checked,
    .blazeai-mic-active {
        background-color: rgba(224, 87, 60, 0.22);
        border-color: rgba(224, 87, 60, 0.85);
        color: #ededed;
    }

    .blazeai-vision-badge {
        background-color: #19191b;
        border: 1px solid rgba(224, 87, 60, 0.45);
        border-radius: 8px;
        padding: 4px 8px;
    }

    .blazeai-proposal-card {
        background-color: #141415;
        border: 1px solid rgba(224, 87, 60, 0.40);
        border-radius: 8px;
        padding: 8px;
        margin-top: 4px;
    }

    .blazeai-proposal-code {
        color: #a1a1a6;
        font-family: monospace;
        font-size: 11px;
    }

    .blazeai-local-card {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 10px;
        padding: 8px 10px;
    }

    .blazeai-local-rec-badge {
        background-color: rgba(224, 87, 60, 0.20);
        color: #ff9c85;
        border-radius: 6px;
        padding: 2px 6px;
        font-size: 10px;
        font-weight: 600;
    }

    .blazeai-tab-btn {
        background-color: #19191b;
        background-image: none;
        color: #a1a1a6;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 8px;
        padding: 4px 8px;
        min-height: 28px;
        font-size: 11px;
        font-weight: 500;
        box-shadow: none;
    }

    .blazeai-tab-btn.blazeai-tab-active {
        background-color: #232325;
        border-color: rgba(224, 87, 60, 0.80);
        color: #ededed;
    }

    .blazeai-copy-btn:hover {
        background-color: #232325;
        color: #ededed;
    }

    .blazeai-action-chip {
        background-color: #232325;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 6px;
        padding: 4px 10px;
        min-height: 26px;
        font-size: 12px;
        font-weight: 500;
        box-shadow: none;
    }

    .blazeai-action-chip:hover {
        background-color: #2c2c2f;
        border-color: rgba(224, 87, 60, 0.65);
    }

    .blazeai-input-wrap {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 2px 6px 2px 10px;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .blazeai-input-wrap:focus-within {
        background-color: #232325;
        border-color: rgba(224, 87, 60, 0.80);
        outline: none;
    }

    .blazeai-entry,
    .blazeai-entry > text {
        background-color: transparent;
        background-image: none;
        border: none;
        outline: none;
        box-shadow: none;
        color: #ededed;
        font-size: 13px;
        font-weight: 400;
        padding: 6px 0px;
        min-height: 24px;
        caret-color: #ededed;
    }

    .blazeai-entry:focus,
    .blazeai-entry:focus-within,
    .blazeai-entry > text:focus,
    .blazeai-entry > text:focus-visible {
        background-color: transparent;
        border: none;
        outline: none;
        box-shadow: none;
    }

    .blazeai-send-btn {
        background-color: transparent;
        background-image: none;
        color: #a1a1a6;
        border: 1px solid transparent;
        border-radius: 6px;
        min-width: 30px;
        min-height: 30px;
        padding: 0px;
        box-shadow: none;
    }

    .blazeai-send-btn:hover {
        background-color: #2c2c2f;
        color: #ededed;
    }

    .blazeai-mem-row {
        background-color: #141415;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 6px;
        padding: 6px 8px;
    }

    .omnibar-entry {
        background-color: #19191b;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        font-size: 14px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
        padding: 12px 16px;
        margin-bottom: 12px;
        caret-color: #ededed;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .omnibar-entry > text,
    .omnibar-entry > text:focus,
    .omnibar-entry > text:focus-visible {
        outline: none;
        border: none;
        box-shadow: none;
    }

    .omnibar-entry:focus,
    .omnibar-entry:focus-within {
        background-color: #232325;
        border-color: rgba(224, 87, 60, 0.80);
        outline: none;
        box-shadow: none;
    }

    .results-list {
        background-color: transparent;
    }

    .result-row {
        background-color: transparent;
        background-image: none;
        border: 1px solid transparent;
        border-left: 2px solid transparent;
        border-radius: 8px;
        padding: 8px 12px;
        margin-bottom: 4px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .result-row:hover {
        background-color: #19191b;
        background-image: none;
        border-color: rgba(237, 237, 237, 0.08);
        border-left-color: rgba(237, 237, 237, 0.12);
    }

    .result-row:selected {
        background-color: #232325;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-left: 2px solid #e0573c;
    }

    .result-icon-slot {
        min-width: 24px;
        min-height: 24px;
    }

    .result-symbolic-icon {
        color: #a1a1a6;
        -gtk-icon-size: 16px;
    }

    .result-row:selected .result-symbolic-icon {
        color: #ededed;
    }

    .result-title {
        color: #ededed;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    .result-subtitle {
        color: #a1a1a6;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    .result-badge {
        background-color: #19191b;
        color: #85858b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 4px;
        padding: 2px 8px;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    .result-row:selected .result-badge {
        background-color: #141415;
        color: #a1a1a6;
        border-color: rgba(237, 237, 237, 0.12);
    }

    .omnibar-footer {
        border-top: 1px solid rgba(237, 237, 237, 0.08);
        padding-top: 12px;
        margin-top: 8px;
    }

    .footer-hint-text {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
    }
"#;

#[derive(Clone, Debug)]
pub enum OmnibarItemType {
    App(AppInfo),
    Calculation(String, f64),
    Conversion(String, String),
    Clipboard(String, String),
    ProcessKill(u32, String),
    SystemAction(String, String, String),
    WebSearch(String, String),
    AiQuery(String),
}

#[derive(Clone, Debug)]
pub struct OmnibarItem {
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub icon_name: Option<String>,
    pub item_type: OmnibarItemType,
}

pub fn window_dimensions_for_panel_state(panel_open: bool) -> (i32, i32) {
    if panel_open {
        (OMNIBAR_OPEN_WIDTH, OMNIBAR_OPEN_HEIGHT)
    } else {
        (OMNIBAR_COLLAPSED_WIDTH, OMNIBAR_COLLAPSED_HEIGHT)
    }
}

pub fn format_model_chip_label(cfg: &OmnibarConfig) -> String {
    let model = cfg.effective_ai_model();
    let short_model = truncate_chars(&model, 22);
    match cfg.ai_provider {
        BlazeAiProvider::Ollama => format!("Ollama · {}", short_model),
        BlazeAiProvider::OpenAiCompatible => format!("Özel · {}", short_model),
        _ => short_model,
    }
}

pub fn format_model_chip_for_state(cfg: &OmnibarConfig, secrets: &BlazeAiSecrets) -> String {
    if cfg.ai_provider.requires_api_key() && !secrets.has_key_for(cfg.ai_provider) {
        "API anahtarı ekle".to_string()
    } else {
        format_model_chip_label(cfg)
    }
}

pub fn blazeai_placeholder_text(cfg: &OmnibarConfig, secrets: &BlazeAiSecrets) -> &'static str {
    if !cfg.ai_provider.requires_api_key() || secrets.has_key_for(cfg.ai_provider) {
        "BlazeAI'a sor veya hatırla: not yaz"
    } else {
        "BlazeAI'a sor (anahtar yoksa web'de arar)"
    }
}

fn omnibar_pid_file() -> std::path::PathBuf {
    solar_common::get_solar_runtime_dir().join("omnibar.pid")
}

/// Checks if an Omnibar window is already running, and if so, terminates it (toggle behavior).
pub fn close_running_omnibar() -> bool {
    let pid_file = omnibar_pid_file();
    let my_pid = std::process::id();
    if let Ok(content) = std::fs::read_to_string(&pid_file) {
        if let Ok(pid) = content.trim().parse::<u32>() {
            if pid > 0 && pid != my_pid {
                let proc_comm = std::path::PathBuf::from(format!("/proc/{}/comm", pid));
                if let Ok(comm) = std::fs::read_to_string(&proc_comm) {
                    let c = comm.trim();
                    if c.contains("solar") || c.contains("omnibar") {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGTERM);
                        }
                        let _ = std::fs::remove_file(&pid_file);
                        return true;
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&pid_file);
    }
    false
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

pub fn launch_omnibar_window() {
    glib::set_prgname(Some("solar-omnibar"));
    glib::set_application_name("SolarUI Omnibar");

    if let Err(err) = gtk4::init() {
        eprintln!("Failed to initialize GTK4: {}", err);
        return;
    }

    let pid_file = omnibar_pid_file();
    if let Some(parent) = pid_file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&pid_file, std::process::id().to_string());

    let main_loop = glib::MainLoop::new(None, false);
    build_omnibar_ui(main_loop.clone());
    main_loop.run();

    let _ = std::fs::remove_file(&pid_file);
}

fn build_symbolic_icon(candidates: &[&str], css_class: &str) -> Image {
    let chosen = gdk::Display::default()
        .map(|d| IconTheme::for_display(&d))
        .and_then(|theme| {
            candidates
                .iter()
                .copied()
                .find(|name| theme.has_icon(name))
        })
        .unwrap_or_else(|| {
            candidates
                .first()
                .copied()
                .unwrap_or("application-x-executable-symbolic")
        });

    let img = Image::from_icon_name(chosen);
    img.set_pixel_size(16);
    img.add_css_class(css_class);
    img.set_halign(gtk4::Align::Center);
    img.set_valign(gtk4::Align::Center);
    img
}

fn append_footer_keycap_hint(container: &GtkBox, keys: &[&str], desc: &str) {
    let group = GtkBox::new(Orientation::Horizontal, 8);
    group.set_valign(gtk4::Align::Center);

    let keys_box = GtkBox::new(Orientation::Horizontal, 4);
    keys_box.set_valign(gtk4::Align::Center);
    for key in keys {
        let cap = Label::new(Some(key));
        cap.add_css_class("lf-keycap");
        cap.set_valign(gtk4::Align::Center);
        keys_box.append(&cap);
    }
    group.append(&keys_box);

    let lbl = Label::new(Some(desc));
    lbl.add_css_class("footer-hint-text");
    lbl.set_valign(gtk4::Align::Center);
    group.append(&lbl);

    container.append(&group);
}

fn copy_text_to_clipboard(text: &str) {
    if let Some(display) = gdk::Display::default() {
        display.clipboard().set_text(text);
    }
    let text_owned = text.to_string();
    std::thread::spawn(move || {
        if let Ok(mut copy_proc) = Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            use std::io::Write;
            if let Some(mut stdin) = copy_proc.stdin.take() {
                let _ = stdin.write_all(text_owned.as_bytes());
            }
            let _ = copy_proc.wait();
        }
    });
}

fn is_focus_within(window: &Window, container: &impl IsA<gtk4::Widget>) -> bool {
    let target = container.upcast_ref::<gtk4::Widget>();
    gtk4::prelude::GtkWindowExt::focus(window)
        .is_some_and(|fw| fw == *target || fw.is_ancestor(target))
}

fn scroll_chat_to_bottom(scrolled: &ScrolledWindow) {
    let sc_idle = scrolled.downgrade();
    glib::idle_add_local_once(move || {
        if let Some(s) = sc_idle.upgrade() {
            let adj = s.vadjustment();
            adj.set_value((adj.upper() - adj.page_size()).max(0.0));
        }
    });
    for delay_ms in [45_u64, 120] {
        let sc_delayed = scrolled.downgrade();
        glib::timeout_add_local_once(std::time::Duration::from_millis(delay_ms), move || {
            if let Some(s) = sc_delayed.upgrade() {
                let adj = s.vadjustment();
                adj.set_value((adj.upper() - adj.page_size()).max(0.0));
            }
        });
    }
}

fn rebuild_chat_view(
    chat_box: &GtkBox,
    chat_scroll: &ScrolledWindow,
    store: &LocalMemoryStore,
    cfg: &OmnibarConfig,
    secrets: &BlazeAiSecrets,
    fluid_cloud: &FluidCloud,
    voice_speaker: &VoiceSpeaker,
    stop_speak_btn: &Button,
    pending_wait: bool,
) {
    while let Some(child) = chat_box.first_child() {
        chat_box.remove(&child);
    }

    if store.messages.is_empty() && !pending_wait {
        let card = GtkBox::new(Orientation::Vertical, 6);
        card.add_css_class("blazeai-empty-card");

        let title = Label::new(Some(&format!(
            "{} · {}",
            cfg.effective_companion_name(),
            cfg.effective_companion_subtitle()
        )));
        title.add_css_class("blazeai-msg-speaker");
        title.set_halign(gtk4::Align::Start);
        card.append(&title);

        let desc = Label::new(Some(cfg.effective_companion_summary()));
        desc.add_css_class("companion-banner-desc");
        desc.set_wrap(true);
        desc.set_xalign(0.0);
        desc.set_halign(gtk4::Align::Start);
        card.append(&desc);

        let mode_note = if !cfg.ai_provider.requires_api_key() || secrets.has_key_for(cfg.ai_provider) {
            "Yerel beyin hafızası için \"hatırla: not\" yazabilir, ekran veya web araştırması ekleyebilirsin"
        } else {
            "Dişli simgesinden API anahtarı girebilir, Yerel Modeller sekmesinden çevrimdışı model indirebilir veya doğrudan web araması başlatabilirsin"
        };
        let hint = Label::new(Some(mode_note));
        hint.add_css_class("blazeai-msg-note");
        hint.set_wrap(true);
        hint.set_xalign(0.0);
        hint.set_halign(gtk4::Align::Start);
        card.append(&hint);

        chat_box.append(&card);
        return;
    }

    for msg in &store.messages {
        match msg.role {
            ChatRole::SystemNote => {
                let lbl = Label::new(Some(&msg.content));
                lbl.add_css_class("blazeai-msg-note");
                lbl.set_wrap(true);
                lbl.set_xalign(0.0);
                lbl.set_halign(gtk4::Align::Start);
                chat_box.append(&lbl);
            }
            ChatRole::User => {
                let card = GtkBox::new(Orientation::Vertical, 4);
                card.add_css_class("blazeai-msg-user");

                let head = GtkBox::new(Orientation::Horizontal, 6);
                let icon = build_symbolic_icon(
                    &["avatar-default-symbolic", "user-info-symbolic"],
                    "omnibar-header-icon",
                );
                head.append(&icon);
                let who = Label::new(Some("Sen"));
                who.add_css_class("blazeai-msg-speaker");
                who.set_halign(gtk4::Align::Start);
                head.append(&who);
                card.append(&head);

                let body = Label::new(Some(&msg.content));
                body.add_css_class("blazeai-msg-text");
                body.set_wrap(true);
                body.set_xalign(0.0);
                body.set_halign(gtk4::Align::Start);
                body.set_selectable(true);
                card.append(&body);

                chat_box.append(&card);
            }
            ChatRole::Assistant => {
                let card = GtkBox::new(Orientation::Vertical, 6);
                card.add_css_class("blazeai-msg-assistant");

                let head = GtkBox::new(Orientation::Horizontal, 6);
                let speaker_text = if msg.speaker_name.trim().is_empty() {
                    cfg.effective_companion_name()
                } else {
                    msg.speaker_name.clone()
                };
                let speaker = Label::new(Some(&speaker_text));
                speaker.add_css_class("blazeai-msg-speaker");
                speaker.set_hexpand(true);
                speaker.set_halign(gtk4::Align::Start);
                head.append(&speaker);

                // Inline speak aloud button (Piper Turkish TTS)
                let speak_btn = Button::new();
                speak_btn.add_css_class("blazeai-copy-btn");
                speak_btn.set_tooltip_text(Some("Türkçe sesli oku (Piper TTS)"));
                speak_btn.set_child(Some(&build_symbolic_icon(
                    &["audio-speakers-symbolic"],
                    "omnibar-header-icon",
                )));
                let content_to_speak = msg.content.clone();
                let voice_id = cfg.effective_tts_voice();
                let spk_clone = voice_speaker.clone();
                let stop_btn_ref = stop_speak_btn.clone();
                let cloud_spk = fluid_cloud.clone();
                speak_btn.connect_clicked(move |_| {
                    let (tx, rx) = async_channel::bounded::<()>(1);
                    match spk_clone.speak_async(
                        content_to_speak.clone(),
                        voice_id.clone(),
                        move || {
                            let _ = tx.send_blocking(());
                        },
                    ) {
                        Ok(()) => {
                            stop_btn_ref.set_visible(true);
                            let sb = stop_btn_ref.clone();
                            glib::spawn_future_local(async move {
                                let _ = rx.recv().await;
                                sb.set_visible(false);
                            });
                        }
                        Err(err) => {
                            cloud_spk.pulse(&truncate_chars(&err, 42), FluidCloudTone::Warning, false);
                        }
                    }
                });
                head.append(&speak_btn);

                let copy_btn = Button::new();
                copy_btn.add_css_class("blazeai-copy-btn");
                copy_btn.set_tooltip_text(Some("Yanıtı panoya kopyala"));
                let copy_ic = build_symbolic_icon(&["edit-copy-symbolic"], "omnibar-header-icon");
                copy_btn.set_child(Some(&copy_ic));
                let content_to_copy = msg.content.clone();
                let cloud_copy = fluid_cloud.clone();
                copy_btn.connect_clicked(move |_| {
                    copy_text_to_clipboard(&content_to_copy);
                    cloud_copy.pulse("Yanıt panoya kopyalandı", FluidCloudTone::Success, false);
                });
                head.append(&copy_btn);
                card.append(&head);

                if !msg.thinking_text.trim().is_empty() {
                    let exp = Expander::new(Some("Düşünme süreci"));
                    let t_lbl = Label::new(Some(msg.thinking_text.trim()));
                    t_lbl.add_css_class("blazeai-msg-note");
                    t_lbl.set_wrap(true);
                    t_lbl.set_xalign(0.0);
                    t_lbl.set_halign(gtk4::Align::Start);
                    exp.set_child(Some(&t_lbl));
                    card.append(&exp);
                }

                let body = Label::new(Some(&msg.content));
                body.add_css_class("blazeai-msg-text");
                body.set_wrap(true);
                body.set_xalign(0.0);
                body.set_halign(gtk4::Align::Start);
                body.set_selectable(true);
                card.append(&body);

                // Inline Approval Cards for proposed user files (never written silently!)
                for prop in &msg.file_proposals {
                    let p_card = GtkBox::new(Orientation::Vertical, 6);
                    p_card.add_css_class("blazeai-proposal-card");

                    let p_hdr = Label::new(Some(&format!("Dosya önerisi: {}", prop.display_path)));
                    p_hdr.add_css_class("blazeai-msg-speaker");
                    p_hdr.set_halign(gtk4::Align::Start);
                    p_hdr.set_wrap(true);
                    p_card.append(&p_hdr);

                    let preview_lines: String = prop
                        .content
                        .lines()
                        .take(6)
                        .collect::<Vec<_>>()
                        .join("\n");
                    let p_code = Label::new(Some(&truncate_chars(&preview_lines, 240)));
                    p_code.add_css_class("blazeai-proposal-code");
                    p_code.set_wrap(true);
                    p_code.set_xalign(0.0);
                    p_code.set_halign(gtk4::Align::Start);
                    p_card.append(&p_code);

                    let p_btns = GtkBox::new(Orientation::Horizontal, 6);
                    let approve_btn = Button::with_label("Onayla ve Oluştur");
                    approve_btn.add_css_class("companion-reset-btn");
                    let reject_btn = Button::with_label("Reddet");
                    reject_btn.add_css_class("blazeai-chip-toggle");
                    p_btns.append(&approve_btn);
                    p_btns.append(&reject_btn);
                    p_card.append(&p_btns);

                    let prop_clone = prop.clone();
                    let cloud_prop = fluid_cloud.clone();
                    let rej_ref = reject_btn.clone();
                    approve_btn.connect_clicked(move |btn| {
                        let home = std::path::PathBuf::from(
                            std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
                        );
                        match apply_approved_file_proposal(&prop_clone, &home) {
                            Ok(saved_path) => {
                                let _ = BrainStore::open_default().record_task_summary(
                                    &format!("Dosya oluşturuldu: {}", prop_clone.display_path),
                                    &format!("Hedef: {}", saved_path.display()),
                                );
                                btn.set_label("Oluşturuldu");
                                btn.set_sensitive(false);
                                rej_ref.set_visible(false);
                                cloud_prop.pulse(
                                    "Dosya güvenle oluşturuldu",
                                    FluidCloudTone::Success,
                                    false,
                                );
                            }
                            Err(err) => {
                                cloud_prop.pulse(
                                    &truncate_chars(&err, 42),
                                    FluidCloudTone::Danger,
                                    false,
                                );
                            }
                        }
                    });
                    let app_ref = approve_btn.clone();
                    let card_ref = p_card.clone();
                    reject_btn.connect_clicked(move |_| {
                        app_ref.set_sensitive(false);
                        card_ref.set_visible(false);
                    });

                    card.append(&p_card);
                }

                if !msg.suggested_actions.is_empty() {
                    let action_box = GtkBox::new(Orientation::Vertical, 4);
                    for action in &msg.suggested_actions {
                        let abtn = Button::with_label(&action.label);
                        abtn.add_css_class("blazeai-action-chip");
                        abtn.set_halign(gtk4::Align::Start);
                        let cmd = action.command.clone();
                        let cloud_act = fluid_cloud.clone();
                        abtn.connect_clicked(move |_| {
                            if execute_suggested_action(&cmd) {
                                cloud_act.pulse("Uygulama açılıyor", FluidCloudTone::Info, false);
                            }
                        });
                        action_box.append(&abtn);
                    }
                    card.append(&action_box);
                }

                chat_box.append(&card);
            }
        }
    }

    if pending_wait {
        let wait_lbl = Label::new(Some(&format!(
            "{} yanıt hazırlıyor...",
            cfg.effective_companion_name()
        )));
        wait_lbl.add_css_class("blazeai-msg-note");
        wait_lbl.set_halign(gtk4::Align::Start);
        chat_box.append(&wait_lbl);
    }

    scroll_chat_to_bottom(chat_scroll);
}

fn build_omnibar_ui(main_loop: glib::MainLoop) {
    apply_living_flow_theme(OMNIBAR_EXTRA_CSS);

    let _migrated_secrets = migrate_legacy_api_key_if_needed();
    let initial_cfg = SolarConfig::load();
    let panel_open_init = initial_cfg.omnibar.companion_panel_open;
    let (init_w, init_h) = window_dimensions_for_panel_state(panel_open_init);

    let store_rc = Rc::new(RefCell::new(LocalMemoryStore::load(
        initial_cfg.omnibar.ai_persist_chat,
    )));
    let is_sending = Rc::new(Cell::new(false));
    let voice_speaker = VoiceSpeaker::new();
    let active_recording: Rc<RefCell<Option<ActiveRecording>>> = Rc::new(RefCell::new(None));
    let pending_vision: Rc<RefCell<Option<VisionAttachment>>> = Rc::new(RefCell::new(None));
    let local_pull_cancel = Arc::new(AtomicBool::new(false));
    let voice_dl_cancel = Arc::new(AtomicBool::new(false));

    let window = Window::builder()
        .title("Solar Omnibar")
        .decorated(false)
        .default_width(init_w)
        .default_height(init_h)
        .resizable(false)
        .build();

    let loop_quit = main_loop.clone();
    let spk_on_close = voice_speaker.clone();
    let rec_on_close = active_recording.clone();
    window.connect_close_request(move |_| {
        spk_on_close.stop();
        if let Some(rec) = rec_on_close.borrow_mut().take() {
            rec.cancel();
        }
        let _ = std::fs::remove_file(omnibar_pid_file());
        loop_quit.quit();
        glib::Propagation::Proceed
    });

    let root_box = GtkBox::new(Orientation::Horizontal, 0);
    root_box.add_css_class("omnibar-card");
    root_box.set_size_request(init_w, init_h);

    // =========================================================================
    // LEFT PANE: Interactive Vector Companion + BlazeAI Local-Memory Assistant
    // =========================================================================
    let left_pane = GtkBox::new(Orientation::Vertical, 10);
    left_pane.add_css_class("omnibar-left-pane");
    left_pane.set_size_request(356, -1);
    left_pane.set_hexpand(false);
    left_pane.set_vexpand(true);
    left_pane.set_visible(panel_open_init);

    let left_top = GtkBox::new(Orientation::Horizontal, 10);
    left_top.set_valign(gtk4::Align::Center);

    let avatar = CompanionAvatarWidget::new_with_glasses(
        initial_cfg.omnibar.companion_shape,
        initial_cfg.omnibar.companion_expression,
        &initial_cfg.omnibar.companion_color,
        initial_cfg.omnibar.wears_round_glasses(),
        52,
    );
    avatar.bind_window(&window);
    left_top.append(avatar.widget());

    let meta_col = GtkBox::new(Orientation::Vertical, 4);
    meta_col.set_hexpand(true);
    meta_col.set_valign(gtk4::Align::Center);

    let name_ctrl_row = GtkBox::new(Orientation::Horizontal, 4);
    name_ctrl_row.set_valign(gtk4::Align::Center);

    let companion_name_lbl = Label::new(Some(&initial_cfg.omnibar.effective_companion_name()));
    companion_name_lbl.add_css_class("companion-name-title");
    companion_name_lbl.set_hexpand(true);
    companion_name_lbl.set_halign(gtk4::Align::Start);
    companion_name_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    name_ctrl_row.append(&companion_name_lbl);

    let edit_btn = Button::new();
    edit_btn.add_css_class("lf-win-btn");
    edit_btn.set_tooltip_text(Some("Karakteri özelleştir"));
    edit_btn.set_child(Some(&build_symbolic_icon(
        &["document-edit-symbolic"],
        "omnibar-header-icon",
    )));
    name_ctrl_row.append(&edit_btn);

    let new_chat_btn = Button::new();
    new_chat_btn.add_css_class("lf-win-btn");
    new_chat_btn.set_tooltip_text(Some("Yeni sohbet"));
    new_chat_btn.set_child(Some(&build_symbolic_icon(
        &["edit-clear-all-symbolic", "view-refresh-symbolic"],
        "omnibar-header-icon",
    )));
    name_ctrl_row.append(&new_chat_btn);

    let gear_btn = Button::new();
    gear_btn.add_css_class("lf-win-btn");
    gear_btn.set_tooltip_text(Some("BlazeAI ve yerel hafıza ayarları"));
    gear_btn.set_child(Some(&build_symbolic_icon(
        &["emblem-system-symbolic", "preferences-system-symbolic"],
        "omnibar-header-icon",
    )));
    name_ctrl_row.append(&gear_btn);

    meta_col.append(&name_ctrl_row);

    let sub_model_row = GtkBox::new(Orientation::Horizontal, 6);
    sub_model_row.set_valign(gtk4::Align::Center);

    let companion_arch_lbl = Label::new(Some(initial_cfg.omnibar.effective_companion_subtitle()));
    companion_arch_lbl.add_css_class("companion-archetype-sub");
    companion_arch_lbl.set_hexpand(true);
    companion_arch_lbl.set_halign(gtk4::Align::Start);
    companion_arch_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    sub_model_row.append(&companion_arch_lbl);

    let model_btn = Button::new();
    model_btn.add_css_class("model-pill-btn");
    model_btn.set_tooltip_text(Some("Desteklenen modelleri seç"));
    let model_btn_box = GtkBox::new(Orientation::Horizontal, 4);
    let initial_secrets = BlazeAiSecrets::load();
    let model_btn_lbl = Label::new(Some(&format_model_chip_for_state(
        &initial_cfg.omnibar,
        &initial_secrets,
    )));
    model_btn_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    model_btn_lbl.set_max_width_chars(18);
    model_btn_box.append(&model_btn_lbl);
    model_btn_box.append(&build_symbolic_icon(
        &["pan-down-symbolic"],
        "omnibar-header-icon",
    ));
    model_btn.set_child(Some(&model_btn_box));
    sub_model_row.append(&model_btn);

    meta_col.append(&sub_model_row);
    left_top.append(&meta_col);
    left_pane.append(&left_top);

    // Chat stream
    let chat_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .build();
    chat_scroll.add_css_class("blazeai-chat-scroll");

    let chat_box = GtkBox::new(Orientation::Vertical, 8);
    chat_scroll.set_child(Some(&chat_box));
    left_pane.append(&chat_scroll);

    // Explicit Vision attachment preview badge (hidden until user clicks Screen button)
    let vision_badge_row = GtkBox::new(Orientation::Horizontal, 6);
    vision_badge_row.add_css_class("blazeai-vision-badge");
    vision_badge_row.set_visible(false);
    let vision_badge_lbl = Label::new(Some("Ekran görüntüsü eklendi"));
    vision_badge_lbl.add_css_class("companion-banner-desc");
    vision_badge_lbl.set_hexpand(true);
    vision_badge_lbl.set_halign(gtk4::Align::Start);
    vision_badge_row.append(&vision_badge_lbl);
    let vision_clear_btn = Button::new();
    vision_clear_btn.add_css_class("blazeai-copy-btn");
    vision_clear_btn.set_tooltip_text(Some("Ekran görüntüsünü kaldır"));
    vision_clear_btn.set_child(Some(&build_symbolic_icon(
        &["window-close-symbolic"],
        "omnibar-header-icon",
    )));
    vision_badge_row.append(&vision_clear_btn);
    left_pane.append(&vision_badge_row);

    // Compact multimodal toolbar: Thinking toggle, Web research, Explicit Screen Vision, Mic, Stop TTS
    let blazeai_tool_bar = GtkBox::new(Orientation::Horizontal, 4);
    blazeai_tool_bar.add_css_class("blazeai-tool-bar");

    let think_toggle = ToggleButton::with_label("Düşün");
    think_toggle.add_css_class("blazeai-chip-toggle");
    think_toggle.set_active(initial_cfg.omnibar.ai_thinking_mode);
    think_toggle.set_tooltip_text(Some("Hibrit derin düşünme / hızlı yanıt modu"));
    blazeai_tool_bar.append(&think_toggle);

    let web_toggle = ToggleButton::with_label("Web");
    web_toggle.add_css_class("blazeai-chip-toggle");
    web_toggle.set_tooltip_text(Some("Sonraki soruda ücretsiz canlı DuckDuckGo araştırması yap"));
    blazeai_tool_bar.append(&web_toggle);

    let vision_btn = Button::new();
    vision_btn.add_css_class("blazeai-chip-toggle");
    vision_btn.set_tooltip_text(Some("Ekran görüntüsü ekle (açık onaylı grim)"));
    vision_btn.set_child(Some(&build_symbolic_icon(
        &["camera-photo-symbolic"],
        "omnibar-header-icon",
    )));
    blazeai_tool_bar.append(&vision_btn);

    let mic_btn = Button::new();
    mic_btn.add_css_class("blazeai-chip-toggle");
    mic_btn.set_tooltip_text(Some("Türkçe sesli sor (Bas-başlat / tekrar bas-gönder)"));
    mic_btn.set_child(Some(&build_symbolic_icon(
        &["audio-input-microphone-symbolic"],
        "omnibar-header-icon",
    )));
    blazeai_tool_bar.append(&mic_btn);

    let stop_speak_btn = Button::new();
    stop_speak_btn.add_css_class("blazeai-chip-toggle");
    stop_speak_btn.set_tooltip_text(Some("Konuşmayı durdur (Barge-in)"));
    stop_speak_btn.set_child(Some(&build_symbolic_icon(
        &["audio-volume-muted-symbolic"],
        "omnibar-header-icon",
    )));
    stop_speak_btn.set_visible(false);
    blazeai_tool_bar.append(&stop_speak_btn);

    let tool_spacer = GtkBox::new(Orientation::Horizontal, 0);
    tool_spacer.set_hexpand(true);
    blazeai_tool_bar.append(&tool_spacer);

    let local_hub_shortcut_btn = Button::new();
    local_hub_shortcut_btn.add_css_class("blazeai-chip-toggle");
    local_hub_shortcut_btn.set_tooltip_text(Some("Yerel model indir veya yönet (1.5B–16B)"));
    local_hub_shortcut_btn.set_child(Some(&build_symbolic_icon(
        &["folder-download-symbolic"],
        "omnibar-header-icon",
    )));
    blazeai_tool_bar.append(&local_hub_shortcut_btn);

    left_pane.append(&blazeai_tool_bar);

    // Bottom prompt bar
    let blazeai_input_wrap = GtkBox::new(Orientation::Horizontal, 4);
    blazeai_input_wrap.add_css_class("blazeai-input-wrap");

    let blazeai_entry = Entry::new();
    blazeai_entry.add_css_class("blazeai-entry");
    blazeai_entry.set_hexpand(true);
    blazeai_entry.set_placeholder_text(Some(blazeai_placeholder_text(
        &initial_cfg.omnibar,
        &BlazeAiSecrets::load(),
    )));
    blazeai_input_wrap.append(&blazeai_entry);

    let send_btn = Button::new();
    send_btn.add_css_class("blazeai-send-btn");
    send_btn.set_tooltip_text(Some("Gönder"));
    send_btn.set_child(Some(&build_symbolic_icon(
        &["document-send-symbolic", "mail-send-symbolic", "go-next-symbolic"],
        "omnibar-header-icon",
    )));
    blazeai_input_wrap.append(&send_btn);
    left_pane.append(&blazeai_input_wrap);

    root_box.append(&left_pane);

    // =========================================================================
    // RIGHT PANE: Fast Launcher, Math, Unit/Currency, Clipboard, Processes
    // =========================================================================
    let right_pane = GtkBox::new(Orientation::Vertical, 0);
    right_pane.add_css_class("omnibar-right-pane");
    right_pane.set_hexpand(true);
    right_pane.set_vexpand(true);

    let header_row = GtkBox::new(Orientation::Horizontal, 10);
    header_row.add_css_class("omnibar-header");
    let header_icon = build_symbolic_icon(
        &["system-search-symbolic", "edit-find-symbolic"],
        "omnibar-header-icon",
    );
    header_row.append(&header_icon);

    let brand_col = GtkBox::new(Orientation::Vertical, 2);
    brand_col.set_valign(gtk4::Align::Center);
    let b_title = Label::new(Some("Solar Omnibar"));
    b_title.add_css_class("omnibar-brand-title");
    b_title.set_halign(gtk4::Align::Start);
    brand_col.append(&b_title);

    let b_sub = Label::new(Some("Uygulama, hesaplama, kur, pano ve sistem"));
    b_sub.add_css_class("omnibar-brand-sub");
    b_sub.set_halign(gtk4::Align::Start);
    brand_col.append(&b_sub);
    header_row.append(&brand_col);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    header_row.append(&spacer);

    let fluid_cloud = FluidCloud::new("Hazır");
    header_row.append(fluid_cloud.widget());

    let toggle_panel_btn = Button::new();
    toggle_panel_btn.add_css_class("lf-win-btn");
    toggle_panel_btn.set_tooltip_text(Some("BlazeAI panelini göster veya gizle"));
    toggle_panel_btn.set_child(Some(&build_symbolic_icon(
        &["sidebar-show-symbolic", "view-left-pane-symbolic", "pan-start-symbolic"],
        "omnibar-header-icon",
    )));
    header_row.append(&toggle_panel_btn);

    let close_btn = Button::new();
    let close_icon = build_symbolic_icon(&["window-close-symbolic"], "omnibar-header-icon");
    close_btn.set_child(Some(&close_icon));
    close_btn.add_css_class("lf-win-btn");
    close_btn.add_css_class("lf-win-close");
    close_btn.set_tooltip_text(Some("Kapat"));
    let win_close = window.downgrade();
    let loop_close = main_loop.clone();
    let spk_close_btn = voice_speaker.clone();
    close_btn.connect_clicked(move |_| {
        spk_close_btn.stop();
        let _ = std::fs::remove_file(omnibar_pid_file());
        if let Some(w) = win_close.upgrade() {
            w.close();
        }
        loop_close.quit();
    });
    header_row.append(&close_btn);
    right_pane.append(&header_row);

    // Search input (initial focus always starts here!)
    let entry = Entry::new();
    entry.add_css_class("omnibar-entry");
    entry.set_placeholder_text(Some("Uygulama, komut, =hesaplama, kur veya pano (cb) ara"));
    right_pane.append(&entry);

    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let list_box = ListBox::new();
    list_box.add_css_class("results-list");
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    scrolled.set_child(Some(&list_box));
    right_pane.append(&scrolled);

    // Footer keycap hints
    let footer_box = GtkBox::new(Orientation::Horizontal, 16);
    footer_box.add_css_class("omnibar-footer");
    footer_box.set_valign(gtk4::Align::Center);
    append_footer_keycap_hint(&footer_box, &["Tab"], "Panel değiştir");
    append_footer_keycap_hint(&footer_box, &["↑", "↓"], "Seç");
    append_footer_keycap_hint(&footer_box, &["Enter"], "Çalıştır veya gönder");
    append_footer_keycap_hint(&footer_box, &["Esc"], "Kapat");
    right_pane.append(&footer_box);

    root_box.append(&right_pane);
    window.set_child(Some(&root_box));

    // Initial chat render
    rebuild_chat_view(
        &chat_box,
        &chat_scroll,
        &store_rc.borrow(),
        &initial_cfg.omnibar,
        &BlazeAiSecrets::load(),
        &fluid_cloud,
        &voice_speaker,
        &stop_speak_btn,
        false,
    );

    // Helper closure to refresh chat + placeholder + model button label
    let refresh_ui_state = {
        let chat_b = chat_box.clone();
        let chat_s = chat_scroll.clone();
        let st_rc = store_rc.clone();
        let cloud_c = fluid_cloud.clone();
        let model_lbl = model_btn_lbl.clone();
        let b_entry = blazeai_entry.clone();
        let send_flag = is_sending.clone();
        let spk_ref = voice_speaker.clone();
        let stop_btn_ref = stop_speak_btn.clone();
        Rc::new(move || {
            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();
            model_lbl.set_text(&format_model_chip_for_state(&cfg.omnibar, &secrets));
            b_entry.set_placeholder_text(Some(blazeai_placeholder_text(&cfg.omnibar, &secrets)));
            rebuild_chat_view(
                &chat_b,
                &chat_s,
                &st_rc.borrow(),
                &cfg.omnibar,
                &secrets,
                &cloud_c,
                &spk_ref,
                &stop_btn_ref,
                send_flag.get(),
            );
        })
    };

    // =========================================================================
    // POPOVERS: Companion Customizer, Model Selector, Gear Settings & Memory
    // =========================================================================
    let customizer_popover = {
        let name_lbl_c = companion_name_lbl.clone();
        let arch_lbl_c = companion_arch_lbl.clone();
        let refresh_c = refresh_ui_state.clone();
        let st_trans = store_rc.clone();
        build_companion_customizer_popover(
            &edit_btn,
            &avatar,
            CompanionCustomizerCallbacks {
                on_appearance_changed: Rc::new(move |omnibar_cfg: &OmnibarConfig| {
                    name_lbl_c.set_text(&omnibar_cfg.effective_companion_name());
                    arch_lbl_c.set_text(omnibar_cfg.effective_companion_subtitle());
                    refresh_c();
                }),
                on_persona_transition: Rc::new(move |from_n, to_n, expr| {
                    let cfg = SolarConfig::load();
                    let _ = st_trans.borrow_mut().record_character_transition(
                        from_n,
                        to_n,
                        expr,
                        cfg.omnibar.ai_persist_chat,
                    );
                }),
            },
        )
    };
    {
        let pop = customizer_popover.clone();
        edit_btn.connect_clicked(move |_| {
            pop.popup();
        });
    }

    // Gear Popover (Provider, Model, Base URL, 0600 PasswordEntry, Switches & Memory)
    let gear_popover = Popover::new();
    gear_popover.set_parent(&gear_btn);
    gear_popover.set_has_arrow(true);
    gear_popover.add_css_class("companion-popover");
    {
        let pop_clean = gear_popover.downgrade();
        gear_btn.connect_destroy(move |_| {
            if let Some(p) = pop_clean.upgrade() {
                p.unparent();
            }
        });
    }

    let gear_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(460)
        .min_content_width(356)
        .build();
    let gear_outer = GtkBox::new(Orientation::Vertical, 8);
    gear_outer.add_css_class("blazeai-gear-box");

    let gear_title = Label::new(Some("BlazeAI, Yerel Modeller ve Ses"));
    gear_title.add_css_class("companion-banner-title");
    gear_title.set_halign(gtk4::Align::Start);
    gear_outer.append(&gear_title);

    // 3-tab navigation row inside Gear Popover
    let gear_tabs_row = GtkBox::new(Orientation::Horizontal, 4);
    let tab_api_btn = Button::with_label("API & Genel");
    tab_api_btn.add_css_class("blazeai-tab-btn");
    tab_api_btn.add_css_class("blazeai-tab-active");
    tab_api_btn.set_hexpand(true);
    let tab_local_btn = Button::with_label("Yerel Modeller");
    tab_local_btn.add_css_class("blazeai-tab-btn");
    tab_local_btn.set_hexpand(true);
    let tab_voice_btn = Button::with_label("Ses & Beyin");
    tab_voice_btn.add_css_class("blazeai-tab-btn");
    tab_voice_btn.set_hexpand(true);
    gear_tabs_row.append(&tab_api_btn);
    gear_tabs_row.append(&tab_local_btn);
    gear_tabs_row.append(&tab_voice_btn);
    gear_outer.append(&gear_tabs_row);

    let gear_stack = Stack::new();
    let gear_box = GtkBox::new(Orientation::Vertical, 8);
    let local_hub_box = GtkBox::new(Orientation::Vertical, 8);
    let voice_brain_box = GtkBox::new(Orientation::Vertical, 8);
    gear_stack.add_named(&gear_box, Some("api"));
    gear_stack.add_named(&local_hub_box, Some("local"));
    gear_stack.add_named(&voice_brain_box, Some("voice"));
    gear_stack.set_visible_child_name("api");
    gear_outer.append(&gear_stack);

    let switch_gear_tab: Rc<dyn Fn(&'static str)> = {
        let st = gear_stack.clone();
        let b1 = tab_api_btn.clone();
        let b2 = tab_local_btn.clone();
        let b3 = tab_voice_btn.clone();
        Rc::new(move |tab: &'static str| {
            st.set_visible_child_name(tab);
            for (btn, id) in [(&b1, "api"), (&b2, "local"), (&b3, "voice")] {
                if id == tab {
                    btn.add_css_class("blazeai-tab-active");
                } else {
                    btn.remove_css_class("blazeai-tab-active");
                }
            }
        })
    };
    {
        let sw = switch_gear_tab.clone();
        tab_api_btn.connect_clicked(move |_| sw("api"));
    }
    {
        let sw = switch_gear_tab.clone();
        tab_local_btn.connect_clicked(move |_| sw("local"));
    }
    {
        let sw = switch_gear_tab.clone();
        tab_voice_btn.connect_clicked(move |_| sw("voice"));
    }

    let syncing_gear = Rc::new(Cell::new(false));

    let provider_labels: Vec<&str> = BlazeAiProvider::ALL
        .iter()
        .map(|p| p.display_name())
        .collect();
    let provider_dropdown =
        DropDown::new(Some(StringList::new(&provider_labels)), None::<gtk4::Expression>);
    gear_box.append(&provider_dropdown);

    let gear_model_lbl = Label::new(Some("Model adı"));
    gear_model_lbl.add_css_class("companion-section-lbl");
    gear_model_lbl.set_halign(gtk4::Align::Start);
    gear_box.append(&gear_model_lbl);

    let gear_model_entry = Entry::new();
    gear_model_entry.set_placeholder_text(Some("gemini-3.8-flash"));
    gear_box.append(&gear_model_entry);

    let gear_url_lbl = Label::new(Some("Özel API veya Ollama adresi"));
    gear_url_lbl.add_css_class("companion-section-lbl");
    gear_url_lbl.set_halign(gtk4::Align::Start);
    gear_box.append(&gear_url_lbl);

    let gear_url_entry = Entry::new();
    gear_url_entry.set_placeholder_text(Some("http://127.0.0.1:11434"));
    gear_box.append(&gear_url_entry);

    let gear_key_lbl = Label::new(Some("API anahtarı (0600 yerel kasa)"));
    gear_key_lbl.add_css_class("companion-section-lbl");
    gear_key_lbl.set_halign(gtk4::Align::Start);
    gear_box.append(&gear_key_lbl);

    let gear_key_entry = PasswordEntry::builder()
        .placeholder_text("Anahtar girin (Ollama için gerekmez)")
        .show_peek_icon(true)
        .build();
    gear_box.append(&gear_key_entry);

    let gear_probe_status_lbl = Label::new(Some(
        "API anahtarı girildiğinde destekli modeller otomatik listelenir",
    ));
    gear_probe_status_lbl.add_css_class("blazeai-msg-note");
    gear_probe_status_lbl.set_wrap(true);
    gear_probe_status_lbl.set_xalign(0.0);
    gear_probe_status_lbl.set_halign(gtk4::Align::Start);
    gear_box.append(&gear_probe_status_lbl);

    let discovered_models: Rc<RefCell<HashMap<BlazeAiProvider, Result<Vec<String>, String>>>> =
        Rc::new(RefCell::new(HashMap::new()));
    let probe_generation = Rc::new(Cell::new(0u64));

    let chk_persist = CheckButton::with_label("Sohbet geçmişini koru");
    let chk_local_mem = CheckButton::with_label("Uzun vadeli yerel hafıza");
    let chk_guide = CheckButton::with_label("Yerleşik BlazeOS rehberini ekle");
    let chk_web_research = CheckButton::with_label("Otomatik web araştırması (DuckDuckGo)");
    let chk_agent_tools = CheckButton::with_label("Onay kartlı dosya oluşturma araçları");
    let chk_slang = CheckButton::with_label("Argo serbest (samimi halk dili)");
    chk_slang.set_tooltip_text(Some(
        "Açıldığında tüm karakterler yakın arkadaş muhabbeti tadında doğal Türkçe argo kullanabilir (varsayılan: kapalı)",
    ));
    gear_box.append(&chk_persist);
    gear_box.append(&chk_local_mem);
    gear_box.append(&chk_guide);
    gear_box.append(&chk_web_research);
    gear_box.append(&chk_agent_tools);
    gear_box.append(&chk_slang);

    gear_box.append(&Separator::new(Orientation::Horizontal));

    let mem_hdr = Label::new(Some("Yerel hafıza notları"));
    mem_hdr.add_css_class("companion-section-lbl");
    mem_hdr.set_halign(gtk4::Align::Start);
    gear_box.append(&mem_hdr);

    let mem_list_box = GtkBox::new(Orientation::Vertical, 4);
    gear_box.append(&mem_list_box);

    let mem_add_row = GtkBox::new(Orientation::Horizontal, 6);
    let mem_add_entry = Entry::new();
    mem_add_entry.set_hexpand(true);
    mem_add_entry.set_placeholder_text(Some("Yeni hafıza notu ekle"));
    mem_add_row.append(&mem_add_entry);
    let mem_add_btn = Button::with_label("Ekle");
    mem_add_btn.add_css_class("companion-reset-btn");
    mem_add_row.append(&mem_add_btn);
    gear_box.append(&mem_add_row);

    let clear_all_btn = Button::with_label("Sohbeti ve yerel hafızayı temizle");
    clear_all_btn.add_css_class("danger-btn");
    gear_box.append(&clear_all_btn);

    // =========================================================================
    // TAB 2: LOCAL MODEL HUB (1.5B-16B, Hardware Probe, User-Space Ollama)
    // =========================================================================
    let hw_summary_lbl = Label::new(Some("Donanım algılanıyor..."));
    hw_summary_lbl.add_css_class("companion-banner-desc");
    hw_summary_lbl.set_wrap(true);
    hw_summary_lbl.set_xalign(0.0);
    hw_summary_lbl.set_halign(gtk4::Align::Start);
    local_hub_box.append(&hw_summary_lbl);

    let ollama_engine_row = GtkBox::new(Orientation::Horizontal, 6);
    let ollama_status_lbl = Label::new(Some("Yerel motor durumu kontrol ediliyor..."));
    ollama_status_lbl.add_css_class("blazeai-msg-note");
    ollama_status_lbl.set_hexpand(true);
    ollama_status_lbl.set_wrap(true);
    ollama_status_lbl.set_xalign(0.0);
    ollama_status_lbl.set_halign(gtk4::Align::Start);
    ollama_engine_row.append(&ollama_status_lbl);

    let ollama_setup_btn = Button::with_label("Motoru kur");
    ollama_setup_btn.add_css_class("companion-reset-btn");
    ollama_engine_row.append(&ollama_setup_btn);
    local_hub_box.append(&ollama_engine_row);

    let pull_progress_box = GtkBox::new(Orientation::Vertical, 4);
    pull_progress_box.set_visible(false);
    let pull_status_lbl = Label::new(Some("İndiriliyor..."));
    pull_status_lbl.add_css_class("blazeai-msg-note");
    pull_status_lbl.set_halign(gtk4::Align::Start);
    pull_progress_box.append(&pull_status_lbl);
    let pull_bar_row = GtkBox::new(Orientation::Horizontal, 6);
    let pull_progress_bar = ProgressBar::new();
    pull_progress_bar.set_hexpand(true);
    pull_progress_bar.set_show_text(true);
    pull_bar_row.append(&pull_progress_bar);
    let pull_cancel_btn = Button::with_label("İptal");
    pull_cancel_btn.add_css_class("danger-btn");
    pull_bar_row.append(&pull_cancel_btn);
    pull_progress_box.append(&pull_bar_row);
    local_hub_box.append(&pull_progress_box);

    let local_tiers_box = GtkBox::new(Orientation::Vertical, 6);
    local_hub_box.append(&local_tiers_box);

    // =========================================================================
    // TAB 3: VOICE (whisper.cpp + Piper Turkish TTS) & LOCAL BRAIN FOLDER
    // =========================================================================
    let voice_hdr = Label::new(Some("Türkçe sesli sohbet (Piper TTS + Whisper)"));
    voice_hdr.add_css_class("companion-section-lbl");
    voice_hdr.set_halign(gtk4::Align::Start);
    voice_brain_box.append(&voice_hdr);

    let chk_tts_auto = CheckButton::with_label("Yanıtları otomatik Türkçe seslendir");
    voice_brain_box.append(&chk_tts_auto);

    let voice_sel_lbl = Label::new(Some("Karakter sesi"));
    voice_sel_lbl.add_css_class("companion-section-lbl");
    voice_sel_lbl.set_halign(gtk4::Align::Start);
    voice_brain_box.append(&voice_sel_lbl);

    let voice_dropdown = DropDown::new(
        Some(StringList::new(&[
            "Otomatik (Karaktere göre)",
            "Persephone / Solar / Kıvılcım / Sis (dfki)",
            "Köz / Stolas (fahrettin)",
        ])),
        None::<gtk4::Expression>,
    );
    voice_brain_box.append(&voice_dropdown);

    let voice_status_lbl = Label::new(Some("Ses paketleri kontrol ediliyor..."));
    voice_status_lbl.add_css_class("blazeai-msg-note");
    voice_status_lbl.set_wrap(true);
    voice_status_lbl.set_xalign(0.0);
    voice_status_lbl.set_halign(gtk4::Align::Start);
    voice_brain_box.append(&voice_status_lbl);

    let dl_tts_btn = Button::with_label("Türkçe ses paketini indir (Piper ~150 MB)");
    dl_tts_btn.add_css_class("companion-reset-btn");
    voice_brain_box.append(&dl_tts_btn);

    let dl_stt_btn = Button::with_label("Türkçe dinleme modelini indir (Whisper ~142 MB)");
    dl_stt_btn.add_css_class("companion-reset-btn");
    voice_brain_box.append(&dl_stt_btn);

    voice_brain_box.append(&Separator::new(Orientation::Horizontal));

    let brain_hdr = Label::new(Some("Yerel Beyin Klasörü (Antigravity mantığı)"));
    brain_hdr.add_css_class("companion-section-lbl");
    brain_hdr.set_halign(gtk4::Align::Start);
    voice_brain_box.append(&brain_hdr);

    let brain_desc = Label::new(Some(
        "memory.md, tasks/ ve knowledge/ dosyaları yerel bilgisayarında 0600 izniyle tutulur; doğrudan açıp düzenleyebilirsin.",
    ));
    brain_desc.add_css_class("blazeai-msg-note");
    brain_desc.set_wrap(true);
    brain_desc.set_xalign(0.0);
    brain_desc.set_halign(gtk4::Align::Start);
    voice_brain_box.append(&brain_desc);

    let open_brain_btn = Button::new();
    open_brain_btn.add_css_class("companion-reset-btn");
    let open_brain_row = GtkBox::new(Orientation::Horizontal, 6);
    open_brain_row.set_halign(gtk4::Align::Center);
    open_brain_row.append(&build_symbolic_icon(&["folder-open-symbolic"], "omnibar-header-icon"));
    open_brain_row.append(&Label::new(Some("Beyin klasörünü aç")));
    open_brain_btn.set_child(Some(&open_brain_row));
    voice_brain_box.append(&open_brain_btn);

    gear_scroll.set_child(Some(&gear_outer));
    gear_popover.set_child(Some(&gear_scroll));

    let rebuild_gear_memory_list: Rc<dyn Fn()> = {
        let list_c = mem_list_box.clone();
        let st_c = store_rc.clone();
        let cloud_c = fluid_cloud.clone();
        Rc::new(move || {
            while let Some(ch) = list_c.first_child() {
                list_c.remove(&ch);
            }
            let facts = st_c.borrow().memory_facts.clone();
            if facts.is_empty() {
                let empty_lbl = Label::new(Some("Kayıtlı yerel hafıza notu yok"));
                empty_lbl.add_css_class("blazeai-msg-note");
                empty_lbl.set_halign(gtk4::Align::Start);
                list_c.append(&empty_lbl);
                return;
            }
            for fact in facts {
                let row = GtkBox::new(Orientation::Horizontal, 6);
                row.add_css_class("blazeai-mem-row");
                let lbl = Label::new(Some(&fact.text));
                lbl.add_css_class("companion-banner-desc");
                lbl.set_wrap(true);
                lbl.set_xalign(0.0);
                lbl.set_hexpand(true);
                lbl.set_halign(gtk4::Align::Start);
                row.append(&lbl);

                let del_btn = Button::new();
                del_btn.add_css_class("blazeai-copy-btn");
                del_btn.set_tooltip_text(Some("Notu sil"));
                del_btn.set_child(Some(&build_symbolic_icon(
                    &["edit-delete-symbolic", "window-close-symbolic"],
                    "omnibar-header-icon",
                )));
                let fact_id = fact.id.clone();
                let st_del = st_c.clone();
                let list_del = list_c.clone();
                let cloud_del = cloud_c.clone();
                let row_del = row.clone();
                del_btn.connect_clicked(move |_| {
                    let _ = st_del.borrow_mut().remove_memory_fact(&fact_id);
                    list_del.remove(&row_del);
                    if st_del.borrow().memory_facts.is_empty() {
                        let empty_lbl = Label::new(Some("Kayıtlı yerel hafıza notu yok"));
                        empty_lbl.add_css_class("blazeai-msg-note");
                        empty_lbl.set_halign(gtk4::Align::Start);
                        list_del.append(&empty_lbl);
                    }
                    cloud_del.pulse("Hafıza notu silindi", FluidCloudTone::Info, false);
                });
                row.append(&del_btn);
                list_c.append(&row);
            }
        })
    };

    let sync_gear_fields = {
        let sync_flag = syncing_gear.clone();
        let dd = provider_dropdown.clone();
        let m_entry = gear_model_entry.clone();
        let u_entry = gear_url_entry.clone();
        let k_entry = gear_key_entry.clone();
        let c_persist = chk_persist.clone();
        let c_mem = chk_local_mem.clone();
        let c_guide = chk_guide.clone();
        let c_web = chk_web_research.clone();
        let c_tools = chk_agent_tools.clone();
        let c_slang = chk_slang.clone();
        let c_tts = chk_tts_auto.clone();
        let v_dd = voice_dropdown.clone();
        let v_lbl = voice_status_lbl.clone();
        let rebuild_mem = rebuild_gear_memory_list.clone();
        Rc::new(move || {
            sync_flag.set(true);
            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();
            let p_idx = BlazeAiProvider::ALL
                .iter()
                .position(|p| *p == cfg.omnibar.ai_provider)
                .unwrap_or(0);
            dd.set_selected(p_idx as u32);
            m_entry.set_text(&cfg.omnibar.effective_ai_model());
            u_entry.set_text(&cfg.omnibar.effective_ai_base_url());
            u_entry.set_sensitive(matches!(
                cfg.omnibar.ai_provider,
                BlazeAiProvider::OpenAiCompatible | BlazeAiProvider::Ollama
            ));
            k_entry.set_text(secrets.key_for(cfg.omnibar.ai_provider));
            k_entry.set_sensitive(cfg.omnibar.ai_provider.requires_api_key());
            c_persist.set_active(cfg.omnibar.ai_persist_chat);
            c_mem.set_active(cfg.omnibar.ai_local_memory_enabled);
            c_guide.set_active(cfg.omnibar.ai_include_blazeos_guide);
            c_web.set_active(cfg.omnibar.ai_web_research_enabled);
            c_tools.set_active(cfg.omnibar.ai_agent_tools_enabled);
            c_slang.set_active(cfg.omnibar.ai_slang_enabled);
            c_tts.set_active(cfg.omnibar.ai_voice_tts_auto);
            let v_idx = match cfg.omnibar.ai_voice_tts_voice.trim() {
                PIPER_VOICE_PERSEPHONE => 1,
                PIPER_VOICE_KOZ => 2,
                _ => 0,
            };
            v_dd.set_selected(v_idx);
            let vp = VoicePackStatus::probe(&cfg.omnibar.ai_voice_stt_model);
            v_lbl.set_text(&format!(
                "Konuşma (Piper): {} · Dinleme (Whisper): {}",
                if vp.persephone_voice_ready && vp.piper_ready {
                    "Hazır"
                } else {
                    "İndirilmedi"
                },
                if vp.stt_ready() {
                    "Hazır"
                } else if vp.whisper_model_ready {
                    "Model hazır (whisper-cli bekleniyor)"
                } else {
                    "İndirilmedi"
                }
            ));
            rebuild_mem();
            sync_flag.set(false);
        })
    };
    sync_gear_fields();

    // Model Selector Popover (strictly lists live supported models for configured keys / running Ollama)
    let model_popover = Popover::new();
    model_popover.set_parent(&model_btn);
    model_popover.set_has_arrow(true);
    model_popover.add_css_class("companion-popover");
    {
        let pop_clean = model_popover.downgrade();
        model_btn.connect_destroy(move |_| {
            if let Some(p) = pop_clean.upgrade() {
                p.unparent();
            }
        });
    }

    let model_list_wrap = GtkBox::new(Orientation::Vertical, 4);
    model_list_wrap.add_css_class("companion-popover-box");
    model_list_wrap.set_size_request(272, -1);
    let model_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(360)
        .build();
    model_scroll.set_child(Some(&model_list_wrap));
    model_popover.set_child(Some(&model_scroll));

    let populate_model_popover: Rc<dyn Fn(bool)> = {
        let wrap = model_list_wrap.clone();
        let pop = model_popover.clone();
        let gear_pop = gear_popover.clone();
        let refresh_c = refresh_ui_state.clone();
        let sync_g = sync_gear_fields.clone();
        let cache_rc = discovered_models.clone();
        let sw_tab_pop = switch_gear_tab.clone();
        Rc::new(move |is_loading: bool| {
            while let Some(ch) = wrap.first_child() {
                wrap.remove(&ch);
            }
            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();
            let active_p = cfg.omnibar.ai_provider;
            let active_m = cfg.omnibar.effective_ai_model();
            let cache = cache_rc.borrow();

            let mut visible_providers: Vec<BlazeAiProvider> = BlazeAiProvider::ALL
                .into_iter()
                .filter(|p| p.requires_api_key() && secrets.has_key_for(*p))
                .collect();

            let ollama_live_has_models = matches!(
                cache.get(&BlazeAiProvider::Ollama),
                Some(Ok(list)) if !list.is_empty()
            );
            if ollama_live_has_models || active_p == BlazeAiProvider::Ollama {
                visible_providers.push(BlazeAiProvider::Ollama);
            }

            if visible_providers.is_empty() {
                let title = Label::new(Some(if is_loading {
                    "Modeller kontrol ediliyor..."
                } else {
                    "Henüz API anahtarı eklenmedi"
                }));
                title.add_css_class("companion-section-lbl");
                title.set_halign(gtk4::Align::Start);
                wrap.append(&title);

                let note = Label::new(Some(
                    "Desteklenen modeller, API anahtarını girdiğinde veya yerel Ollama çalıştırdığında burada otomatik listelenir.",
                ));
                note.add_css_class("blazeai-msg-note");
                note.set_wrap(true);
                note.set_xalign(0.0);
                note.set_halign(gtk4::Align::Start);
                wrap.append(&note);

                let open_gear_btn = Button::with_label("API anahtarı gir");
                open_gear_btn.add_css_class("companion-reset-btn");
                open_gear_btn.set_margin_top(6);
                let pop_c = pop.clone();
                let gpop_c = gear_pop.clone();
                let sync_c = sync_g.clone();
                let sw_api = sw_tab_pop.clone();
                open_gear_btn.connect_clicked(move |_| {
                    pop_c.popdown();
                    sync_c();
                    sw_api("api");
                    gpop_c.popup();
                });
                wrap.append(&open_gear_btn);

                let open_local_btn = Button::with_label("Yerel model indir (1.5B–16B)");
                open_local_btn.add_css_class("companion-reset-btn");
                open_local_btn.set_margin_top(4);
                let pop_c2 = pop.clone();
                let gpop_c2 = gear_pop.clone();
                let sync_c2 = sync_g.clone();
                let sw_loc = sw_tab_pop.clone();
                open_local_btn.connect_clicked(move |_| {
                    pop_c2.popdown();
                    sync_c2();
                    sw_loc("local");
                    gpop_c2.popup();
                });
                wrap.append(&open_local_btn);
                return;
            }

            for provider in visible_providers {
                let hdr = Label::new(Some(provider.display_name()));
                hdr.add_css_class("companion-section-lbl");
                hdr.set_halign(gtk4::Align::Start);
                hdr.set_margin_top(4);
                wrap.append(&hdr);

                match cache.get(&provider) {
                    None => {
                        let loading_lbl = Label::new(Some(if is_loading {
                            "Desteklenen modeller yükleniyor..."
                        } else {
                            "Liste hazırlanıyor..."
                        }));
                        loading_lbl.add_css_class("blazeai-msg-note");
                        loading_lbl.set_halign(gtk4::Align::Start);
                        wrap.append(&loading_lbl);
                    }
                    Some(Err(err_msg)) => {
                        let err_lbl = Label::new(Some(&format!(
                            "Modeller alınamadı: {}",
                            truncate_chars(err_msg, 110)
                        )));
                        err_lbl.add_css_class("blazeai-msg-note");
                        err_lbl.set_wrap(true);
                        err_lbl.set_xalign(0.0);
                        err_lbl.set_halign(gtk4::Align::Start);
                        wrap.append(&err_lbl);
                    }
                    Some(Ok(models)) if models.is_empty() => {
                        let empty_lbl = Label::new(Some(if provider == BlazeAiProvider::Ollama {
                            "Yerel Ollama modeli bulunamadı"
                        } else {
                            "Bu anahtar için uyumlu sohbet modeli bulunamadı"
                        }));
                        empty_lbl.add_css_class("blazeai-msg-note");
                        empty_lbl.set_wrap(true);
                        empty_lbl.set_xalign(0.0);
                        empty_lbl.set_halign(gtk4::Align::Start);
                        wrap.append(&empty_lbl);
                    }
                    Some(Ok(models)) => {
                        for model_name in models {
                            let is_cur = active_p == provider && active_m == *model_name;
                            let btn = Button::new();
                            btn.add_css_class("companion-opt-btn");
                            if is_cur {
                                btn.add_css_class("companion-opt-active");
                            }
                            let row = GtkBox::new(Orientation::Horizontal, 8);
                            let lbl = Label::new(Some(model_name));
                            lbl.add_css_class("companion-banner-desc");
                            lbl.set_hexpand(true);
                            lbl.set_halign(gtk4::Align::Start);
                            row.append(&lbl);
                            if is_cur {
                                row.append(&build_symbolic_icon(
                                    &["object-select-symbolic"],
                                    "omnibar-header-icon",
                                ));
                            }
                            btn.set_child(Some(&row));

                            let pop_c = pop.clone();
                            let refresh_in = refresh_c.clone();
                            let sync_in = sync_g.clone();
                            let m_owned = model_name.clone();
                            btn.connect_clicked(move |_| {
                                let _ = SolarConfig::update(|c| {
                                    let switched_provider = c.omnibar.ai_provider != provider;
                                    c.omnibar.ai_provider = provider;
                                    c.omnibar.ai_model = m_owned.clone();
                                    if switched_provider {
                                        c.omnibar.ai_custom_base_url =
                                            provider.default_base_url().to_string();
                                    }
                                });
                                sync_in();
                                refresh_in();
                                pop_c.popdown();
                            });
                            wrap.append(&btn);
                        }
                    }
                }
            }

            wrap.append(&Separator::new(Orientation::Horizontal));
            let footer_local_btn = Button::with_label("Yerel model indir / yönet (1.5B–16B)");
            footer_local_btn.add_css_class("companion-reset-btn");
            footer_local_btn.set_margin_top(4);
            let pop_fl = pop.clone();
            let gpop_fl = gear_pop.clone();
            let sync_fl = sync_g.clone();
            let sw_fl = sw_tab_pop.clone();
            footer_local_btn.connect_clicked(move |_| {
                pop_fl.popdown();
                sync_fl();
                sw_fl("local");
                gpop_fl.popup();
            });
            wrap.append(&footer_local_btn);
        })
    };

    let schedule_active_provider_probe: Rc<dyn Fn(u64)> = {
        let gen_rc = probe_generation.clone();
        let cache_rc = discovered_models.clone();
        let status_lbl = gear_probe_status_lbl.clone();
        let sync_flag = syncing_gear.clone();
        let m_entry = gear_model_entry.clone();
        let refresh_c = refresh_ui_state.clone();
        let pop_fill = populate_model_popover.clone();
        let pop_w = model_popover.downgrade();
        let cloud_c = fluid_cloud.clone();

        Rc::new(move |delay_ms: u64| {
            let my_gen = gen_rc.get().wrapping_add(1);
            gen_rc.set(my_gen);

            let gen_in = gen_rc.clone();
            let cache_in = cache_rc.clone();
            let status_in = status_lbl.clone();
            let sync_in = sync_flag.clone();
            let m_entry_in = m_entry.clone();
            let refresh_in = refresh_c.clone();
            let pop_fill_in = pop_fill.clone();
            let pop_w_in = pop_w.clone();
            let cloud_in = cloud_c.clone();

            glib::timeout_add_local_once(std::time::Duration::from_millis(delay_ms), move || {
                if gen_in.get() != my_gen {
                    return;
                }
                let cfg = SolarConfig::load();
                let secrets = BlazeAiSecrets::load();
                let provider = cfg.omnibar.ai_provider;
                let key = secrets.key_for(provider).trim().to_string();

                if provider.requires_api_key() && key.is_empty() {
                    cache_in.borrow_mut().remove(&provider);
                    status_in.set_text(
                        "API anahtarı girildiğinde destekli modeller otomatik listelenir",
                    );
                    refresh_in();
                    if pop_w_in.upgrade().is_some_and(|p| p.is_visible()) {
                        pop_fill_in(false);
                    }
                    return;
                }
                if provider.requires_api_key() && key.len() < 8 {
                    status_in.set_text("Anahtar tamamlandığında modeller sorgulanacak...");
                    return;
                }

                let base_url = cfg.omnibar.effective_ai_base_url();
                status_in.set_text(&format!(
                    "{} destekli modelleri sorgulanıyor...",
                    provider.display_name()
                ));

                let (tx, rx) = async_channel::bounded::<Result<Vec<String>, String>>(1);
                std::thread::spawn(move || {
                    let res = probe_provider_models(provider, &key, &base_url);
                    let _ = tx.send_blocking(res);
                });

                glib::spawn_future_local(async move {
                    if let Ok(res) = rx.recv().await {
                        if gen_in.get() != my_gen {
                            return;
                        }
                        match &res {
                            Ok(models) if !models.is_empty() => {
                                let cur_cfg = SolarConfig::load();
                                if cur_cfg.omnibar.ai_provider == provider {
                                    if let Some(chosen) = pick_preferred_live_model(
                                        provider,
                                        &cur_cfg.omnibar.effective_ai_model(),
                                        models,
                                    ) {
                                        if chosen != cur_cfg.omnibar.ai_model {
                                            let _ = SolarConfig::update(|c| {
                                                if c.omnibar.ai_provider == provider {
                                                    c.omnibar.ai_model = chosen.clone();
                                                }
                                            });
                                            sync_in.set(true);
                                            m_entry_in.set_text(&chosen);
                                            sync_in.set(false);
                                        }
                                        status_in.set_text(&format!(
                                            "{} destekli model bulundu ({})",
                                            models.len(),
                                            chosen
                                        ));
                                    } else {
                                        status_in.set_text(&format!(
                                            "{} destekli model bulundu",
                                            models.len()
                                        ));
                                    }
                                }
                                cloud_in.pulse(
                                    "Destekli modeller yüklendi",
                                    FluidCloudTone::Success,
                                    false,
                                );
                            }
                            Ok(_) => {
                                status_in.set_text(if provider == BlazeAiProvider::Ollama {
                                    "Yerel Ollama modeli bulunamadı"
                                } else {
                                    "Bu anahtar için destekli sohbet modeli bulunamadı"
                                });
                            }
                            Err(err) => {
                                status_in.set_text(&format!(
                                    "Modeller alınamadı: {}",
                                    truncate_chars(err, 90)
                                ));
                            }
                        }
                        cache_in.borrow_mut().insert(provider, res);
                        refresh_in();
                        if pop_w_in.upgrade().is_some_and(|p| p.is_visible()) {
                            pop_fill_in(false);
                        }
                    }
                });
            });
        })
    };

    {
        let sync_flag = syncing_gear.clone();
        let m_entry = gear_model_entry.clone();
        let u_entry = gear_url_entry.clone();
        let k_entry = gear_key_entry.clone();
        let refresh_c = refresh_ui_state.clone();
        let probe_fn = schedule_active_provider_probe.clone();
        provider_dropdown.connect_selected_notify(move |dd| {
            if sync_flag.get() {
                return;
            }
            let sel = dd.selected() as usize;
            if let Some(provider) = BlazeAiProvider::ALL.get(sel).copied() {
                sync_flag.set(true);
                let mut new_model = provider.default_model().to_string();
                let mut new_url = provider.default_base_url().to_string();
                let _ = SolarConfig::update(|c| {
                    c.omnibar.ai_provider = provider;
                    c.omnibar.ai_model = provider.default_model().to_string();
                    c.omnibar.ai_custom_base_url = provider.default_base_url().to_string();
                    new_model = c.omnibar.ai_model.clone();
                    new_url = c.omnibar.effective_ai_base_url();
                });
                let secrets = BlazeAiSecrets::load();
                m_entry.set_text(&new_model);
                u_entry.set_text(&new_url);
                u_entry.set_sensitive(matches!(
                    provider,
                    BlazeAiProvider::OpenAiCompatible | BlazeAiProvider::Ollama
                ));
                k_entry.set_text(secrets.key_for(provider));
                k_entry.set_sensitive(provider.requires_api_key());
                sync_flag.set(false);
                refresh_c();
                probe_fn(120);
            }
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        let refresh_c = refresh_ui_state.clone();
        gear_model_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let txt = e.text().trim().to_string();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_model = if txt.is_empty() {
                    c.omnibar.ai_provider.default_model().to_string()
                } else {
                    txt
                };
            });
            refresh_c();
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        let probe_fn = schedule_active_provider_probe.clone();
        gear_url_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let txt = e.text().trim().to_string();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_custom_base_url = txt;
            });
            probe_fn(500);
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        let refresh_c = refresh_ui_state.clone();
        let probe_fn = schedule_active_provider_probe.clone();
        gear_key_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let cfg = SolarConfig::load();
            if !cfg.omnibar.ai_provider.requires_api_key() {
                return;
            }
            let mut secrets = BlazeAiSecrets::load();
            secrets.set_key_for(cfg.omnibar.ai_provider, &e.text());
            let _ = secrets.save();
            refresh_c();
            probe_fn(450);
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        let refresh_c = refresh_ui_state.clone();
        chk_persist.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_persist_chat = active;
            });
            refresh_c();
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_local_mem.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_local_memory_enabled = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_guide.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_include_blazeos_guide = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_slang.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_slang_enabled = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_web_research.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_web_research_enabled = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_agent_tools.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_agent_tools_enabled = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        chk_tts_auto.connect_toggled(move |cb| {
            if sync_flag.get() {
                return;
            }
            let active = cb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_voice_tts_auto = active;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        voice_dropdown.connect_selected_notify(move |dd| {
            if sync_flag.get() {
                return;
            }
            let chosen = match dd.selected() {
                1 => PIPER_VOICE_PERSEPHONE,
                2 => PIPER_VOICE_KOZ,
                _ => "",
            };
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_voice_tts_voice = chosen.to_string();
            });
        });
    }

    {
        let cloud_b = fluid_cloud.clone();
        open_brain_btn.connect_clicked(move |_| {
            let brain_dir = BrainStore::open_default().root_dir;
            let _ = Command::new("xdg-open").arg(&brain_dir).spawn();
            cloud_b.pulse("Beyin klasörü açıldı", FluidCloudTone::Info, false);
        });
    }

    // On-demand Turkish Piper TTS pack download
    {
        let cancel_arc = voice_dl_cancel.clone();
        let status_lbl = voice_status_lbl.clone();
        let sync_g = sync_gear_fields.clone();
        let cloud_v = fluid_cloud.clone();
        dl_tts_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            cancel_arc.store(false, Ordering::Relaxed);
            let cancel_t = cancel_arc.clone();
            let (tx, rx) = async_channel::unbounded::<Result<String, String>>();
            std::thread::spawn(move || {
                let tx_prog = tx.clone();
                let res = install_turkish_tts_pack(&cancel_t, move |msg| {
                    let _ = tx_prog.send_blocking(Ok(msg.to_string()));
                });
                match res {
                    Ok(()) => {
                        let _ = tx.send_blocking(Ok("__DONE__".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(Err(e));
                    }
                }
            });
            let btn_ref = btn.clone();
            let st_ref = status_lbl.clone();
            let sync_ref = sync_g.clone();
            let cloud_ref = cloud_v.clone();
            glib::spawn_future_local(async move {
                while let Ok(item) = rx.recv().await {
                    match item {
                        Ok(s) if s == "__DONE__" => {
                            btn_ref.set_sensitive(true);
                            sync_ref();
                            cloud_ref.pulse("Türkçe ses paketi hazır", FluidCloudTone::Success, false);
                            break;
                        }
                        Ok(msg) => st_ref.set_text(&msg),
                        Err(e) => {
                            btn_ref.set_sensitive(true);
                            st_ref.set_text(&format!("Hata: {}", truncate_chars(&e, 100)));
                            cloud_ref.pulse("Ses paketi indirilemedi", FluidCloudTone::Danger, false);
                            break;
                        }
                    }
                }
            });
        });
    }

    // On-demand Whisper Turkish STT model download
    {
        let cancel_arc = voice_dl_cancel.clone();
        let status_lbl = voice_status_lbl.clone();
        let sync_g = sync_gear_fields.clone();
        let cloud_v = fluid_cloud.clone();
        dl_stt_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            cancel_arc.store(false, Ordering::Relaxed);
            let stt_model = SolarConfig::load().omnibar.ai_voice_stt_model;
            let cancel_t = cancel_arc.clone();
            let (tx, rx) = async_channel::unbounded::<Result<String, String>>();
            std::thread::spawn(move || {
                let tx_prog = tx.clone();
                let res = install_whisper_stt_model(&stt_model, &cancel_t, move |msg| {
                    let _ = tx_prog.send_blocking(Ok(msg.to_string()));
                });
                match res {
                    Ok(()) => {
                        let _ = tx.send_blocking(Ok("__DONE__".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(Err(e));
                    }
                }
            });
            let btn_ref = btn.clone();
            let st_ref = status_lbl.clone();
            let sync_ref = sync_g.clone();
            let cloud_ref = cloud_v.clone();
            glib::spawn_future_local(async move {
                while let Ok(item) = rx.recv().await {
                    match item {
                        Ok(s) if s == "__DONE__" => {
                            btn_ref.set_sensitive(true);
                            sync_ref();
                            cloud_ref.pulse("Whisper modeli hazır", FluidCloudTone::Success, false);
                            break;
                        }
                        Ok(msg) => st_ref.set_text(&msg),
                        Err(e) => {
                            btn_ref.set_sensitive(true);
                            st_ref.set_text(&format!("Hata: {}", truncate_chars(&e, 100)));
                            cloud_ref.pulse("Whisper indirilemedi", FluidCloudTone::Danger, false);
                            break;
                        }
                    }
                }
            });
        });
    }

    // Local Model Hub (1.5B - 16B) dynamic builder + hardware probe + pull/cancel/delete
    let hw_profile_rc: Rc<RefCell<Option<HardwareProfile>>> = Rc::new(RefCell::new(None));
    let pull_busy = Rc::new(Cell::new(false));

    let refresh_local_hub: Rc<dyn Fn()> = {
        let tiers_box = local_tiers_box.clone();
        let hw_lbl = hw_summary_lbl.clone();
        let eng_lbl = ollama_status_lbl.clone();
        let eng_btn = ollama_setup_btn.clone();
        let hw_rc = hw_profile_rc.clone();
        let p_box = pull_progress_box.clone();
        let p_lbl = pull_status_lbl.clone();
        let p_bar = pull_progress_bar.clone();
        let p_busy = pull_busy.clone();
        let p_cancel = local_pull_cancel.clone();
        let cache_rc = discovered_models.clone();
        let refresh_ui = refresh_ui_state.clone();
        let sync_g = sync_gear_fields.clone();
        let cloud_l = fluid_cloud.clone();
        let gear_pop = gear_popover.clone();

        Rc::new(move || {
            let cfg = SolarConfig::load();
            let ollama_url = if cfg.omnibar.ai_provider == BlazeAiProvider::Ollama
                && !cfg.omnibar.ai_custom_base_url.trim().is_empty()
            {
                cfg.omnibar.effective_ai_base_url()
            } else {
                BlazeAiProvider::Ollama.default_base_url().to_string()
            };
            let existing_hw = hw_rc.borrow().clone();
            let (tx, rx) =
                async_channel::bounded::<(HardwareProfile, bool, bool, Vec<String>)>(1);
            std::thread::spawn(move || {
                let hw = existing_hw.unwrap_or_else(HardwareProfile::probe);
                let has_bin = find_ollama_binary().is_some();
                let models = probe_ollama_models(&ollama_url).unwrap_or_default();
                let serving = is_ollama_serving(&ollama_url);
                let _ = tx.send_blocking((hw, has_bin, serving, models));
            });

            let tiers_box_c = tiers_box.clone();
            let hw_lbl_c = hw_lbl.clone();
            let eng_lbl_c = eng_lbl.clone();
            let eng_btn_c = eng_btn.clone();
            let hw_rc_c = hw_rc.clone();
            let p_box_c = p_box.clone();
            let p_lbl_c = p_lbl.clone();
            let p_bar_c = p_bar.clone();
            let p_busy_c = p_busy.clone();
            let p_cancel_c = p_cancel.clone();
            let cache_rc_c = cache_rc.clone();
            let refresh_ui_c = refresh_ui.clone();
            let sync_g_c = sync_g.clone();
            let cloud_l_c = cloud_l.clone();
            let gear_pop_c = gear_pop.clone();

            glib::spawn_future_local(async move {
                let Ok((hw, has_bin, serving, installed_models)) = rx.recv().await else {
                    return;
                };
                *hw_rc_c.borrow_mut() = Some(hw.clone());
                hw_lbl_c.set_text(&hw.summary_tr());
                if serving {
                    cache_rc_c
                        .borrow_mut()
                        .insert(BlazeAiProvider::Ollama, Ok(installed_models.clone()));
                    eng_lbl_c.set_text(&format!(
                        "Yerel Ollama hazır (127.0.0.1:11434 · {} yüklü model)",
                        installed_models.len()
                    ));
                    eng_btn_c.set_visible(false);
                } else if has_bin {
                    eng_lbl_c.set_text("Yerel Ollama kurulu ancak duraklatılmış");
                    eng_btn_c.set_label("Motoru başlat");
                    eng_btn_c.set_visible(true);
                } else {
                    eng_lbl_c.set_text("Yerel motor kurulu değil (sudo gerektirmez)");
                    eng_btn_c.set_label("Motoru kur");
                    eng_btn_c.set_visible(true);
                }

                while let Some(ch) = tiers_box_c.first_child() {
                    tiers_box_c.remove(&ch);
                }

                let cur_cfg = SolarConfig::load();
                for tier in LOCAL_MODEL_TIERS {
                    let is_rec = tier.tier_id == hw.recommended_tier_id;
                    let is_installed = installed_models.iter().any(|m| {
                        m == tier.primary_tag
                            || m.starts_with(&format!("{}:", tier.primary_tag))
                            || m == tier.alt_tag
                    });
                    let is_active = cur_cfg.omnibar.ai_provider == BlazeAiProvider::Ollama
                        && cur_cfg.omnibar.effective_ai_model() == tier.primary_tag;

                    let card = GtkBox::new(Orientation::Vertical, 4);
                    card.add_css_class("blazeai-local-card");

                    let top = GtkBox::new(Orientation::Horizontal, 6);
                    let title = Label::new(Some(&format!(
                        "{} ({})",
                        tier.label_tr, tier.primary_tag
                    )));
                    title.add_css_class("blazeai-msg-speaker");
                    title.set_hexpand(true);
                    title.set_halign(gtk4::Align::Start);
                    top.append(&title);

                    if is_rec {
                        let rec_b = Label::new(Some("Önerilen"));
                        rec_b.add_css_class("blazeai-local-rec-badge");
                        top.append(&rec_b);
                    }
                    card.append(&top);

                    let meta_lbl = Label::new(Some(&format!(
                        "{} · {} · {}{}",
                        tier.size_hint,
                        tier.desc_tr,
                        if tier.supports_vision { "Görsel" } else { "Metin" },
                        if tier.supports_thinking { " + Düşünme" } else { "" }
                    )));
                    meta_lbl.add_css_class("blazeai-msg-note");
                    meta_lbl.set_wrap(true);
                    meta_lbl.set_xalign(0.0);
                    meta_lbl.set_halign(gtk4::Align::Start);
                    card.append(&meta_lbl);

                    let btn_row = GtkBox::new(Orientation::Horizontal, 6);
                    if is_installed {
                        let use_btn = Button::with_label(if is_active { "Aktif" } else { "Kullan" });
                        use_btn.add_css_class("companion-reset-btn");
                        use_btn.set_sensitive(!is_active);
                        let tag = tier.primary_tag.to_string();
                        let sync_u = sync_g_c.clone();
                        let ref_u = refresh_ui_c.clone();
                        let pop_u = gear_pop_c.clone();
                        let cl_u = cloud_l_c.clone();
                        use_btn.connect_clicked(move |_| {
                            let _ = SolarConfig::update(|c| {
                                c.omnibar.ai_provider = BlazeAiProvider::Ollama;
                                c.omnibar.ai_model = tag.clone();
                                if c.omnibar.ai_custom_base_url.trim().is_empty() {
                                    c.omnibar.ai_custom_base_url =
                                        BlazeAiProvider::Ollama.default_base_url().to_string();
                                }
                            });
                            sync_u();
                            ref_u();
                            pop_u.popdown();
                            cl_u.pulse("Yerel model seçildi", FluidCloudTone::Success, false);
                        });
                        btn_row.append(&use_btn);

                        let del_btn = Button::with_label("Sil");
                        del_btn.add_css_class("danger-btn");
                        let tag_d = tier.primary_tag.to_string();
                        let cl_d = cloud_l_c.clone();
                        let card_d = card.clone();
                        del_btn.connect_clicked(move |b| {
                            b.set_sensitive(false);
                            let t_owned = tag_d.clone();
                            let url_d = BlazeAiProvider::Ollama.default_base_url().to_string();
                            let (dtx, drx) = async_channel::bounded::<Result<(), String>>(1);
                            std::thread::spawn(move || {
                                let _ = dtx.send_blocking(delete_ollama_model(&url_d, &t_owned));
                            });
                            let cl_in = cl_d.clone();
                            let b_in = b.clone();
                            let card_in = card_d.clone();
                            glib::spawn_future_local(async move {
                                match drx.recv().await {
                                    Ok(Ok(())) => {
                                        card_in.set_opacity(0.65);
                                        b_in.set_label("Silindi");
                                        cl_in.pulse("Yerel model silindi", FluidCloudTone::Info, false);
                                    }
                                    _ => {
                                        b_in.set_sensitive(true);
                                        cl_in.pulse("Model silinemedi", FluidCloudTone::Danger, false);
                                    }
                                }
                            });
                        });
                        btn_row.append(&del_btn);
                    } else {
                        let dl_btn = Button::with_label(&format!("İndir ({})", tier.size_hint));
                        dl_btn.add_css_class("companion-reset-btn");
                        let tag_p = tier.primary_tag.to_string();
                        let p_box_in = p_box_c.clone();
                        let p_lbl_in = p_lbl_c.clone();
                        let p_bar_in = p_bar_c.clone();
                        let p_busy_in = p_busy_c.clone();
                        let p_cancel_in = p_cancel_c.clone();
                        let cache_in = cache_rc_c.clone();
                        let sync_in = sync_g_c.clone();
                        let ref_in = refresh_ui_c.clone();
                        let cl_in = cloud_l_c.clone();
                        dl_btn.connect_clicked(move |btn| {
                            if p_busy_in.get() {
                                cl_in.pulse("Başka bir indirme sürüyor", FluidCloudTone::Warning, false);
                                return;
                            }
                            p_busy_in.set(true);
                            btn.set_sensitive(false);
                            p_cancel_in.store(false, Ordering::Relaxed);
                            p_box_in.set_visible(true);
                            p_bar_in.set_fraction(0.0);
                            p_bar_in.set_text(Some("0%"));
                            p_lbl_in.set_text(&format!("{} hazırlanıyor...", tag_p));

                            let cancel_t = p_cancel_in.clone();
                            let tag_t = tag_p.clone();
                            let url_t = BlazeAiProvider::Ollama.default_base_url().to_string();
                            let (ptx, prx) =
                                async_channel::unbounded::<Result<PullProgress, String>>();
                            std::thread::spawn(move || {
                                if !is_ollama_serving(&url_t) {
                                    if find_ollama_binary().is_none() {
                                        let ptx_i = ptx.clone();
                                        if let Err(e) = install_userspace_ollama(&cancel_t, move |s| {
                                            let _ = ptx_i.send_blocking(Ok(PullProgress {
                                                status: s.to_string(),
                                                completed: 0,
                                                total: 0,
                                                fraction: None,
                                                done: false,
                                                error: None,
                                            }));
                                        }) {
                                            let _ = ptx.send_blocking(Err(e));
                                            return;
                                        }
                                    }
                                    if let Err(e) = ensure_ollama_serving(&url_t) {
                                        let _ = ptx.send_blocking(Err(e));
                                        return;
                                    }
                                }
                                let ptx_u = ptx.clone();
                                match pull_ollama_model(&url_t, &tag_t, &cancel_t, move |prog| {
                                    let _ = ptx_u.send_blocking(Ok(prog));
                                }) {
                                    Ok(()) => {
                                        let _ = ptx.send_blocking(Ok(PullProgress {
                                            status: "__PULL_DONE__".to_string(),
                                            completed: 1,
                                            total: 1,
                                            fraction: Some(1.0),
                                            done: true,
                                            error: None,
                                        }));
                                    }
                                    Err(e) => {
                                        let _ = ptx.send_blocking(Err(e));
                                    }
                                }
                            });

                            let pb_u = p_box_in.clone();
                            let pl_u = p_lbl_in.clone();
                            let pbar_u = p_bar_in.clone();
                            let busy_u = p_busy_in.clone();
                            let btn_u = btn.clone();
                            let tag_done = tag_p.clone();
                            let cache_done = cache_in.clone();
                            let sync_done = sync_in.clone();
                            let ref_done = ref_in.clone();
                            let cl_done = cl_in.clone();
                            glib::spawn_future_local(async move {
                                while let Ok(ev) = prx.recv().await {
                                    match ev {
                                        Ok(p) if p.status == "__PULL_DONE__" => {
                                            busy_u.set(false);
                                            pb_u.set_visible(false);
                                            btn_u.set_label("Yüklendi (Aktif)");
                                            let _ = SolarConfig::update(|c| {
                                                c.omnibar.ai_provider = BlazeAiProvider::Ollama;
                                                c.omnibar.ai_model = tag_done.clone();
                                                c.omnibar.ai_custom_base_url =
                                                    BlazeAiProvider::Ollama
                                                        .default_base_url()
                                                        .to_string();
                                            });
                                            cache_done.borrow_mut().remove(&BlazeAiProvider::Ollama);
                                            sync_done();
                                            ref_done();
                                            cl_done.pulse(
                                                "Yerel model indirildi ve seçildi",
                                                FluidCloudTone::Success,
                                                false,
                                            );
                                            break;
                                        }
                                        Ok(p) => {
                                            if let Some(frac) = p.fraction {
                                                pbar_u.set_fraction(frac);
                                                pbar_u.set_text(Some(&format!(
                                                    "{}%",
                                                    (frac * 100.0).round() as u32
                                                )));
                                            } else {
                                                pbar_u.pulse();
                                            }
                                            pl_u.set_text(&format!(
                                                "{}: {}",
                                                tag_done,
                                                truncate_chars(&p.status, 48)
                                            ));
                                        }
                                        Err(e) => {
                                            busy_u.set(false);
                                            pb_u.set_visible(false);
                                            btn_u.set_sensitive(true);
                                            cl_done.pulse(
                                                &truncate_chars(&e, 44),
                                                FluidCloudTone::Danger,
                                                false,
                                            );
                                            break;
                                        }
                                    }
                                }
                            });
                        });
                        btn_row.append(&dl_btn);
                    }
                    card.append(&btn_row);
                    tiers_box_c.append(&card);
                }
            });
        })
    };

    {
        let cancel_p = local_pull_cancel.clone();
        pull_cancel_btn.connect_clicked(move |_| {
            cancel_p.store(true, Ordering::Relaxed);
        });
    }

    {
        let cancel_e = local_pull_cancel.clone();
        let busy_e = pull_busy.clone();
        let p_box_e = pull_progress_box.clone();
        let p_lbl_e = pull_status_lbl.clone();
        let p_bar_e = pull_progress_bar.clone();
        let ref_hub = refresh_local_hub.clone();
        let cl_e = fluid_cloud.clone();
        ollama_setup_btn.connect_clicked(move |btn| {
            if busy_e.get() {
                return;
            }
            busy_e.set(true);
            btn.set_sensitive(false);
            cancel_e.store(false, Ordering::Relaxed);
            p_box_e.set_visible(true);
            p_bar_e.pulse();
            let cancel_t = cancel_e.clone();
            let url_t = BlazeAiProvider::Ollama.default_base_url().to_string();
            let (tx, rx) = async_channel::unbounded::<Result<String, String>>();
            std::thread::spawn(move || {
                if find_ollama_binary().is_none() {
                    let tx_s = tx.clone();
                    if let Err(e) = install_userspace_ollama(&cancel_t, move |s| {
                        let _ = tx_s.send_blocking(Ok(s.to_string()));
                    }) {
                        let _ = tx.send_blocking(Err(e));
                        return;
                    }
                }
                let _ = tx.send_blocking(Ok("Yerel Ollama başlatılıyor...".to_string()));
                match ensure_ollama_serving(&url_t) {
                    Ok(()) => {
                        let _ = tx.send_blocking(Ok("__OK__".to_string()));
                    }
                    Err(e) => {
                        let _ = tx.send_blocking(Err(e));
                    }
                }
            });
            let busy_u = busy_e.clone();
            let btn_u = btn.clone();
            let pb_u = p_box_e.clone();
            let pl_u = p_lbl_e.clone();
            let ref_u = ref_hub.clone();
            let cl_u = cl_e.clone();
            glib::spawn_future_local(async move {
                while let Ok(msg) = rx.recv().await {
                    match msg {
                        Ok(s) if s == "__OK__" => {
                            busy_u.set(false);
                            btn_u.set_sensitive(true);
                            pb_u.set_visible(false);
                            ref_u();
                            cl_u.pulse("Yerel Ollama motoru hazır", FluidCloudTone::Success, false);
                            break;
                        }
                        Ok(s) => pl_u.set_text(&s),
                        Err(e) => {
                            busy_u.set(false);
                            btn_u.set_sensitive(true);
                            pb_u.set_visible(false);
                            cl_u.pulse(&truncate_chars(&e, 44), FluidCloudTone::Danger, false);
                            break;
                        }
                    }
                }
            });
        });
    }

    // Refresh live Ollama + Hardware state whenever the Local Models tab becomes active
    {
        let ref_hub = refresh_local_hub.clone();
        gear_stack.connect_visible_child_name_notify(move |st| {
            if st.visible_child_name().as_deref() == Some("local") {
                ref_hub();
            }
        });
    }
    {
        let pop = gear_popover.clone();
        let sync_g = sync_gear_fields.clone();
        let sw = switch_gear_tab.clone();
        local_hub_shortcut_btn.connect_clicked(move |_| {
            sync_g();
            sw("local");
            pop.popup();
        });
    }

    // Multimodal toolbar handlers: Thinking toggle, Vision capture, Stop TTS (Barge-in)
    {
        let cloud_t = fluid_cloud.clone();
        think_toggle.connect_toggled(move |tb| {
            let active = tb.is_active();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_thinking_mode = active;
            });
            cloud_t.pulse(
                if active {
                    "Derin düşünme modu açık"
                } else {
                    "Hızlı yanıt modu aktif"
                },
                FluidCloudTone::Info,
                false,
            );
        });
    }

    {
        let pv = pending_vision.clone();
        let badge_r = vision_badge_row.clone();
        vision_clear_btn.connect_clicked(move |_| {
            *pv.borrow_mut() = None;
            badge_r.set_visible(false);
        });
    }

    {
        let pv = pending_vision.clone();
        let badge_r = vision_badge_row.clone();
        let badge_l = vision_badge_lbl.clone();
        let win_w = window.downgrade();
        let cloud_v = fluid_cloud.clone();
        vision_btn.connect_clicked(move |btn| {
            btn.set_sensitive(false);
            if let Some(w) = win_w.upgrade() {
                w.set_visible(false);
            }
            let pv_in = pv.clone();
            let br_in = badge_r.clone();
            let bl_in = badge_l.clone();
            let win_in = win_w.clone();
            let cl_in = cloud_v.clone();
            let btn_in = btn.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(180), move || {
                let (tx, rx) = async_channel::bounded::<Result<VisionAttachment, String>>(1);
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(capture_wayland_screen_for_vision());
                });
                glib::spawn_future_local(async move {
                    if let Some(w) = win_in.upgrade() {
                        w.set_visible(true);
                        w.present();
                    }
                    btn_in.set_sensitive(true);
                    match rx.recv().await {
                        Ok(Ok(att)) => {
                            bl_in.set_text(&format!(
                                "Ekran eklendi ({}x{})",
                                att.width, att.height
                            ));
                            *pv_in.borrow_mut() = Some(att);
                            br_in.set_visible(true);
                            cl_in.pulse("Ekran görüntüsü eklendi", FluidCloudTone::Success, false);
                        }
                        Ok(Err(err)) => {
                            cl_in.pulse(&truncate_chars(&err, 44), FluidCloudTone::Danger, false);
                        }
                        Err(_) => {}
                    }
                });
            });
        });
    }

    {
        let spk = voice_speaker.clone();
        stop_speak_btn.connect_clicked(move |b| {
            spk.stop();
            b.set_visible(false);
        });
    }

    {
        let st_add = store_rc.clone();
        let m_entry = mem_add_entry.clone();
        let rebuild_mem = rebuild_gear_memory_list.clone();
        let cloud_add = fluid_cloud.clone();
        let add_fact_fn = Rc::new(move || {
            let txt = m_entry.text().to_string();
            if st_add.borrow_mut().add_memory_fact(&txt).unwrap_or(false) {
                m_entry.set_text("");
                rebuild_mem();
                cloud_add.pulse("Yerel hafızaya eklendi", FluidCloudTone::Success, false);
            }
        });
        let add_click = add_fact_fn.clone();
        mem_add_btn.connect_clicked(move |_| add_click());
        mem_add_entry.connect_activate(move |_| add_fact_fn());
    }

    {
        let st_clr = store_rc.clone();
        let rebuild_mem = rebuild_gear_memory_list.clone();
        let refresh_c = refresh_ui_state.clone();
        let cloud_clr = fluid_cloud.clone();
        clear_all_btn.connect_clicked(move |_| {
            let _ = st_clr.borrow_mut().clear_all_memory();
            rebuild_mem();
            refresh_c();
            cloud_clr.pulse("Sohbet ve hafıza temizlendi", FluidCloudTone::Info, false);
        });
    }

    {
        let pop = gear_popover.clone();
        let sync_g = sync_gear_fields.clone();
        gear_btn.connect_clicked(move |_| {
            sync_g();
            pop.popup();
        });
    }

    // New chat button
    {
        let st_new = store_rc.clone();
        let refresh_c = refresh_ui_state.clone();
        let cloud_new = fluid_cloud.clone();
        new_chat_btn.connect_clicked(move |_| {
            let _ = st_new.borrow_mut().clear_session();
            refresh_c();
            cloud_new.pulse("Yeni sohbet başlatıldı", FluidCloudTone::Info, false);
        });
    }

    {
        let pop = model_popover.clone();
        let pop_fill = populate_model_popover.clone();
        let cache_rc = discovered_models.clone();
        let refresh_c = refresh_ui_state.clone();
        let sync_g = sync_gear_fields.clone();
        model_btn.connect_clicked(move |_| {
            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();

            // Build targets strictly for providers with configured API keys (+ Ollama auto-detection)
            let mut targets: Vec<(BlazeAiProvider, String, String)> = Vec::new();
            for p in BlazeAiProvider::ALL {
                if p.requires_api_key() {
                    let key = secrets.key_for(p).trim().to_string();
                    if !key.is_empty() {
                        let base = if cfg.omnibar.ai_provider == p {
                            cfg.omnibar.effective_ai_base_url()
                        } else {
                            p.default_base_url().to_string()
                        };
                        targets.push((p, key, base));
                    } else {
                        cache_rc.borrow_mut().remove(&p);
                    }
                }
            }
            let ollama_url = if cfg.omnibar.ai_provider == BlazeAiProvider::Ollama
                && !cfg.omnibar.ai_custom_base_url.trim().is_empty()
            {
                cfg.omnibar.effective_ai_base_url()
            } else {
                BlazeAiProvider::Ollama.default_base_url().to_string()
            };
            targets.push((BlazeAiProvider::Ollama, String::new(), ollama_url));

            // Always refresh any uncached or previously errored configured provider
            let pending_targets: Vec<(BlazeAiProvider, String, String)> = {
                let cache = cache_rc.borrow();
                targets
                    .into_iter()
                    .filter(|(p, _, _)| !matches!(cache.get(p), Some(Ok(v)) if !v.is_empty()))
                    .collect()
            };

            pop_fill(!pending_targets.is_empty());
            pop.popup();

            if pending_targets.is_empty() {
                return;
            }

            let (tx, rx) =
                async_channel::bounded::<Vec<(BlazeAiProvider, Result<Vec<String>, String>)>>(1);
            std::thread::spawn(move || {
                let mut results = Vec::with_capacity(pending_targets.len());
                for (p, key, base) in pending_targets {
                    let res = probe_provider_models(p, &key, &base);
                    results.push((p, res));
                }
                let _ = tx.send_blocking(results);
            });

            let cache_done = cache_rc.clone();
            let pop_fill_done = pop_fill.clone();
            let pop_weak = pop.downgrade();
            let refresh_done = refresh_c.clone();
            let sync_done = sync_g.clone();
            glib::spawn_future_local(async move {
                if let Ok(results) = rx.recv().await {
                    let cur_cfg = SolarConfig::load();
                    let mut model_upgraded = false;
                    {
                        let mut cache = cache_done.borrow_mut();
                        for (p, res) in results {
                            if p == cur_cfg.omnibar.ai_provider {
                                if let Ok(ref models) = res {
                                    if let Some(chosen) = pick_preferred_live_model(
                                        p,
                                        &cur_cfg.omnibar.effective_ai_model(),
                                        models,
                                    ) {
                                        if chosen != cur_cfg.omnibar.ai_model {
                                            let _ = SolarConfig::update(|c| {
                                                if c.omnibar.ai_provider == p {
                                                    c.omnibar.ai_model = chosen.clone();
                                                }
                                            });
                                            model_upgraded = true;
                                        }
                                    }
                                }
                            }
                            cache.insert(p, res);
                        }
                    }
                    if model_upgraded {
                        sync_done();
                        refresh_done();
                    }
                    if pop_weak.upgrade().is_some_and(|p| p.is_visible()) {
                        pop_fill_done(false);
                    }
                }
            });
        });
    }

    // Quietly warm up live supported models on startup when a key is already saved
    if !initial_cfg.omnibar.ai_provider.requires_api_key()
        || initial_secrets.has_key_for(initial_cfg.omnibar.ai_provider)
    {
        schedule_active_provider_probe(150);
    }

    // =========================================================================
    // SEND PROMPT TO BLAZEAI (or memory command / web search fallback)
    // =========================================================================
    let dispatch_blazeai_prompt: Rc<dyn Fn(String)> = {
        let st_rc = store_rc.clone();
        let sending_rc = is_sending.clone();
        let send_b = send_btn.clone();
        let av = avatar.clone();
        let refresh_c = refresh_ui_state.clone();
        let rebuild_mem = rebuild_gear_memory_list.clone();
        let cloud_c = fluid_cloud.clone();
        let left_p = left_pane.clone();
        let root_b = root_box.clone();
        let win_w = window.downgrade();
        let spk_d = voice_speaker.clone();
        let stop_b_d = stop_speak_btn.clone();
        let pv_d = pending_vision.clone();
        let v_badge_d = vision_badge_row.clone();
        let web_t_d = web_toggle.clone();

        Rc::new(move |raw_prompt: String| {
            let prompt = raw_prompt.trim().to_string();
            if prompt.is_empty() || sending_rc.get() {
                return;
            }

            // Instant barge-in: stop ongoing speech whenever user sends a new prompt
            spk_d.stop();
            stop_b_d.set_visible(false);

            // Ensure left panel is open if triggered from right pane AiQuery
            if !left_p.is_visible() {
                left_p.set_visible(true);
                let (w, h) = window_dimensions_for_panel_state(true);
                root_b.set_size_request(w, h);
                if let Some(win) = win_w.upgrade() {
                    win.set_default_size(w, h);
                }
                let _ = SolarConfig::update(|c| {
                    c.omnibar.companion_panel_open = true;
                });
            }

            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();

            // 1. Check explicit local memory command ("hatırla: ...", "not al: ...")
            if let Some(fact) = extract_user_memory_command(&prompt) {
                let ts = chrono::Local::now().to_rfc3339();
                {
                    let mut st = st_rc.borrow_mut();
                    let _ = st.append_message(
                        ChatMessage {
                            role: ChatRole::User,
                            content: prompt.clone(),
                            timestamp: ts.clone(),
                            speaker_name: "Sen".to_string(),
                            suggested_actions: Vec::new(),
                            thinking_text: String::new(),
                            file_proposals: Vec::new(),
                        },
                        cfg.omnibar.ai_persist_chat,
                    );
                    if cfg.omnibar.ai_local_memory_enabled {
                        let _ = st.add_memory_fact(&fact);
                        let _ = st.append_message(
                            ChatMessage {
                                role: ChatRole::Assistant,
                                content: format!("Yerel hafızaya kaydedildi: {}", fact),
                                timestamp: chrono::Local::now().to_rfc3339(),
                                speaker_name: cfg.omnibar.effective_companion_name(),
                                suggested_actions: Vec::new(),
                                thinking_text: String::new(),
                                file_proposals: Vec::new(),
                            },
                            cfg.omnibar.ai_persist_chat,
                        );
                    } else {
                        let _ = st.append_message(
                            ChatMessage {
                                role: ChatRole::SystemNote,
                                content: "Uzun vadeli yerel hafıza ayarlardan kapalı".to_string(),
                                timestamp: chrono::Local::now().to_rfc3339(),
                                speaker_name: cfg.omnibar.effective_companion_name(),
                                suggested_actions: Vec::new(),
                                thinking_text: String::new(),
                                file_proposals: Vec::new(),
                            },
                            cfg.omnibar.ai_persist_chat,
                        );
                    }
                }
                rebuild_mem();
                refresh_c();
                av.pulse_temporary_expression(CompanionExpression::Happy, 2500);
                cloud_c.pulse("Yerel hafıza güncellendi", FluidCloudTone::Success, false);
                return;
            }

            // 2. Check automatic web search fallback immediately for non-Ollama empty keys
            if let AiDispatchMode::FallbackToWebSearch { url, engine } =
                resolve_dispatch_mode(&cfg.omnibar, &secrets, &prompt)
            {
                let _ = Command::new("xdg-open").arg(&url).spawn();
                let _ = st_rc.borrow_mut().append_message(
                    ChatMessage {
                        role: ChatRole::SystemNote,
                        content: format!(
                            "{} ile web'de arandı (API anahtarı tanımlı değil): {}",
                            engine, prompt
                        ),
                        timestamp: chrono::Local::now().to_rfc3339(),
                        speaker_name: cfg.omnibar.effective_companion_name(),
                        suggested_actions: Vec::new(),
                        thinking_text: String::new(),
                        file_proposals: Vec::new(),
                    },
                    cfg.omnibar.ai_persist_chat,
                );
                refresh_c();
                cloud_c.pulse(&format!("{} ile açıldı", engine), FluidCloudTone::Info, false);
                return;
            }

            let attached_vision = pv_d.borrow_mut().take();
            v_badge_d.set_visible(false);
            let force_web = web_t_d.is_active();
            web_t_d.set_active(false);

            let display_prompt = if attached_vision.is_some() {
                format!("[Ekran görüntüsü] {}", prompt)
            } else {
                prompt.clone()
            };

            // 3. Append user turn & execute provider request off the GTK main thread
            let (history_snapshot, mem_snapshot) = {
                let mut st = st_rc.borrow_mut();
                let _ = st.append_message(
                    ChatMessage {
                        role: ChatRole::User,
                        content: display_prompt,
                        timestamp: chrono::Local::now().to_rfc3339(),
                        speaker_name: "Sen".to_string(),
                        suggested_actions: Vec::new(),
                        thinking_text: String::new(),
                        file_proposals: Vec::new(),
                    },
                    cfg.omnibar.ai_persist_chat,
                );
                (st.messages.clone(), st.memory_facts.clone())
            };

            sending_rc.set(true);
            send_b.set_sensitive(false);
            av.set_waiting(true);
            refresh_c();
            cloud_c.pulse(
                if force_web {
                    "Web araştırılıyor ve yanıt hazırlanıyor"
                } else {
                    "BlazeAI düşünüyor"
                },
                FluidCloudTone::Info,
                false,
            );

            let (tx, rx) = async_channel::bounded::<Result<ChatCompletionOutcome, String>>(1);
            let omnibar_cfg = cfg.omnibar.clone();
            let prompt_clone = prompt.clone();
            std::thread::spawn(move || {
                let res = send_chat_completion_with_options(
                    &omnibar_cfg,
                    &secrets,
                    &mem_snapshot,
                    &history_snapshot,
                    &prompt_clone,
                    attached_vision.as_ref(),
                    force_web,
                );
                let _ = tx.send_blocking(res);
            });

            let st_done = st_rc.clone();
            let sending_done = sending_rc.clone();
            let send_b_done = send_b.clone();
            let av_done = av.clone();
            let refresh_done = refresh_c.clone();
            let rebuild_mem_done = rebuild_mem.clone();
            let cloud_done = cloud_c.clone();
            let spk_done = spk_d.clone();
            let stop_b_done = stop_b_d.clone();

            glib::spawn_future_local(async move {
                let outcome = rx.recv().await;
                sending_done.set(false);
                send_b_done.set_sensitive(true);
                av_done.set_waiting(false);

                let latest_cfg = SolarConfig::load();
                match outcome {
                    Ok(Ok(ChatCompletionOutcome::FallbackToWebSearch { url, engine })) => {
                        let _ = Command::new("xdg-open").arg(&url).spawn();
                        let _ = st_done.borrow_mut().append_message(
                            ChatMessage {
                                role: ChatRole::SystemNote,
                                content: format!(
                                    "Yerel Ollama kapalı; {} ile web araması açıldı",
                                    engine
                                ),
                                timestamp: chrono::Local::now().to_rfc3339(),
                                speaker_name: latest_cfg.omnibar.effective_companion_name(),
                                suggested_actions: Vec::new(),
                                thinking_text: String::new(),
                                file_proposals: Vec::new(),
                            },
                            latest_cfg.omnibar.ai_persist_chat,
                        );
                        av_done.pulse_temporary_expression(CompanionExpression::Confused, 2500);
                        cloud_done.pulse(&format!("{} ile açıldı", engine), FluidCloudTone::Warning, false);
                    }
                    Ok(Ok(ChatCompletionOutcome::Reply {
                        cleaned_text,
                        thinking_text,
                        extracted_facts,
                        suggested_actions,
                        file_proposals,
                        upgraded_model,
                        fallback_model_used,
                        ..
                    })) => {
                        if let Some(new_model) = upgraded_model {
                            let _ = SolarConfig::update(|c| {
                                if c.omnibar.ai_provider == BlazeAiProvider::Gemini {
                                    c.omnibar.ai_model = new_model.clone();
                                }
                            });
                        }
                        let speaker_badge = if let Some(ref fb) = fallback_model_used {
                            format!(
                                "{} · {} (yoğunluk yedeği)",
                                latest_cfg.omnibar.effective_companion_name(),
                                fb
                            )
                        } else {
                            latest_cfg.omnibar.effective_companion_name()
                        };
                        let speak_copy = cleaned_text.clone();
                        {
                            let mut st = st_done.borrow_mut();
                            if latest_cfg.omnibar.ai_local_memory_enabled {
                                for f in extracted_facts {
                                    let _ = st.add_memory_fact(&f);
                                }
                            }
                            let _ = st.append_message(
                                ChatMessage {
                                    role: ChatRole::Assistant,
                                    content: cleaned_text,
                                    timestamp: chrono::Local::now().to_rfc3339(),
                                    speaker_name: speaker_badge,
                                    suggested_actions,
                                    thinking_text,
                                    file_proposals,
                                },
                                latest_cfg.omnibar.ai_persist_chat,
                            );
                        }
                        rebuild_mem_done();
                        av_done.pulse_temporary_expression(CompanionExpression::Happy, 2500);
                        cloud_done.pulse("Yanıt hazır", FluidCloudTone::Success, false);

                        if latest_cfg.omnibar.ai_voice_tts_auto && !speak_copy.trim().is_empty() {
                            let (stx, srx) = async_channel::bounded::<()>(1);
                            if spk_done
                                .speak_async(
                                    speak_copy,
                                    latest_cfg.omnibar.effective_tts_voice(),
                                    move || {
                                        let _ = stx.send_blocking(());
                                    },
                                )
                                .is_ok()
                            {
                                stop_b_done.set_visible(true);
                                let sb_c = stop_b_done.clone();
                                glib::spawn_future_local(async move {
                                    let _ = srx.recv().await;
                                    sb_c.set_visible(false);
                                });
                            }
                        }
                    }
                    Ok(Err(err_msg)) => {
                        let _ = st_done.borrow_mut().append_message(
                            ChatMessage {
                                role: ChatRole::SystemNote,
                                content: format!("Hata: {}", err_msg),
                                timestamp: chrono::Local::now().to_rfc3339(),
                                speaker_name: latest_cfg.omnibar.effective_companion_name(),
                                suggested_actions: Vec::new(),
                                thinking_text: String::new(),
                                file_proposals: Vec::new(),
                            },
                            latest_cfg.omnibar.ai_persist_chat,
                        );
                        av_done.pulse_temporary_expression(CompanionExpression::Confused, 2500);
                        cloud_done.pulse("Bağlantı hatası", FluidCloudTone::Danger, false);
                    }
                    Err(_) => {
                        av_done.pulse_temporary_expression(CompanionExpression::Confused, 2500);
                    }
                }
                refresh_done();
            });
        })
    };

    // Push-to-talk / click-to-talk microphone handler (PipeWire pw-record -> whisper-cli + Barge-in)
    {
        let rec_rc = active_recording.clone();
        let spk_m = voice_speaker.clone();
        let stop_b_m = stop_speak_btn.clone();
        let dispatch_m = dispatch_blazeai_prompt.clone();
        let cloud_m = fluid_cloud.clone();
        let av_m = avatar.clone();
        mic_btn.connect_clicked(move |btn| {
            // Barge-in: halt active TTS immediately when microphone is pressed
            spk_m.stop();
            stop_b_m.set_visible(false);

            let maybe_rec = rec_rc.borrow_mut().take();
            if let Some(rec) = maybe_rec {
                btn.remove_css_class("blazeai-mic-active");
                btn.set_sensitive(false);
                btn.set_child(Some(&build_symbolic_icon(
                    &["audio-input-microphone-symbolic"],
                    "omnibar-header-icon",
                )));
                cloud_m.pulse("Ses çözülüyor...", FluidCloudTone::Info, false);
                let stt_model = SolarConfig::load().omnibar.ai_voice_stt_model;
                let (tx, rx) = async_channel::bounded::<Result<String, String>>(1);
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(rec.stop_and_transcribe(&stt_model));
                });
                let btn_u = btn.clone();
                let disp_u = dispatch_m.clone();
                let cl_u = cloud_m.clone();
                glib::spawn_future_local(async move {
                    btn_u.set_sensitive(true);
                    match rx.recv().await {
                        Ok(Ok(transcript)) => {
                            cl_u.pulse("Ses anlaşıldı", FluidCloudTone::Success, false);
                            disp_u(transcript);
                        }
                        Ok(Err(err)) => {
                            cl_u.pulse(&truncate_chars(&err, 44), FluidCloudTone::Warning, false);
                        }
                        Err(_) => {}
                    }
                });
            } else {
                match ActiveRecording::start() {
                    Ok(rec) => {
                        *rec_rc.borrow_mut() = Some(rec);
                        btn.add_css_class("blazeai-mic-active");
                        btn.set_child(Some(&build_symbolic_icon(
                            &["media-playback-stop-symbolic"],
                            "omnibar-header-icon",
                        )));
                        av_m.pulse_temporary_expression(CompanionExpression::Curious, 3500);
                        cloud_m.pulse("Dinliyorum (bitirmek için tekrar bas)", FluidCloudTone::Info, false);
                    }
                    Err(err) => {
                        cloud_m.pulse(&truncate_chars(&err, 44), FluidCloudTone::Danger, false);
                    }
                }
            }
        });
    }

    {
        let b_entry = blazeai_entry.clone();
        let dispatch = dispatch_blazeai_prompt.clone();
        send_btn.connect_clicked(move |_| {
            let txt = b_entry.text().to_string();
            if !txt.trim().is_empty() {
                b_entry.set_text("");
                dispatch(txt);
            }
        });
    }
    {
        let dispatch = dispatch_blazeai_prompt.clone();
        blazeai_entry.connect_activate(move |e| {
            let txt = e.text().to_string();
            if !txt.trim().is_empty() {
                e.set_text("");
                dispatch(txt);
            }
        });
    }

    // Sidebar toggle button (expand/collapse left panel 1040x520 <-> 680x492)
    {
        let left_t = left_pane.clone();
        let root_t = root_box.clone();
        let win_t = window.downgrade();
        let ent_focus = entry.clone();
        toggle_panel_btn.connect_clicked(move |_| {
            let next_open = !left_t.is_visible();
            left_t.set_visible(next_open);
            let (w, h) = window_dimensions_for_panel_state(next_open);
            root_t.set_size_request(w, h);
            if let Some(win) = win_t.upgrade() {
                win.set_default_size(w, h);
            }
            let _ = SolarConfig::update(|c| {
                c.omnibar.companion_panel_open = next_open;
            });
            if !next_open {
                ent_focus.grab_focus();
            }
        });
    }

    // Window-wide mouse motion tracking for companion eye gaze
    let motion_ctrl = EventControllerMotion::new();
    {
        let av = avatar.clone();
        motion_ctrl.connect_motion(move |_, x, y| {
            av.update_pointer(x, y);
        });
    }
    {
        let av = avatar.clone();
        motion_ctrl.connect_leave(move |_| {
            av.clear_pointer();
        });
    }
    window.add_controller(motion_ctrl);

    // =========================================================================
    // RIGHT LAUNCHER LOGIC + CAPTURE KEY CONTROLLER
    // =========================================================================
    let cached_apps = Arc::new(scan_desktop_applications());
    let current_items: Arc<Mutex<Vec<OmnibarItem>>> = Arc::new(Mutex::new(Vec::new()));

    let activate_omnibar_item = {
        let win_act = window.downgrade();
        let loop_act = main_loop.clone();
        let dispatch_ai = dispatch_blazeai_prompt.clone();
        Rc::new(move |item: &OmnibarItem| {
            if let OmnibarItemType::AiQuery(q) = &item.item_type {
                let _ = crate::delight::play_acoustic_feedback("click");
                dispatch_ai(q.clone());
                return;
            }
            execute_omnibar_item(item);
            let _ = std::fs::remove_file(omnibar_pid_file());
            if let Some(win) = win_act.upgrade() {
                win.close();
            }
            loop_act.quit();
        })
    };

    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    {
        let list_weak = list_box.downgrade();
        let win_weak = window.downgrade();
        let items_ref = current_items.clone();
        let loop_action = main_loop.clone();
        let pop_cust = customizer_popover.clone();
        let pop_mod = model_popover.clone();
        let pop_gear = gear_popover.clone();
        let left_ref = left_pane.clone();
        let right_entry = entry.clone();
        let left_entry = blazeai_entry.clone();
        let act_fn = activate_omnibar_item.clone();
        let spk_on_esc = voice_speaker.clone();

        key_controller.connect_key_pressed(move |_ctrl, key, _code, _modifier| {
            if key == gdk::Key::Escape {
                if pop_cust.is_visible() {
                    pop_cust.popdown();
                    return glib::Propagation::Stop;
                }
                if pop_mod.is_visible() {
                    pop_mod.popdown();
                    return glib::Propagation::Stop;
                }
                if pop_gear.is_visible() {
                    pop_gear.popdown();
                    return glib::Propagation::Stop;
                }
                spk_on_esc.stop();
                let _ = std::fs::remove_file(omnibar_pid_file());
                if let Some(win) = win_weak.upgrade() {
                    win.close();
                }
                loop_action.quit();
                return glib::Propagation::Stop;
            }

            let any_popover_open =
                pop_cust.is_visible() || pop_mod.is_visible() || pop_gear.is_visible();
            if any_popover_open {
                return glib::Propagation::Proceed;
            }

            if key == gdk::Key::Tab || key == gdk::Key::ISO_Left_Tab {
                if left_ref.is_visible() {
                    if let Some(win) = win_weak.upgrade() {
                        if is_focus_within(&win, &left_entry) {
                            right_entry.grab_focus();
                        } else {
                            left_entry.grab_focus();
                        }
                        return glib::Propagation::Stop;
                    }
                }
                return glib::Propagation::Proceed;
            }

            // Do not hijack Up/Down/Return when user focus is inside the left BlazeAI pane
            if let Some(win) = win_weak.upgrade() {
                if is_focus_within(&win, &left_ref) {
                    return glib::Propagation::Proceed;
                }
            }

            if key == gdk::Key::Down {
                if let Some(list) = list_weak.upgrade() {
                    let cur = list.selected_row().map(|r| r.index()).unwrap_or(-1);
                    if let Some(next_row) = list.row_at_index(cur + 1) {
                        list.select_row(Some(&next_row));
                    }
                }
                return glib::Propagation::Stop;
            }

            if key == gdk::Key::Up {
                if let Some(list) = list_weak.upgrade() {
                    let cur = list.selected_row().map(|r| r.index()).unwrap_or(0);
                    if cur > 0 {
                        if let Some(prev_row) = list.row_at_index(cur - 1) {
                            list.select_row(Some(&prev_row));
                        }
                    }
                }
                return glib::Propagation::Stop;
            }

            if key == gdk::Key::Return || key == gdk::Key::KP_Enter {
                let selected_idx = list_weak
                    .upgrade()
                    .and_then(|l| l.selected_row().map(|r| r.index()))
                    .unwrap_or(0);

                let item_opt = items_ref
                    .lock()
                    .ok()
                    .and_then(|items| items.get(selected_idx as usize).cloned());

                if let Some(item) = item_opt {
                    act_fn(&item);
                }
                return glib::Propagation::Stop;
            }

            glib::Propagation::Proceed
        });
    }
    window.add_controller(key_controller);

    // Right search entry activate handler
    {
        let items_act = current_items.clone();
        let list_act = list_box.downgrade();
        let act_fn = activate_omnibar_item.clone();
        entry.connect_activate(move |_| {
            let selected_idx = list_act
                .upgrade()
                .and_then(|l| l.selected_row().map(|r| r.index()))
                .unwrap_or(0);

            let item_opt = items_act
                .lock()
                .ok()
                .and_then(|items| items.get(selected_idx as usize).cloned());

            if let Some(item) = item_opt {
                act_fn(&item);
            }
        });
    }

    // ListBox row click handler
    {
        let items_row = current_items.clone();
        let act_fn = activate_omnibar_item.clone();
        list_box.connect_row_activated(move |_list, row| {
            let idx = row.index();
            let item_opt = items_row
                .lock()
                .ok()
                .and_then(|items| items.get(idx as usize).cloned());

            if let Some(item) = item_opt {
                act_fn(&item);
            }
        });
    }

    let list_clone = list_box.clone();
    let apps_clone = cached_apps.clone();
    let items_store = current_items.clone();
    let cloud_search = fluid_cloud.clone();

    let update_results = move |query: &str| {
        while let Some(child) = list_clone.first_child() {
            list_clone.remove(&child);
        }

        let new_items = generate_omnibar_items(query, &apps_clone);
        let count = new_items.len();
        if query.trim().is_empty() {
            cloud_search.pulse(&format!("{} öneri", count), FluidCloudTone::Info, false);
        } else {
            cloud_search.pulse(
                &format!("{} eşleşme", count),
                if count > 0 {
                    FluidCloudTone::Success
                } else {
                    FluidCloudTone::Warning
                },
                false,
            );
        }

        if let Ok(mut lock) = items_store.lock() {
            *lock = new_items.clone();
        }

        for (idx, item) in new_items.iter().enumerate() {
            let row = ListBoxRow::new();
            row.add_css_class("result-row");

            let row_box = GtkBox::new(Orientation::Horizontal, 12);
            row_box.set_hexpand(true);
            row_box.set_valign(gtk4::Align::Center);

            let icon_slot = GtkBox::new(Orientation::Horizontal, 0);
            icon_slot.add_css_class("result-icon-slot");
            icon_slot.set_halign(gtk4::Align::Center);
            icon_slot.set_valign(gtk4::Align::Center);
            let icon = resolve_omnibar_item_icon(item);
            icon_slot.append(&icon);
            row_box.append(&icon_slot);

            let text_box = GtkBox::new(Orientation::Vertical, 2);
            text_box.set_hexpand(true);

            let title_lbl = Label::new(Some(&item.title));
            title_lbl.add_css_class("result-title");
            title_lbl.set_halign(gtk4::Align::Start);
            title_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            text_box.append(&title_lbl);

            let subtitle_lbl = Label::new(Some(&item.subtitle));
            subtitle_lbl.add_css_class("result-subtitle");
            subtitle_lbl.set_halign(gtk4::Align::Start);
            subtitle_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            text_box.append(&subtitle_lbl);

            row_box.append(&text_box);

            let badge_lbl = Label::new(Some(&item.badge));
            badge_lbl.add_css_class("result-badge");
            badge_lbl.set_valign(gtk4::Align::Center);
            row_box.append(&badge_lbl);

            row.set_child(Some(&row_box));
            list_clone.append(&row);

            if idx == 0 {
                list_clone.select_row(Some(&row));
            }
        }
    };

    update_results("");

    let update_box = update_results.clone();
    entry.connect_changed(move |e| {
        update_box(&e.text());
    });

    window.present();
    entry.grab_focus();
}

fn resolve_omnibar_item_icon(item: &OmnibarItem) -> Image {
    match &item.item_type {
        OmnibarItemType::App(_) => resolve_app_icon(item.icon_name.as_deref(), 20),
        OmnibarItemType::Calculation(_, _) | OmnibarItemType::Conversion(_, _) => {
            build_symbolic_icon(
                &["accessories-calculator-symbolic", "system-run-symbolic"],
                "result-symbolic-icon",
            )
        }
        OmnibarItemType::Clipboard(_, _) => {
            build_symbolic_icon(&["edit-paste-symbolic"], "result-symbolic-icon")
        }
        OmnibarItemType::ProcessKill(_, _) => build_symbolic_icon(
            &["process-stop-symbolic", "media-playback-stop-symbolic"],
            "result-symbolic-icon",
        ),
        OmnibarItemType::WebSearch(_, _) => build_symbolic_icon(
            &[
                "web-browser-symbolic",
                "network-server-symbolic",
                "system-search-symbolic",
            ],
            "result-symbolic-icon",
        ),
        OmnibarItemType::AiQuery(_) => build_symbolic_icon(
            &["system-help-symbolic", "dialog-information-symbolic"],
            "result-symbolic-icon",
        ),
        OmnibarItemType::SystemAction(_, _, _) => {
            let icon_name = item
                .icon_name
                .as_deref()
                .unwrap_or("application-x-executable-symbolic");
            build_symbolic_icon(
                &[
                    icon_name,
                    "emblem-system-symbolic",
                    "application-x-executable-symbolic",
                ],
                "result-symbolic-icon",
            )
        }
    }
}

fn resolve_app_icon(icon_opt: Option<&str>, size: i32) -> Image {
    if let Some(icon_str) = icon_opt {
        let mut candidates = Vec::new();
        if icon_str.starts_with('/') {
            candidates.push(std::path::PathBuf::from(icon_str));
        } else {
            for base in [
                "/usr/share/icons/hicolor/128x128/apps",
                "/usr/share/icons/hicolor/256x256/apps",
                "/usr/share/icons/hicolor/64x64/apps",
                "/usr/share/icons/hicolor/48x48/apps",
                "/usr/share/pixmaps",
            ] {
                candidates.push(std::path::Path::new(base).join(format!("{}.png", icon_str)));
            }
        }
        for cand in candidates {
            if cand.exists() {
                if let Ok(bytes) = std::fs::read(&cand) {
                    let gbytes = glib::Bytes::from(&bytes);
                    if let Ok(texture) = gdk::Texture::from_bytes(&gbytes) {
                        let img = Image::from_paintable(Some(&texture));
                        img.set_pixel_size(size);
                        img.set_size_request(size, size);
                        img.set_halign(gtk4::Align::Center);
                        img.set_valign(gtk4::Align::Center);
                        return img;
                    }
                }
            }
        }
        if let Some(display) = gdk::Display::default() {
            let theme = IconTheme::for_display(&display);
            if theme.has_icon(icon_str) {
                let img = Image::from_icon_name(icon_str);
                img.set_pixel_size(size);
                img.set_halign(gtk4::Align::Center);
                img.set_valign(gtk4::Align::Center);
                return img;
            }
        }
    }
    build_symbolic_icon(&["application-x-executable-symbolic"], "result-symbolic-icon")
}

fn execute_omnibar_item(item: &OmnibarItem) {
    let _ = crate::delight::play_acoustic_feedback("click");
    match &item.item_type {
        OmnibarItemType::App(app) => {
            println!("Launching application: {} ({})", app.name, app.exec);
            let mut parts = app.exec.split_whitespace();
            if let Some(cmd) = parts.next() {
                let args: Vec<&str> = parts.filter(|a| !a.starts_with('%')).collect();
                let _ = Command::new(cmd).args(&args).spawn();
            }
        }
        OmnibarItemType::Calculation(_, val) => {
            let res_str = format!("{}", val);
            println!("Calculation copied: {}", res_str);
            let _ = Command::new("wl-copy").arg(&res_str).spawn();
            let _ = Command::new("notify-send")
                .args(["-a", "Solar Omnibar", "Sonuç panoya kopyalandı", &res_str])
                .spawn();
        }
        OmnibarItemType::Conversion(_, val) => {
            println!("Conversion copied: {}", val);
            let _ = Command::new("wl-copy").arg(val).spawn();
            let _ = Command::new("notify-send")
                .args(["-a", "Solar Omnibar", "Çeviri panoya kopyalandı", val])
                .spawn();
        }
        OmnibarItemType::Clipboard(raw_entry, text) => {
            println!("Copying to clipboard: {}", text);
            let is_cliphist = raw_entry
                .split('\t')
                .next()
                .map(|s| s.chars().all(|c| c.is_ascii_digit()))
                .unwrap_or(false);
            let mut copied = false;
            if is_cliphist {
                if let Ok(mut child) = Command::new("cliphist")
                    .arg("decode")
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .spawn()
                {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(raw_entry.as_bytes());
                    }
                    if let Ok(output) = child.wait_with_output() {
                        if output.status.success() && !output.stdout.is_empty() {
                            if let Ok(mut copy_proc) = Command::new("wl-copy")
                                .stdin(std::process::Stdio::piped())
                                .spawn()
                            {
                                if let Some(mut cp_in) = copy_proc.stdin.take() {
                                    let _ = cp_in.write_all(&output.stdout);
                                }
                                let _ = copy_proc.wait();
                                copied = true;
                            }
                        }
                    }
                }
            }

            if !copied {
                if let Ok(mut copy_proc) = Command::new("wl-copy")
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                {
                    use std::io::Write;
                    if let Some(mut cp_in) = copy_proc.stdin.take() {
                        let _ = cp_in.write_all(text.as_bytes());
                    }
                    let _ = copy_proc.wait();
                }
            }

            let preview_short = truncate_chars(text, 50);
            let _ = Command::new("notify-send")
                .args(["-a", "Solar Omnibar", "Panoya kopyalandı", &preview_short])
                .spawn();
        }
        OmnibarItemType::ProcessKill(pid, name) => {
            println!("Killing process: {} (PID: {})", name, pid);
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            let _ = Command::new("notify-send")
                .args([
                    "-a",
                    "Solar Omnibar",
                    "Süreç sonlandırıldı",
                    &format!("{} (PID {})", name, pid),
                ])
                .spawn();
        }
        OmnibarItemType::SystemAction(_, _, cmd) => {
            println!("Executing system action: {}", cmd);
            let cmd_str = cmd.clone();
            if Command::new("setsid")
                .args(["-f", "sh", "-c", &cmd_str])
                .spawn()
                .is_err()
            {
                let _ = Command::new("sh").args(["-c", &cmd_str]).spawn();
            }
        }
        OmnibarItemType::WebSearch(query, url) => {
            println!("Opening web search for '{}': {}", query, url);
            let _ = Command::new("xdg-open").arg(url).spawn();
        }
        OmnibarItemType::AiQuery(query) => {
            let url = build_web_search_url("DuckDuckGo", query);
            let _ = Command::new("xdg-open").arg(&url).spawn();
        }
    }
}

fn format_category_badge(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "game" => "Oyun".to_string(),
        "network" => "Ağ".to_string(),
        "utility" => "Araç".to_string(),
        "system" => "Sistem".to_string(),
        "development" => "Geliştirme".to_string(),
        "graphics" => "Grafik".to_string(),
        "audiovideo" | "audio" | "video" => "Medya".to_string(),
        "office" => "Ofis".to_string(),
        "settings" => "Ayarlar".to_string(),
        "education" | "science" => "Eğitim".to_string(),
        "" => "Uygulama".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => "Uygulama".to_string(),
            }
        }
    }
}

fn scan_clipboard_items(filter: &str, limit: usize) -> Vec<OmnibarItem> {
    let mut items = Vec::new();
    let q_lower = filter.trim().to_lowercase();

    if let Ok(out) = Command::new("cliphist").arg("list").output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let parts: Vec<&str> = line.splitn(2, '\t').collect();
                let (id_str, preview) = if parts.len() == 2 {
                    (parts[0], parts[1])
                } else {
                    ("", line)
                };

                if q_lower.is_empty() || preview.to_lowercase().contains(&q_lower) {
                    let title = truncate_chars(preview, 65);
                    let subtitle = if !id_str.is_empty() {
                        format!("Pano kaydı #{}", id_str)
                    } else {
                        "Pano kaydı".to_string()
                    };

                    items.push(OmnibarItem {
                        title,
                        subtitle,
                        badge: "Pano".to_string(),
                        icon_name: Some("edit-paste-symbolic".to_string()),
                        item_type: OmnibarItemType::Clipboard(
                            line.to_string(),
                            preview.to_string(),
                        ),
                    });

                    if items.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    if items.is_empty() {
        if let Ok(out) = Command::new("wl-paste").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty()
                    && (q_lower.is_empty() || text.to_lowercase().contains(&q_lower))
                {
                    let title = truncate_chars(&text, 65);
                    items.push(OmnibarItem {
                        title,
                        subtitle: "Mevcut pano içeriği".to_string(),
                        badge: "Pano".to_string(),
                        icon_name: Some("edit-paste-symbolic".to_string()),
                        item_type: OmnibarItemType::Clipboard(text.clone(), text),
                    });
                }
            }
        }
    }

    items
}

fn generate_omnibar_items(raw_query: &str, apps: &[AppInfo]) -> Vec<OmnibarItem> {
    let cfg = solar_common::SolarConfig::load();
    let secrets = BlazeAiSecrets::load();
    generate_omnibar_items_with_cfg(raw_query, apps, &cfg, &secrets)
}

fn generate_omnibar_items_with_cfg(
    raw_query: &str,
    apps: &[AppInfo],
    cfg: &SolarConfig,
    secrets: &BlazeAiSecrets,
) -> Vec<OmnibarItem> {
    let query = raw_query.trim();
    let mut items = Vec::new();
    let q_lower = query.to_lowercase();

    // 0. Clipboard history
    let is_clipboard_prefix =
        q_lower.starts_with("cb") || q_lower.starts_with("pano") || q_lower.starts_with("clip");
    if is_clipboard_prefix {
        let skip_chars = if q_lower.starts_with("cb ") {
            3
        } else if q_lower.starts_with("cb") {
            2
        } else if q_lower.starts_with("pano ") || q_lower.starts_with("clip ") {
            5
        } else if q_lower.starts_with("pano") || q_lower.starts_with("clip") {
            4
        } else {
            0
        };
        let filter: String = query.chars().skip(skip_chars).collect();

        let clip_items = scan_clipboard_items(&filter, 20);
        items.extend(clip_items);
    }

    // 1. Math calculation check
    if query.starts_with('=')
        || query.chars().any(|c| "+-*/^%".contains(c)) && query.chars().any(|c| c.is_ascii_digit())
    {
        let clean = query.trim_start_matches('=').trim();
        if let Some(result) = evaluate_simple_math(clean) {
            items.push(OmnibarItem {
                title: format!("= {}", result),
                subtitle: format!("Hesaplama: {}", clean),
                badge: "Hesaplama".to_string(),
                icon_name: Some("accessories-calculator-symbolic".to_string()),
                item_type: OmnibarItemType::Calculation(clean.to_string(), result),
            });
        }
    }

    // 2. Unit, Currency & Crypto converter check
    if query.contains(" to ")
        || query.contains(" in ")
        || query.contains("->")
        || query.chars().any(|c| "$€₺£".contains(c))
        || query.split_whitespace().count() == 3
    {
        if let Some((title, res_str)) = evaluate_units(query) {
            items.push(OmnibarItem {
                title,
                subtitle: format!("Çevrimdışı yaklaşık çeviri: {}", query),
                badge: "Çeviri".to_string(),
                icon_name: Some("accessories-calculator-symbolic".to_string()),
                item_type: OmnibarItemType::Conversion(query.to_string(), res_str),
            });
        }
    }

    // 3. Process killer check
    if query.starts_with("kill ") || query.starts_with("/kill ") {
        let proc_query = query
            .trim_start_matches("/kill ")
            .trim_start_matches("kill ")
            .trim()
            .to_lowercase();
        let running_procs = scan_running_processes(&proc_query);
        for (pid, name) in running_procs.into_iter().take(5) {
            items.push(OmnibarItem {
                title: format!("Süreci sonlandır: {} (PID {})", name, pid),
                subtitle: "Seçilen süreci kapat".to_string(),
                badge: "Süreç".to_string(),
                icon_name: Some("process-stop-symbolic".to_string()),
                item_type: OmnibarItemType::ProcessKill(pid, name),
            });
        }
    }

    // 4. System quick actions
    let toggle_shell_cmd = match cfg.shell.engine {
        solar_common::ShellEngine::Noctalia => "solar-shell switch caelestia",
        solar_common::ShellEngine::Caelestia
        | solar_common::ShellEngine::End4Pc
        | solar_common::ShellEngine::We10x => "solar-shell switch noctalia",
    };
    let toggle_shell_desc = match cfg.shell.engine {
        solar_common::ShellEngine::Noctalia => "Etkin: Noctalia · Caelestia kabuğuna geç",
        solar_common::ShellEngine::Caelestia => "Etkin: Caelestia · Noctalia kabuğuna geç",
        solar_common::ShellEngine::End4Pc => "Etkin: end4-pC · Noctalia kabuğuna geç",
        solar_common::ShellEngine::We10x => "Etkin: We10X · Noctalia kabuğuna geç",
    };

    let sys_commands = [
        ("game", "Blaze GameZone", "Tam ekran oyun kabuğunu aç", "solar-shell gamezone", "input-gaming-symbolic"),
        ("store", "FireHub yazılım mağazası", "Portadora ve Flathub uygulamalarını aç", "firehub", "system-software-install-symbolic"),
        ("welcome", "SolarUI kısayol rehberi", "Masaüstü ve pencere kısayollarını aç", "solar-shell welcome", "preferences-desktop-keyboard-shortcuts-symbolic"),
        ("oobe", "BlazeOS kurulum asistanı", "Dil, bölge, kullanıcı ve masaüstü ilk kurulumu", "/usr/bin/blaze-setup", "system-software-install-symbolic"),
        ("setup", "İlk kurulum asistanı", "Sistem ayarlarını ve kullanıcı tercihlerini yapılandır", "/usr/bin/blaze-setup", "system-software-install-symbolic"),
        ("berp", "Blaze Acil Kurtarma (BERP)", "Kurtarma ve anlık görüntü konsolunu aç", "ptyxis -- blaze-recovery", "dialog-warning-symbolic"),
        ("kurtarma", "Acil kurtarma ve geri yükleme", "Sistem geri yükleme ve donanım denetim konsolu", "ptyxis -- blaze-recovery", "dialog-warning-symbolic"),
        ("akrep", "Akrep güvenlik merkezi", "SELinux, güvenlik duvarı, Flatpak ve cihaz güvenliği", "akrep-ui", "security-high-symbolic"),
        ("security", "Akrep güvenlik durumu", "Sistem güvenlik duruşu ve bütünlük görünümü", "akrep-ui", "security-high-symbolic"),
        ("konami", "SolarEvolution retro modu", "CRT ve terminal görünümünü aç", "solar-shell konami", "input-gaming-symbolic"),
        ("season", "Mevsimsel tema", "Gündönümü ve ekinoks renk tonunu göster", "solar-shell season", "preferences-desktop-wallpaper-symbolic"),
        ("theme", "Masaüstü kabuğunu değiştir", toggle_shell_desc, toggle_shell_cmd, "preferences-desktop-appearance-symbolic"),
        ("noctalia", "Noctalia kabuğuna geç", "Noctalia masaüstü kabuğunu etkinleştir", "solar-shell switch noctalia", "preferences-desktop-appearance-symbolic"),
        ("caelestia", "Caelestia kabuğuna geç", "Caelestia masaüstü kabuğunu etkinleştir", "solar-shell switch caelestia", "preferences-desktop-appearance-symbolic"),
        ("end4", "end4-pC kabuğuna geç", "end4-pC masaüstü kabuğunu etkinleştir", "solar-shell switch end4-pc", "preferences-desktop-appearance-symbolic"),
        ("we10x", "We10X kabuğuna geç", "Klasik başlat menüsü, görev çubuğu ve masaüstü simgeleri", "solar-shell switch we10x", "preferences-desktop-appearance-symbolic"),
        ("launcher", "Kabuk uygulama menüsünü aç", "Etkin kabuğun yerel uygulama başlatıcısını aç", "solar-shell route launcher", "view-app-grid-symbolic"),
        ("settings", "SolarUI masaüstü ayarları", "Görev çubuğu, tema, ekran ve sistem tercihleri", "solar-shell settings", "preferences-system-symbolic"),
        ("ekran", "Ekran ve ölçekleme ayarları", "Çoklu monitör çözünürlüğü ve arayüz ölçeği", "solar-shell settings", "video-display-symbolic"),
        ("pano", "Pano geçmişi", "Kopyalanan metinleri filtrele (cb)", "fuzzel-clipboard", "edit-paste-symbolic"),
        ("lock", "Ekranı kilitle", "Oturumu kilitle", "solar-lock", "system-lock-screen-symbolic"),
        ("reboot", "Sistemi yeniden başlat", "Bilgisayarı yeniden başlat", "systemctl reboot", "system-reboot-symbolic"),
        ("power", "Bilgisayarı kapat", "Sistemi kapat", "systemctl poweroff", "system-shutdown-symbolic"),
    ];

    for (cmd_id, title, desc, action, icon) in sys_commands {
        if query.is_empty() || cmd_id.contains(&q_lower) || title.to_lowercase().contains(&q_lower) {
            items.push(OmnibarItem {
                title: title.to_string(),
                subtitle: desc.to_string(),
                badge: "Sistem".to_string(),
                icon_name: Some(icon.to_string()),
                item_type: OmnibarItemType::SystemAction(
                    cmd_id.to_string(),
                    title.to_string(),
                    action.to_string(),
                ),
            });
        }
    }

    // 5. Desktop Application Search (Fuzzy)
    let q_lower = query.to_lowercase();
    for app in apps {
        let name_match = app.name.to_lowercase().contains(&q_lower);
        let comment_match = app
            .comment
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(&q_lower);
        let exec_match = app.exec.to_lowercase().contains(&q_lower);

        if query.is_empty() || name_match || comment_match || exec_match {
            let cat = app
                .categories
                .first()
                .map(|c| format_category_badge(c))
                .unwrap_or_else(|| "Uygulama".to_string());
            items.push(OmnibarItem {
                title: app.name.clone(),
                subtitle: app.comment.clone().unwrap_or_else(|| app.exec.clone()),
                badge: cat,
                icon_name: app
                    .icon
                    .clone()
                    .or_else(|| Some("application-x-executable-symbolic".to_string())),
                item_type: OmnibarItemType::App(app.clone()),
            });
        }

        if items.len() >= 30 {
            break;
        }
    }

    // 5.5. Clipboard matches without prefix
    if !is_clipboard_prefix && query.len() >= 3 {
        let clip_matches = scan_clipboard_items(query, 3);
        items.extend(clip_matches);
    }

    // 6. Web and BlazeAI Integration
    if !query.is_empty() {
        if cfg.omnibar.enable_web_search {
            let search_url = build_web_search_url(&cfg.omnibar.search_engine, query);
            items.push(OmnibarItem {
                title: format!("Web'de ara ({}): {}", cfg.omnibar.search_engine, query),
                subtitle: format!("{} ile varsayılan tarayıcıda aç", cfg.omnibar.search_engine),
                badge: "Web".to_string(),
                icon_name: Some("web-browser-symbolic".to_string()),
                item_type: OmnibarItemType::WebSearch(query.to_string(), search_url),
            });
        }

        if secrets.has_key_for(cfg.omnibar.ai_provider) {
            items.push(OmnibarItem {
                title: format!("BlazeAI'a sor: {}", query),
                subtitle: format!(
                    "{} ile sol panelde yanıtla",
                    cfg.omnibar.effective_companion_name()
                ),
                badge: "BlazeAI".to_string(),
                icon_name: Some("system-help-symbolic".to_string()),
                item_type: OmnibarItemType::AiQuery(query.to_string()),
            });
        }
    }

    items
}

fn evaluate_simple_math(expr: &str) -> Option<f64> {
    let clean = expr.replace(' ', "");
    if clean.is_empty() {
        return None;
    }

    if clean.starts_with("sqrt(") && clean.ends_with(')') {
        let inner = &clean[5..clean.len() - 1];
        let val = inner.parse::<f64>().ok().or_else(|| evaluate_simple_math(inner))?;
        let res = val.sqrt();
        return if res.is_finite() { Some(res) } else { None };
    }

    let bytes = clean.as_bytes();
    for ops in [&['+', '-'][..], &['*', '/', '%'][..], &['^'][..]] {
        let mut paren_depth: i32 = 0;
        for pos in (0..bytes.len()).rev() {
            let b = bytes[pos];
            if b == b')' {
                paren_depth += 1;
                continue;
            }
            if b == b'(' {
                paren_depth -= 1;
                continue;
            }
            if paren_depth != 0 {
                continue;
            }
            let ch = b as char;
            if !ops.contains(&ch) {
                continue;
            }
            if (ch == '+' || ch == '-')
                && (pos == 0 || matches!(bytes[pos - 1], b'+' | b'-' | b'*' | b'/' | b'%' | b'^' | b'('))
            {
                continue;
            }
            if pos == 0 || pos + 1 >= bytes.len() {
                continue;
            }
            let left_str = &clean[..pos];
            let right_str = &clean[pos + 1..];
            let left = left_str.parse::<f64>().ok().or_else(|| evaluate_simple_math(left_str))?;
            let right = right_str.parse::<f64>().ok().or_else(|| evaluate_simple_math(right_str))?;

            let res = match ch {
                '+' => left + right,
                '-' => left - right,
                '*' => left * right,
                '/' => if right != 0.0 { left / right } else { return None; },
                '^' => left.powf(right),
                '%' => if right != 0.0 { left % right } else { return None; },
                _ => return None,
            };
            return if res.is_finite() { Some(res) } else { None };
        }
    }

    clean.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn evaluate_units(query: &str) -> Option<(String, String)> {
    let mut q = query.to_lowercase();
    q = q.replace('$', " usd ");
    q = q.replace('€', " eur ");
    q = q.replace('₺', " try ");
    q = q.replace('£', " gbp ");

    let parts: Vec<&str> = q.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let val = parts[0].parse::<f64>().ok()?;
    let from_unit = parts[1];
    let to_unit = if parts.len() >= 4
        && (parts[2] == "to" || parts[2] == "in" || parts[2] == "->" || parts[2] == "=")
    {
        parts[3]
    } else if parts.len() == 3 {
        parts[2]
    } else {
        parts[parts.len() - 1]
    };

    if from_unit == "usd" && to_unit == "try" {
        let res = val * 38.50;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }
    if from_unit == "try" && to_unit == "usd" {
        let res = val / 38.50;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if from_unit == "eur" && to_unit == "try" {
        let res = val * 42.20;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }
    if from_unit == "try" && to_unit == "eur" {
        let res = val / 42.20;
        return Some((format!("{:.2} EUR", res), format!("{:.2}", res)));
    }
    if from_unit == "gbp" && to_unit == "try" {
        let res = val * 49.80;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }
    if from_unit == "try" && to_unit == "gbp" {
        let res = val / 49.80;
        return Some((format!("{:.2} GBP", res), format!("{:.2}", res)));
    }
    if from_unit == "eur" && to_unit == "usd" {
        let res = val * 1.096;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if from_unit == "usd" && to_unit == "eur" {
        let res = val / 1.096;
        return Some((format!("{:.2} EUR", res), format!("{:.2}", res)));
    }

    if (from_unit == "btc" || from_unit == "bitcoin") && to_unit == "usd" {
        let res = val * 95000.0;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if from_unit == "usd" && (to_unit == "btc" || to_unit == "bitcoin") {
        let res = val / 95000.0;
        return Some((format!("{:.6} BTC", res), format!("{:.6}", res)));
    }
    if (from_unit == "btc" || from_unit == "bitcoin") && to_unit == "try" {
        let res = val * 95000.0 * 38.50;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }
    if (from_unit == "eth" || from_unit == "ethereum") && to_unit == "usd" {
        let res = val * 2750.0;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if from_unit == "usd" && (to_unit == "eth" || to_unit == "ethereum") {
        let res = val / 2750.0;
        return Some((format!("{:.5} ETH", res), format!("{:.5}", res)));
    }
    if (from_unit == "eth" || from_unit == "ethereum") && to_unit == "try" {
        let res = val * 2750.0 * 38.50;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }
    if (from_unit == "sol" || from_unit == "solana") && to_unit == "usd" {
        let res = val * 190.0;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if (from_unit == "sol" || from_unit == "solana") && to_unit == "try" {
        let res = val * 190.0 * 38.50;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }

    if (from_unit == "gold" || from_unit == "xau" || from_unit == "ons") && to_unit == "usd" {
        let res = val * 2700.0;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if (from_unit == "gold" || from_unit == "altin" || from_unit == "gram") && to_unit == "try" {
        let res = val * 3350.0;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }

    if from_unit == "km" && (to_unit == "mi" || to_unit == "miles") {
        let res = val * 0.621371;
        return Some((format!("{:.2} Mil", res), format!("{:.2}", res)));
    }
    if (from_unit == "mi" || from_unit == "miles") && to_unit == "km" {
        let res = val * 1.60934;
        return Some((format!("{:.2} km", res), format!("{:.2}", res)));
    }

    if from_unit == "c" && to_unit == "f" {
        let res = (val * 9.0 / 5.0) + 32.0;
        return Some((format!("{:.1} °F", res), format!("{:.1}", res)));
    }
    if from_unit == "f" && to_unit == "c" {
        let res = (val - 32.0) * 5.0 / 9.0;
        return Some((format!("{:.1} °C", res), format!("{:.1}", res)));
    }

    if from_unit == "gb" && to_unit == "mb" {
        let res = val * 1024.0;
        return Some((format!("{:.0} MB", res), format!("{:.0}", res)));
    }
    if from_unit == "mb" && to_unit == "gb" {
        let res = val / 1024.0;
        return Some((format!("{:.2} GB", res), format!("{:.2}", res)));
    }
    if from_unit == "tb" && to_unit == "gb" {
        let res = val * 1024.0;
        return Some((format!("{:.0} GB", res), format!("{:.0}", res)));
    }

    None
}

fn scan_running_processes(filter: &str) -> Vec<(u32, String)> {
    let mut procs = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(pid_str) = path.file_name().and_then(|n| n.to_str()) {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    let comm_path = path.join("comm");
                    if let Ok(comm) = std::fs::read_to_string(comm_path) {
                        let name = comm.trim().to_string();
                        if filter.is_empty() || name.to_lowercase().contains(filter) {
                            procs.push((pid, name));
                        }
                    }
                }
            }
        }
    }
    procs.sort_by(|a, b| a.1.cmp(&b.1));
    procs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_simple_math() {
        assert_eq!(evaluate_simple_math("25 * 1024"), Some(25600.0));
        assert_eq!(evaluate_simple_math("sqrt(144)"), Some(12.0));
        assert_eq!(evaluate_simple_math("100 / 4 + 15"), Some(40.0));
        assert_eq!(evaluate_simple_math("2 ^ 8"), Some(256.0));
        assert_eq!(evaluate_simple_math("10 * 3 % 2"), Some(0.0));
        assert_eq!(evaluate_simple_math("sqrt(-4)"), None);
        assert_eq!(evaluate_simple_math("100 % 0"), None);
        assert_eq!(evaluate_simple_math("100 / 0"), None);
    }

    #[test]
    fn test_evaluate_units() {
        let (title, res) = evaluate_units("100 usd in try").unwrap();
        assert!(title.contains("TRY"));
        assert_eq!(res, "3850.00");

        let (title_btc, res_btc) = evaluate_units("1 btc to usd").unwrap();
        assert!(title_btc.contains("USD"));
        assert_eq!(res_btc, "95000.00");

        let (title_eth, res_eth) = evaluate_units("2 eth to usd").unwrap();
        assert!(title_eth.contains("USD"));
        assert_eq!(res_eth, "5500.00");

        let (title_km, _) = evaluate_units("50 km in miles").unwrap();
        assert!(title_km.contains("Mil"));

        let (title_gb, _) = evaluate_units("16 gb in mb").unwrap();
        assert!(title_gb.contains("16384 MB"));
    }

    #[test]
    fn test_truncate_chars_utf8_turkish_safety() {
        let turkish =
            "Çağdaş güneş ışığı altında özgürce çalışan işlemci çekirdeği güç tüketimi ölçümü";
        for max_len in [0, 1, 2, 5, 10, 15, 25, 50, 65, 100] {
            let out = truncate_chars(turkish, max_len);
            assert!(out.chars().count() <= max_len + 3);
        }
        let exact = truncate_chars("ğüşıöçĞÜŞİÖÇ", 6);
        assert_eq!(exact, "ğüşıöç...");

        let items = generate_omnibar_items("pano ğüşıöç", &[]);
        let _ = items.len();
    }

    #[test]
    fn test_omnibar_window_dimensions_and_akrep_action_preserved() {
        assert_eq!(window_dimensions_for_panel_state(true), (1040, 520));
        assert_eq!(window_dimensions_for_panel_state(false), (680, 492));

        let mut cfg = SolarConfig::default();
        let secrets = BlazeAiSecrets::default();
        let akrep_items = generate_omnibar_items_with_cfg("akrep", &[], &cfg, &secrets);
        assert!(akrep_items.iter().any(|it| matches!(
            &it.item_type,
            OmnibarItemType::SystemAction(_, _, cmd) if cmd == "akrep-ui"
        )));

        // Without API key on Gemini, web search shows up, model chip prompts for key, and AiQuery does not
        assert_eq!(
            format_model_chip_for_state(&cfg.omnibar, &secrets),
            "API anahtarı ekle"
        );
        let mut with_key = BlazeAiSecrets::default();
        with_key.set_key_for(BlazeAiProvider::Gemini, "AIza-test-key");
        assert_eq!(
            format_model_chip_for_state(&cfg.omnibar, &with_key),
            "gemini-3.8-flash"
        );
        let no_key_items = generate_omnibar_items_with_cfg("niri klavye", &[], &cfg, &secrets);
        assert!(no_key_items
            .iter()
            .any(|it| matches!(&it.item_type, OmnibarItemType::WebSearch(_, _))));
        assert!(!no_key_items
            .iter()
            .any(|it| matches!(&it.item_type, OmnibarItemType::AiQuery(_))));

        // With Ollama (no API key required) or configured key, AiQuery appears with companion name
        cfg.omnibar.ai_provider = BlazeAiProvider::Ollama;
        cfg.omnibar.ai_model = "llama3.2".to_string();
        assert_eq!(format_model_chip_label(&cfg.omnibar), "Ollama · llama3.2");
        let ollama_items = generate_omnibar_items_with_cfg("niri klavye", &[], &cfg, &secrets);
        assert!(ollama_items.iter().any(|it| matches!(
            &it.item_type,
            OmnibarItemType::AiQuery(q) if q == "niri klavye"
        )));
    }
}
