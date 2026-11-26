// ==============================================================================
// SolarUI Graphite + Blaze Ember Theme Module (GTK4)
// Shared window chrome, status indicator, form controls, and token stylesheet
// for SolarUI desktop utilities on Blaze SolarEvolution.
// ==============================================================================

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, IconTheme, Image, Label, Orientation, Window, WindowHandle,
};
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FluidCloudTone {
    Info,
    Success,
    Warning,
    Danger,
}

#[derive(Clone)]
pub struct FluidCloud {
    pill_box: GtkBox,
    text_lbl: Label,
    generation: Rc<RefCell<u64>>,
}

impl FluidCloud {
    pub fn new(initial_text: &str) -> Self {
        let pill_box = GtkBox::new(Orientation::Horizontal, 8);
        pill_box.add_css_class("lf-fluid-cloud");
        pill_box.add_css_class("cloud-info");
        pill_box.set_valign(gtk4::Align::Center);

        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("lf-cloud-dot");
        dot.set_size_request(8, 8);
        dot.set_halign(gtk4::Align::Center);
        dot.set_valign(gtk4::Align::Center);
        dot.set_hexpand(false);
        dot.set_vexpand(false);
        pill_box.append(&dot);

        let text_lbl = Label::new(Some(initial_text));
        text_lbl.add_css_class("lf-cloud-text");
        text_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        text_lbl.set_max_width_chars(38);
        pill_box.append(&text_lbl);

        Self {
            pill_box,
            text_lbl,
            generation: Rc::new(RefCell::new(0)),
        }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.pill_box
    }

    /// Updates the topbar status indicator message and semantic tone,
    /// with a brief surface highlight and optional desktop notification.
    pub fn pulse(&self, message: &str, tone: FluidCloudTone, send_osd: bool) {
        let _ = crate::delight::play_acoustic_feedback("click");

        for cls in ["cloud-info", "cloud-ok", "cloud-warn", "cloud-danger", "cloud-pulse"] {
            self.pill_box.remove_css_class(cls);
        }

        let tone_cls = match tone {
            FluidCloudTone::Info => "cloud-info",
            FluidCloudTone::Success => "cloud-ok",
            FluidCloudTone::Warning => "cloud-warn",
            FluidCloudTone::Danger => "cloud-danger",
        };

        self.text_lbl.set_text(message);
        self.pill_box.add_css_class(tone_cls);
        self.pill_box.add_css_class("cloud-pulse");

        let next_gen = {
            let mut g = self.generation.borrow_mut();
            *g += 1;
            *g
        };

        let pill_ref = self.pill_box.clone();
        let gen_ref = self.generation.clone();
        glib::timeout_add_local_once(std::time::Duration::from_millis(320), move || {
            if *gen_ref.borrow() == next_gen {
                pill_ref.remove_css_class("cloud-pulse");
            }
        });

        if send_osd {
            let msg_owned = message.to_string();
            std::thread::spawn(move || {
                let _ = Command::new("notify-send")
                    .args([
                        "-a",
                        "SolarUI",
                        "-i",
                        "solar-settings",
                        "SolarUI",
                        &msg_owned,
                    ])
                    .spawn();
            });
        }
    }
}

