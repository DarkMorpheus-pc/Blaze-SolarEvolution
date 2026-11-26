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
    EventControllerMotion, IconTheme, Image, Label, ListBox, ListBoxRow, Orientation,
    PasswordEntry, Popover, ScrolledWindow, Separator, StringList, Window,
};
use solar_common::{BlazeAiProvider, CompanionExpression, OmnibarConfig, SolarConfig};
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::apps::{scan_desktop_applications, AppInfo};
use crate::blazeai::{
    build_web_search_url, curated_models_for_provider, execute_suggested_action,
    extract_user_memory_command, migrate_legacy_api_key_if_needed, probe_ollama_models,
    resolve_dispatch_mode, send_chat_completion, AiDispatchMode, BlazeAiSecrets,
    ChatCompletionOutcome, ChatMessage, ChatRole, LocalMemoryStore,
};
use crate::companion_avatar::{
    build_companion_customizer_popover, CompanionAvatarWidget, CompanionCustomizerCallbacks,
};
use crate::living_flow::{apply_living_flow_theme, FluidCloud, FluidCloudTone};

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
        border-color: rgba(224, 87, 60, 0.65);
        outline: 2px solid rgba(224, 87, 60, 0.65);
        outline-offset: 1px;
    }

    .blazeai-entry {
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
    .blazeai-entry:focus-within {
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

    .omnibar-entry:focus,
    .omnibar-entry:focus-within {
        background-color: #232325;
        border-color: rgba(224, 87, 60, 0.65);
        outline: 2px solid rgba(224, 87, 60, 0.65);
        outline-offset: 1px;
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
    let sc = scrolled.downgrade();
    glib::idle_add_local_once(move || {
        if let Some(s) = sc.upgrade() {
            let adj = s.vadjustment();
            adj.set_value((adj.upper() - adj.page_size()).max(0.0));
        }
    });
}

fn rebuild_chat_view(
    chat_box: &GtkBox,
    chat_scroll: &ScrolledWindow,
    store: &LocalMemoryStore,
    cfg: &OmnibarConfig,
    secrets: &BlazeAiSecrets,
    fluid_cloud: &FluidCloud,
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
            cfg.companion_expression.title_tr()
        )));
        title.add_css_class("blazeai-msg-speaker");
        title.set_halign(gtk4::Align::Start);
        card.append(&title);

        let desc = Label::new(Some(cfg.companion_expression.meaning_tr()));
        desc.add_css_class("companion-banner-desc");
        desc.set_wrap(true);
        desc.set_xalign(0.0);
        desc.set_halign(gtk4::Align::Start);
        card.append(&desc);

        let mode_note = if !cfg.ai_provider.requires_api_key() || secrets.has_key_for(cfg.ai_provider) {
            "Yerel hafıza için \"hatırla: not\" yazabilir veya BlazeOS hakkında sorabilirsin"
        } else {
            "API anahtarı girilmemiş; dişli simgesinden ekleyebilir veya doğrudan web araması başlatabilirsin"
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

                let body = Label::new(Some(&msg.content));
                body.add_css_class("blazeai-msg-text");
                body.set_wrap(true);
                body.set_xalign(0.0);
                body.set_halign(gtk4::Align::Start);
                body.set_selectable(true);
                card.append(&body);

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

    let window = Window::builder()
        .title("Solar Omnibar")
        .decorated(false)
        .default_width(init_w)
        .default_height(init_h)
        .resizable(false)
        .build();

    let loop_quit = main_loop.clone();
    window.connect_close_request(move |_| {
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

    let avatar = CompanionAvatarWidget::new(
        initial_cfg.omnibar.companion_shape,
        initial_cfg.omnibar.companion_expression,
        &initial_cfg.omnibar.companion_color,
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

    let companion_arch_lbl = Label::new(Some(initial_cfg.omnibar.companion_expression.title_tr()));
    companion_arch_lbl.add_css_class("companion-archetype-sub");
    companion_arch_lbl.set_hexpand(true);
    companion_arch_lbl.set_halign(gtk4::Align::Start);
    companion_arch_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    sub_model_row.append(&companion_arch_lbl);

    let model_btn = Button::new();
    model_btn.add_css_class("model-pill-btn");
    model_btn.set_tooltip_text(Some("Sağlayıcı ve model seç"));
    let model_btn_box = GtkBox::new(Orientation::Horizontal, 4);
    let model_btn_lbl = Label::new(Some(&format_model_chip_label(&initial_cfg.omnibar)));
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

    // Bottom prompt bar (Entry + Send button, NO microphone icon)
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
    close_btn.connect_clicked(move |_| {
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
        Rc::new(move || {
            let cfg = SolarConfig::load();
            let secrets = BlazeAiSecrets::load();
            model_lbl.set_text(&format_model_chip_label(&cfg.omnibar));
            b_entry.set_placeholder_text(Some(blazeai_placeholder_text(&cfg.omnibar, &secrets)));
            rebuild_chat_view(
                &chat_b,
                &chat_s,
                &st_rc.borrow(),
                &cfg.omnibar,
                &secrets,
                &cloud_c,
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
                    arch_lbl_c.set_text(omnibar_cfg.companion_expression.title_tr());
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
        .max_content_height(430)
        .min_content_width(326)
        .build();
    let gear_box = GtkBox::new(Orientation::Vertical, 10);
    gear_box.add_css_class("blazeai-gear-box");

    let gear_title = Label::new(Some("BlazeAI ve yerel hafıza"));
    gear_title.add_css_class("companion-banner-title");
    gear_title.set_halign(gtk4::Align::Start);
    gear_box.append(&gear_title);

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
    gear_model_entry.set_placeholder_text(Some("gemini-2.5-flash"));
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

    let chk_persist = CheckButton::with_label("Sohbet geçmişini koru");
    let chk_local_mem = CheckButton::with_label("Uzun vadeli yerel hafıza");
    let chk_guide = CheckButton::with_label("Yerleşik BlazeOS rehberini ekle");
    gear_box.append(&chk_persist);
    gear_box.append(&chk_local_mem);
    gear_box.append(&chk_guide);

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

    gear_scroll.set_child(Some(&gear_box));
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
            rebuild_mem();
            sync_flag.set(false);
        })
    };
    sync_gear_fields();

    {
        let sync_flag = syncing_gear.clone();
        let m_entry = gear_model_entry.clone();
        let u_entry = gear_url_entry.clone();
        let k_entry = gear_key_entry.clone();
        let refresh_c = refresh_ui_state.clone();
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
        gear_url_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let txt = e.text().trim().to_string();
            let _ = SolarConfig::update(|c| {
                c.omnibar.ai_custom_base_url = txt;
            });
        });
    }

    {
        let sync_flag = syncing_gear.clone();
        let refresh_c = refresh_ui_state.clone();
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

    // Model Selector Popover
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
    model_list_wrap.set_size_request(260, -1);
    let model_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(360)
        .build();
    model_scroll.set_child(Some(&model_list_wrap));
    model_popover.set_child(Some(&model_scroll));

    let populate_model_popover = {
        let wrap = model_list_wrap.clone();
        let pop = model_popover.clone();
        let refresh_c = refresh_ui_state.clone();
        let sync_g = sync_gear_fields.clone();
        Rc::new(move |extra_ollama: Vec<String>| {
            while let Some(ch) = wrap.first_child() {
                wrap.remove(&ch);
            }
            let cfg = SolarConfig::load();
            let active_p = cfg.omnibar.ai_provider;
            let active_m = cfg.omnibar.effective_ai_model();

            for provider in BlazeAiProvider::ALL {
                let hdr = Label::new(Some(provider.display_name()));
                hdr.add_css_class("companion-section-lbl");
                hdr.set_halign(gtk4::Align::Start);
                hdr.set_margin_top(4);
                wrap.append(&hdr);

                let mut models: Vec<String> = curated_models_for_provider(provider)
                    .iter()
                    .map(|m| (*m).to_string())
                    .collect();
                if provider == BlazeAiProvider::Ollama {
                    for om in &extra_ollama {
                        if !models.contains(om) {
                            models.push(om.clone());
                        }
                    }
                }

                for model_name in models {
                    let is_cur = active_p == provider && active_m == model_name;
                    let btn = Button::new();
                    btn.add_css_class("companion-opt-btn");
                    if is_cur {
                        btn.add_css_class("companion-opt-active");
                    }
                    let row = GtkBox::new(Orientation::Horizontal, 8);
                    let lbl = Label::new(Some(&model_name));
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
                            c.omnibar.ai_provider = provider;
                            c.omnibar.ai_model = m_owned.clone();
                            c.omnibar.ai_custom_base_url = provider.default_base_url().to_string();
                        });
                        sync_in();
                        refresh_in();
                        pop_c.popdown();
                    });
                    wrap.append(&btn);
                }
            }
        })
    };

    {
        let pop = model_popover.clone();
        let pop_fill = populate_model_popover.clone();
        model_btn.connect_clicked(move |_| {
            pop_fill(Vec::new());
            pop.popup();

            // Probe local Ollama models asynchronously without blocking UI
            let cfg = SolarConfig::load();
            let ollama_url = if cfg.omnibar.ai_provider == BlazeAiProvider::Ollama
                && !cfg.omnibar.ai_custom_base_url.trim().is_empty()
            {
                cfg.omnibar.effective_ai_base_url()
            } else {
                BlazeAiProvider::Ollama.default_base_url().to_string()
            };
            let (tx, rx) = async_channel::bounded::<Vec<String>>(1);
            std::thread::spawn(move || {
                let models = probe_ollama_models(&ollama_url).unwrap_or_default();
                let _ = tx.send_blocking(models);
            });
            let pop_fill_async = pop_fill.clone();
            let pop_weak = pop.downgrade();
            glib::spawn_future_local(async move {
                if let Ok(models) = rx.recv().await {
                    if !models.is_empty() && pop_weak.upgrade().is_some_and(|p| p.is_visible()) {
                        pop_fill_async(models);
                    }
                }
            });
        });
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

        Rc::new(move |raw_prompt: String| {
            let prompt = raw_prompt.trim().to_string();
            if prompt.is_empty() || sending_rc.get() {
                return;
            }

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
                    },
                    cfg.omnibar.ai_persist_chat,
                );
                refresh_c();
                cloud_c.pulse(&format!("{} ile açıldı", engine), FluidCloudTone::Info, false);
                return;
            }

            // 3. Append user turn & execute provider request off the GTK main thread
            let (history_snapshot, mem_snapshot) = {
                let mut st = st_rc.borrow_mut();
                let _ = st.append_message(
                    ChatMessage {
                        role: ChatRole::User,
                        content: prompt.clone(),
                        timestamp: chrono::Local::now().to_rfc3339(),
                        speaker_name: "Sen".to_string(),
                        suggested_actions: Vec::new(),
                    },
                    cfg.omnibar.ai_persist_chat,
                );
                (st.messages.clone(), st.memory_facts.clone())
            };

            sending_rc.set(true);
            send_b.set_sensitive(false);
            av.set_waiting(true);
            refresh_c();
            cloud_c.pulse("BlazeAI düşünüyor", FluidCloudTone::Info, false);

            let (tx, rx) = async_channel::bounded::<Result<ChatCompletionOutcome, String>>(1);
            let omnibar_cfg = cfg.omnibar.clone();
            let prompt_clone = prompt.clone();
            std::thread::spawn(move || {
                let res = send_chat_completion(
                    &omnibar_cfg,
                    &secrets,
                    &mem_snapshot,
                    &history_snapshot,
                    &prompt_clone,
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
                            },
                            latest_cfg.omnibar.ai_persist_chat,
                        );
                        av_done.pulse_temporary_expression(CompanionExpression::Confused, 2500);
                        cloud_done.pulse(&format!("{} ile açıldı", engine), FluidCloudTone::Warning, false);
                    }
                    Ok(Ok(ChatCompletionOutcome::Reply {
                        cleaned_text,
                        extracted_facts,
                        suggested_actions,
                        ..
                    })) => {
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
                                    speaker_name: latest_cfg.omnibar.effective_companion_name(),
                                    suggested_actions,
                                },
                                latest_cfg.omnibar.ai_persist_chat,
                            );
                        }
                        rebuild_mem_done();
                        av_done.pulse_temporary_expression(CompanionExpression::Happy, 2500);
                        cloud_done.pulse("Yanıt hazır", FluidCloudTone::Success, false);
                    }
                    Ok(Err(err_msg)) => {
                        let _ = st_done.borrow_mut().append_message(
                            ChatMessage {
                                role: ChatRole::SystemNote,
                                content: format!("Hata: {}", err_msg),
                                timestamp: chrono::Local::now().to_rfc3339(),
                                speaker_name: latest_cfg.omnibar.effective_companion_name(),
                                suggested_actions: Vec::new(),
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

        // Without API key on Gemini, web search shows up while AiQuery does not
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