pub const LIVING_FLOW_CORE_CSS: &str = r#"
    window,
    window.background,
    window.csd {
        background-color: #141415;
        color: #ededed;
    }

    button:focus-visible,
    row:focus-visible,
    checkbutton:focus-visible,
    radiobutton:focus-visible,
    switch:focus-visible,
    scale:focus-visible,
    modelbutton:focus-visible {
        outline: 2px solid rgba(224, 87, 60, 0.65);
        outline-offset: 2px;
    }

    entry > text:focus-visible,
    passwordentry > text:focus-visible,
    spinbutton > text:focus-visible {
        outline: none;
    }

    headerbar {
        background-color: #141415;
        background-image: none;
        color: #ededed;
        border-bottom: 1px solid rgba(237, 237, 237, 0.08);
        box-shadow: none;
    }

    headerbar title,
    headerbar .title {
        color: #ededed;
        font-size: 14px;
        font-weight: 500;
    }

    /* Graphite Window Shell & Top Bar */
    .lf-window-shell {
        background-color: #141415;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 16px;
        box-shadow: none;
    }

    .lf-topbar {
        background-color: #141415;
        background-image: none;
        border-bottom: 1px solid rgba(237, 237, 237, 0.08);
        border-top-left-radius: 16px;
        border-top-right-radius: 16px;
        padding: 12px 20px;
    }

    .lf-topbar-icon {
        color: #85858b;
        -gtk-icon-size: 16px;
    }

    .lf-topbar-title {
        color: #ededed;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
    }

    .lf-topbar-subtitle {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    /* Quiet Status Indicator (8px Dot + Plain Label) */
    .lf-fluid-cloud {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 6px;
        padding: 4px 12px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    opacity 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-fluid-cloud.cloud-pulse {
        background-color: #232325;
        border-color: rgba(237, 237, 237, 0.12);
    }

    .lf-cloud-dot {
        min-width: 8px;
        min-height: 8px;
        border-radius: 999px;
        background-color: #e0573c;
    }

    .lf-fluid-cloud.cloud-info .lf-cloud-dot   { background-color: #e0573c; }
    .lf-fluid-cloud.cloud-ok .lf-cloud-dot     { background-color: #4cc38a; }
    .lf-fluid-cloud.cloud-warn .lf-cloud-dot   { background-color: #d9a03f; }
    .lf-fluid-cloud.cloud-danger .lf-cloud-dot { background-color: #e5484d; }

    .lf-cloud-text {
        color: #a1a1a6;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    /* Window Controls (Symbolic Icons, Quiet Graphite Hover) */
    .lf-win-btn {
        background-color: transparent;
        background-image: none;
        color: #85858b;
        border: 1px solid transparent;
        border-radius: 8px;
        min-width: 32px;
        min-height: 32px;
        padding: 0px;
        font-size: 13px;
        font-weight: 400;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    opacity 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-win-btn image {
        -gtk-icon-size: 16px;
    }

    .lf-win-btn:hover {
        background-color: #232325;
        background-image: none;
        border-color: rgba(237, 237, 237, 0.08);
        color: #ededed;
    }

    .lf-win-btn:active {
        transform: scale(0.98);
    }

    .lf-win-close:hover {
        background-color: #232325;
        background-image: none;
        border-color: rgba(229, 72, 77, 0.35);
        color: #e5484d;
    }

    /* Graphite Surface Cards */
    .lf-card,
    .section-box,
    .installer-card {
        background-color: #19191b;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 12px;
        padding: 16px 20px;
        margin-bottom: 16px;
        box-shadow: none;
        transition: border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    background-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-card:hover,
    .section-box:hover,
    .installer-card:hover {
        border-color: rgba(237, 237, 237, 0.12);
    }

    /* Typography Hierarchy */
    .lf-section-title,
    .section-title,
    .section-heading {
        color: #ededed;
        font-size: 16px;
        font-weight: 500;
        margin-bottom: 4px;
        letter-spacing: 0px;
    }

    .lf-label,
    .setting-label {
        color: #ededed;
        font-size: 14px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .lf-subtext,
    .setting-subtext,
    .section-desc {
        color: #a1a1a6;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    /* Reusable Keycap Signature */
    .lf-keycap {
        background-color: #232325;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-bottom: 2px solid rgba(0, 0, 0, 0.72);
        border-radius: 6px;
        padding: 2px 8px;
        min-height: 20px;
        font-family: monospace;
        font-size: 12px;
        font-weight: 500;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
        box-shadow: inset 0 1px 0 rgba(237, 237, 237, 0.08);
    }

    .lf-keycap-plus {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        padding: 0px 2px;
    }

    .lf-keycap-sep {
        color: #85858b;
        font-size: 12px;
        font-weight: 400;
        padding: 0px 4px;
    }

    button {
        background-image: none;
        text-shadow: none;
        -gtk-icon-shadow: none;
    }

    button:disabled {
        opacity: 0.45;
    }

    /* Primary Buttons: Off-white #ededed fill, #141415 text */
    .lf-btn-primary,
    .action-btn,
    .start-btn,
    .lf-btn-flame,
    .primary-btn {
        background-color: #ededed;
        background-image: none;
        color: #141415;
        border: 1px solid #ededed;
        border-radius: 8px;
        padding: 8px 16px;
        min-height: 36px;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    opacity 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-btn-primary:hover,
    .action-btn:hover,
    .start-btn:hover,
    .lf-btn-flame:hover,
    .primary-btn:hover {
        background-color: #ffffff;
        border-color: #ffffff;
        color: #141415;
    }

    .lf-btn-primary:active,
    .action-btn:active,
    .start-btn:active,
    .lf-btn-flame:active,
    .primary-btn:active {
        transform: scale(0.98);
    }

    /* Secondary Buttons: #232325 + Hairline + Primary Text */
    .lf-btn-emerald,
    .install-btn,
    .lf-btn-glass,
    .secondary-btn {
        background-color: #232325;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 8px 16px;
        min-height: 36px;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    opacity 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-btn-emerald:hover,
    .install-btn:hover,
    .lf-btn-glass:hover,
    .secondary-btn:hover {
        background-color: #2c2c2f;
        background-image: none;
        color: #ededed;
        border-color: rgba(237, 237, 237, 0.18);
    }

    .lf-btn-emerald:active,
    .install-btn:active,
    .lf-btn-glass:active,
    .secondary-btn:active {
        transform: scale(0.98);
    }

    /* Destructive Buttons: #e5484d Text on #232325 + Red Hairline */
    .lf-btn-danger,
    .danger-btn {
        background-color: #232325;
        background-image: none;
        color: #e5484d;
        border: 1px solid rgba(229, 72, 77, 0.35);
        border-radius: 8px;
        padding: 8px 16px;
        min-height: 36px;
        font-size: 14px;
        font-weight: 500;
        letter-spacing: 0px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    opacity 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-btn-danger:hover,
    .danger-btn:hover {
        background-color: rgba(229, 72, 77, 0.12);
        border-color: rgba(229, 72, 77, 0.50);
        color: #e5484d;
    }

    .lf-btn-danger:active,
    .danger-btn:active {
        transform: scale(0.98);
    }

    /* Sidebar & Navigation Rows */
    .lf-sidebar {
        background-color: #0f0f10;
        border-right: 1px solid rgba(237, 237, 237, 0.08);
        padding: 12px 8px;
    }

    .lf-nav-pill {
        background-color: transparent;
        background-image: none;
        color: #a1a1a6;
        border: 1px solid transparent;
        border-radius: 8px;
        min-height: 36px;
        padding: 6px 12px;
        margin-bottom: 4px;
        font-size: 14px;
        font-weight: 400;
        letter-spacing: 0px;
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-nav-pill:hover {
        background-color: #19191b;
        color: #ededed;
    }

    .lf-nav-pill.active {
        background-color: #232325;
        color: #ededed;
        font-weight: 500;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-left: 2px solid #c83b22;
        box-shadow: none;
    }

    .lf-step-pill {
        background-color: transparent;
        background-image: none;
        color: #85858b;
        border: 1px solid transparent;
        border-radius: 8px;
        padding: 8px 16px;
        min-height: 36px;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
        box-shadow: none;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .lf-step-pill:hover {
        background-color: #19191b;
        color: #ededed;
    }

    .lf-step-pill.active {
        background-color: #232325;
        color: #ededed;
        font-weight: 500;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-bottom: 2px solid #c83b22;
        box-shadow: none;
    }

    /* Quiet Status Badges */
    .status-badge-ok,
    .disk-badge-ok {
        color: #4cc38a;
        background-color: rgba(76, 195, 138, 0.10);
        border: 1px solid rgba(76, 195, 138, 0.24);
        border-radius: 6px;
        padding: 6px 12px;
        font-size: 12px;
        font-weight: 500;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    .status-badge-info {
        color: #a1a1a6;
        background-color: #232325;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 6px;
        padding: 4px 8px;
        font-size: 12px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    .disk-badge-warn {
        color: #d9a03f;
        background-color: rgba(217, 160, 63, 0.10);
        border: 1px solid rgba(217, 160, 63, 0.28);
        border-radius: 6px;
        padding: 8px 12px;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
        font-feature-settings: "tnum";
    }

    /* Form Controls */
    entry,
    passwordentry,
    spinbutton {
        background-color: #232325;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 8px 12px;
        min-height: 20px;
        caret-color: #ededed;
        font-size: 14px;
        font-weight: 400;
        font-feature-settings: "tnum";
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    passwordentry > text {
        background-color: transparent;
        background-image: none;
        border: none;
        padding: 0px;
        color: #ededed;
        caret-color: #ededed;
    }

    entry:focus,
    entry:focus-within,
    passwordentry:focus-within,
    spinbutton:focus-within {
        border-color: rgba(224, 87, 60, 0.65);
        outline: 2px solid rgba(224, 87, 60, 0.65);
        outline-offset: 1px;
        background-color: #232325;
        box-shadow: none;
    }

    dropdown > button,
    combobox > box > button {
        background-color: #232325;
        background-image: none;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 8px;
        padding: 8px 12px;
        min-height: 36px;
        font-size: 14px;
        font-weight: 400;
        font-feature-settings: "tnum";
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    dropdown > button:hover,
    combobox > box > button:hover {
        background-color: #2c2c2f;
        border-color: rgba(237, 237, 237, 0.18);
    }

    popover,
    popover > contents {
        background-color: #19191b;
        color: #ededed;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 12px;
        padding: 4px;
        box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
    }

    popover listview,
    popover box {
        background-color: transparent;
        color: #ededed;
    }

    popover row,
    popover modelbutton {
        border-radius: 8px;
        padding: 8px 12px;
        color: #ededed;
        font-size: 14px;
        font-weight: 400;
    }

    popover row:selected,
    popover row:hover,
    popover modelbutton:hover {
        background-color: #232325;
        color: #ededed;
    }

    checkbutton {
        color: #ededed;
        font-size: 14px;
        font-weight: 400;
    }

    checkbutton > label {
        color: #ededed;
    }

    checkbutton > check {
        background-color: #232325;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.24);
        border-radius: 4px;
        min-width: 16px;
        min-height: 16px;
        margin-right: 8px;
        color: #ededed;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    checkbutton:checked > check {
        background-color: #c83b22;
        border-color: #c83b22;
        color: #ededed;
    }

    switch {
        background-color: #232325;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.12);
        border-radius: 999px;
        min-width: 40px;
        min-height: 24px;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                    border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    switch:checked {
        background-color: #c83b22;
        border-color: #c83b22;
    }

    switch > slider {
        background-color: #ededed;
        background-image: none;
        border-radius: 999px;
        min-width: 18px;
        min-height: 18px;
        margin: 2px;
    }

    progressbar > text {
        color: #a1a1a6;
        font-size: 12px;
        font-weight: 400;
        font-feature-settings: "tnum";
        margin-bottom: 4px;
    }

    progressbar > trough {
        background-color: #232325;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 4px;
        min-height: 8px;
    }

    progressbar > trough > progress {
        background-color: #c83b22;
        background-image: none;
        border-radius: 4px;
        min-height: 8px;
    }

    notebook,
    notebook > stack {
        background-color: transparent;
    }

    scrolledwindow,
    viewport {
        background-color: transparent;
    }

    scrollbar {
        background-color: transparent;
    }

    scrollbar slider {
        background-color: rgba(237, 237, 237, 0.16);
        border-radius: 4px;
        min-width: 6px;
        min-height: 32px;
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    scrollbar slider:hover {
        background-color: rgba(237, 237, 237, 0.28);
    }

    separator {
        background-color: rgba(237, 237, 237, 0.08);
        min-height: 1px;
    }
"#;

pub fn apply_living_flow_theme(extra_css: &str) {
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }

    if let Some(display) = gdk::Display::default() {
        let icon_theme = IconTheme::for_display(&display);
        icon_theme.add_search_path("/usr/share/icons/hicolor");
        icon_theme.add_search_path("/usr/share/pixmaps");
        icon_theme.add_search_path("/usr/share/solarui/icons/living-flow");
        icon_theme.add_search_path("/usr/share/solarui/icons");

        let combined = format!("{}\n{}", LIVING_FLOW_CORE_CSS, extra_css);
        let provider = CssProvider::new();
        provider.load_from_string(&combined);
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk4::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

/// Decodes embedded PNG bytes directly via GTK4's native `gdk::Texture::from_bytes`
/// (never relies on external `gdk-pixbuf` loader plugins or `loaders.cache`).
pub fn load_png_emblem(png_bytes: &[u8], size: i32) -> Image {
    let bytes = glib::Bytes::from(png_bytes);
    if let Ok(texture) = gdk::Texture::from_bytes(&bytes) {
        let img = Image::from_paintable(Some(&texture));
        img.set_pixel_size(size);
        img.set_size_request(size, size);
        return img;
    }
    let fallback = Image::from_icon_name("blazeos");
    fallback.set_pixel_size(size);
    fallback
}

/// Decodes wide/non-square official brand logos with guaranteed `ContentFit::Contain`
/// aspect-ratio preservation (never stretches or squishes horizontal logos).
pub fn load_png_wide_logo(png_bytes: &[u8], width: i32, height: i32) -> gtk4::Picture {
    let pic = gtk4::Picture::new();
    pic.set_can_shrink(true);
    pic.set_content_fit(gtk4::ContentFit::Contain);
    pic.set_size_request(width, height);
    pic.set_halign(gtk4::Align::Center);
    pic.set_valign(gtk4::Align::Center);
    let bytes = glib::Bytes::from(png_bytes);
    if let Ok(texture) = gdk::Texture::from_bytes(&bytes) {
        pic.set_paintable(Some(&texture));
    }
    pic
}

/// Builds a draggable SolarUI top bar with optional emblem, title, subtitle,
/// status indicator, and symbolic minimize/maximize/close controls.
pub fn build_living_flow_topbar(
    window: &Window,
    emblem: Option<Image>,
    title_text: &str,
    subtitle_text: &str,
    fluid_cloud: Option<&FluidCloud>,
    on_close: Option<Box<dyn Fn()>>,
) -> WindowHandle {
    let handle = WindowHandle::new();
    let bar = GtkBox::new(Orientation::Horizontal, 12);
    bar.add_css_class("lf-topbar");

    if let Some(img) = emblem {
        img.set_valign(gtk4::Align::Center);
        bar.append(&img);
    }

    let titles = GtkBox::new(Orientation::Vertical, 2);
    titles.set_valign(gtk4::Align::Center);
    titles.set_hexpand(true);

    let t_lbl = Label::new(Some(title_text));
    t_lbl.add_css_class("lf-topbar-title");
    t_lbl.set_halign(gtk4::Align::Start);
    t_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    titles.append(&t_lbl);

    if !subtitle_text.trim().is_empty() {
        let s_lbl = Label::new(Some(subtitle_text));
        s_lbl.add_css_class("lf-topbar-subtitle");
        s_lbl.set_halign(gtk4::Align::Start);
        s_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        titles.append(&s_lbl);
    }

    bar.append(&titles);
    if let Some(fc) = fluid_cloud {
        bar.append(fc.widget());
    }

    let controls = GtkBox::new(Orientation::Horizontal, 4);
    controls.set_valign(gtk4::Align::Center);

    let icon_theme = gdk::Display::default().map(|d| IconTheme::for_display(&d));
    let has_icon = |name: &str| {
        icon_theme
            .as_ref()
            .map(|theme| theme.has_icon(name))
            .unwrap_or(false)
    };

    let min_btn = if has_icon("window-minimize-symbolic") {
        Button::from_icon_name("window-minimize-symbolic")
    } else {
        Button::with_label("—")
    };
    min_btn.add_css_class("lf-win-btn");
    min_btn.set_tooltip_text(Some("Küçült"));
    let win_min = window.downgrade();
    min_btn.connect_clicked(move |_| {
        if let Some(w) = win_min.upgrade() {
            w.minimize();
        }
        let _ = Command::new("solar-shell").arg("minimize").spawn();
    });
    controls.append(&min_btn);

    let max_btn = if has_icon("window-maximize-symbolic") {
        Button::from_icon_name("window-maximize-symbolic")
    } else {
        Button::with_label("□")
    };
    max_btn.add_css_class("lf-win-btn");
    max_btn.set_tooltip_text(Some("Ekranı kapla"));
    let win_max = window.downgrade();
    max_btn.connect_clicked(move |_| {
        if let Some(w) = win_max.upgrade() {
            if w.is_maximized() {
                w.unmaximize();
            } else {
                w.maximize();
            }
        }
    });
    controls.append(&max_btn);

    let close_btn = if has_icon("window-close-symbolic") {
        Button::from_icon_name("window-close-symbolic")
    } else {
        Button::with_label("✕")
    };
    close_btn.add_css_class("lf-win-btn");
    close_btn.add_css_class("lf-win-close");
    close_btn.set_tooltip_text(Some("Kapat"));
    let win_close = window.downgrade();
    let on_close_rc = on_close.map(Rc::new);
    close_btn.connect_clicked(move |_| {
        if let Some(cb) = &on_close_rc {
            cb();
        }
        if let Some(w) = win_close.upgrade() {
            w.close();
        }
    });
    controls.append(&close_btn);

    bar.append(&controls);
    handle.set_child(Some(&bar));
    handle
}
