// ==============================================================================
// SolarUI Omnibar
// Raycast & Spotlight Grade Unified Command & Search Hub
// - Instant Math Calculator (= 25 * 1024, sqrt, sin, hex, bin)
// - Unit & Currency Converter (usd to try, km in miles, c in f, gb in mb)
// - Process Killer (kill <proc>)
// - System Commands (game, berp, theme, lock, reboot, power)
// - Fuzzy Desktop Application Launcher
// ==============================================================================

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, Entry, EventControllerKey, IconTheme, Image, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow, Window,
};
use std::process::Command;
use std::sync::{Arc, Mutex};

use crate::apps::{scan_desktop_applications, AppInfo};
use crate::living_flow::{apply_living_flow_theme, FluidCloud, FluidCloudTone};

#[derive(Clone, Debug)]
pub enum OmnibarItemType {
    App(AppInfo),
    Calculation(String, f64),
    Conversion(String, String),
    Clipboard(String, String), // raw_entry, decoded_text
    ProcessKill(u32, String),
    SystemAction(String, String, String), // id, label, command
    WebSearch(String, String),            // query, url
    AiQuery(String),                      // prompt
}

#[derive(Clone, Debug)]
pub struct OmnibarItem {
    pub title: String,
    pub subtitle: String,
    pub badge: String,
    pub icon_name: Option<String>,
    pub item_type: OmnibarItemType,
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

fn build_omnibar_ui(main_loop: glib::MainLoop) {
    let extra_css = r#"
        .omnibar-card {
            background-color: #141415;
            background-image: none;
            border: 1px solid rgba(237, 237, 237, 0.08);
            border-radius: 16px;
            padding: 16px 20px;
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
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

        .omnibar-entry:focus {
            background-color: #232325;
            border-color: rgba(224, 87, 60, 0.65);
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
    apply_living_flow_theme(extra_css);

    let window = Window::builder()
        .title("Solar Omnibar")
        .decorated(false)
        .default_width(720)
        .default_height(492)
        .resizable(false)
        .build();

    let loop_quit = main_loop.clone();
    window.connect_close_request(move |_| {
        let _ = std::fs::remove_file(omnibar_pid_file());
        loop_quit.quit();
        glib::Propagation::Proceed
    });

    let root_box = GtkBox::new(Orientation::Vertical, 0);
    root_box.add_css_class("omnibar-card");

    // Top Header with symbolic search icon, title, status dot and close control
    let header_row = GtkBox::new(Orientation::Horizontal, 12);
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
    root_box.append(&header_row);

    // Search input
    let entry = Entry::new();
    entry.add_css_class("omnibar-entry");
    entry.set_placeholder_text(Some("Uygulama, komut, =hesaplama, kur veya pano (cb) ara"));
    root_box.append(&entry);

    // Results container
    let scrolled = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let list_box = ListBox::new();
    list_box.add_css_class("results-list");
    list_box.set_selection_mode(gtk4::SelectionMode::Single);
    scrolled.set_child(Some(&list_box));
    root_box.append(&scrolled);

    // Footer split keycap hints
    let footer_box = GtkBox::new(Orientation::Horizontal, 16);
    footer_box.add_css_class("omnibar-footer");
    footer_box.set_valign(gtk4::Align::Center);
    append_footer_keycap_hint(&footer_box, &["↑", "↓"], "Seç");
    append_footer_keycap_hint(&footer_box, &["Enter"], "Çalıştır veya kopyala");
    append_footer_keycap_hint(&footer_box, &["Esc"], "Kapat");
    root_box.append(&footer_box);

    window.set_child(Some(&root_box));

    // Shared state
    let cached_apps = Arc::new(scan_desktop_applications());
    let current_items: Arc<Mutex<Vec<OmnibarItem>>> = Arc::new(Mutex::new(Vec::new()));

    // Window-level Key controller (captures Esc, Up/Down, Return regardless of child focus)
    let key_controller = EventControllerKey::new();
    key_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let list_weak = list_box.downgrade();
    let win_weak = window.downgrade();
    let items_ref = current_items.clone();
    let loop_action = main_loop.clone();

    key_controller.connect_key_pressed(move |_ctrl, key, _code, _modifier| {
        if key == gdk::Key::Escape {
            let _ = std::fs::remove_file(omnibar_pid_file());
            if let Some(win) = win_weak.upgrade() {
                win.close();
            }
            loop_action.quit();
            return glib::Propagation::Stop;
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
                execute_omnibar_item(&item);
                let _ = std::fs::remove_file(omnibar_pid_file());
                if let Some(win) = win_weak.upgrade() {
                    win.close();
                }
                loop_action.quit();
            }
            return glib::Propagation::Stop;
        }

        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    // Entry activate handler (Enter key inside GtkEntry)
    let win_act = window.downgrade();
    let loop_act = main_loop.clone();
    let items_act = current_items.clone();
    let list_act = list_box.downgrade();
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
            execute_omnibar_item(&item);
            let _ = std::fs::remove_file(omnibar_pid_file());
            if let Some(win) = win_act.upgrade() {
                win.close();
            }
            loop_act.quit();
        }
    });

    // ListBox row click handler
    let win_row = window.downgrade();
    let loop_row = main_loop.clone();
    let items_row = current_items.clone();
    list_box.connect_row_activated(move |_list, row| {
        let idx = row.index();
        let item_opt = items_row
            .lock()
            .ok()
            .and_then(|items| items.get(idx as usize).cloned());

        if let Some(item) = item_opt {
            execute_omnibar_item(&item);
            let _ = std::fs::remove_file(omnibar_pid_file());
            if let Some(win) = win_row.upgrade() {
                win.close();
            }
            loop_row.quit();
        }
    });

    // Search query update
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

            // Fixed-width icon column: uniform 16px symbolic icons for built-ins,
            // real app icons (20px) with quiet symbolic fallback for desktop apps
            let icon_slot = GtkBox::new(Orientation::Horizontal, 0);
            icon_slot.add_css_class("result-icon-slot");
            icon_slot.set_halign(gtk4::Align::Center);
            icon_slot.set_valign(gtk4::Align::Center);
            let icon = resolve_omnibar_item_icon(item);
            icon_slot.append(&icon);
            row_box.append(&icon_slot);

            // Text details
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

            // Quiet category tag
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

    // Initial populate
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
                // Strip freedesktop %u, %f flags
                let args: Vec<&str> = parts.filter(|a| !a.starts_with('%')).collect();
                let _ = Command::new(cmd)
                    .args(&args)
                    .spawn();
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
            let is_cliphist = raw_entry.split('\t').next().map(|s| s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false);
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
                .args(["-a", "Solar Omnibar", "Süreç sonlandırıldı", &format!("{} (PID {})", name, pid)])
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
            println!("Opening AI query for '{}'", query);
            let encoded = url_encode_query(query);
            let search_url = format!("https://duckduckgo.com/?q={}&ia=chat", encoded);
            let _ = Command::new("xdg-open").arg(&search_url).spawn();
        }
    }
}

fn url_encode_query(input: &str) -> String {
    let mut encoded = String::new();
    for b in input.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            b' ' => encoded.push('+'),
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
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

    // 1. Try cliphist list
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
                        item_type: OmnibarItemType::Clipboard(line.to_string(), preview.to_string()),
                    });

                    if items.len() >= limit {
                        break;
                    }
                }
            }
        }
    }

    // 2. If no items from cliphist, check active clipboard via wl-paste
    if items.is_empty() {
        if let Ok(out) = Command::new("wl-paste").output() {
            if out.status.success() {
                let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !text.is_empty() && (q_lower.is_empty() || text.to_lowercase().contains(&q_lower)) {
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
    let query = raw_query.trim();
    let mut items = Vec::new();
    let q_lower = query.to_lowercase();

    // 0. Pano Geçmişi (Clipboard History) - cb, pano, clip prefixes
    let is_clipboard_prefix = q_lower.starts_with("cb") || q_lower.starts_with("pano") || q_lower.starts_with("clip");
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
    if query.starts_with('=') || query.chars().any(|c| "+-*/^%".contains(c)) && query.chars().any(|c| c.is_ascii_digit()) {
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
    if query.contains(" to ") || query.contains(" in ") || query.contains("->") || query.chars().any(|c| "$€₺£".contains(c)) || query.split_whitespace().count() == 3 {
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
        let proc_query = query.trim_start_matches("/kill ").trim_start_matches("kill ").trim().to_lowercase();
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

    // 4. System quick actions (with dynamic shell toggle based on active engine)
    let cfg = solar_common::SolarConfig::load();
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
                item_type: OmnibarItemType::SystemAction(cmd_id.to_string(), title.to_string(), action.to_string()),
            });
        }
    }

    // 5. Desktop Application Search (Fuzzy)
    let q_lower = query.to_lowercase();
    for app in apps {
        let name_match = app.name.to_lowercase().contains(&q_lower);
        let comment_match = app.comment.as_deref().unwrap_or("").to_lowercase().contains(&q_lower);
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
                icon_name: app.icon.clone().or_else(|| Some("application-x-executable-symbolic".to_string())),
                item_type: OmnibarItemType::App(app.clone()),
            });
        }

        if items.len() >= 30 {
            break;
        }
    }

    // 5.5. Pano geçmişi eşleşmeleri (kullanıcı cb yazmasa bile eşleşenleri göster)
    if !is_clipboard_prefix && query.len() >= 3 {
        let clip_matches = scan_clipboard_items(query, 3);
        items.extend(clip_matches);
    }

    // 6. Web and AI Search Integration
    if !query.is_empty() {
        let cfg = solar_common::SolarConfig::load();
        if cfg.omnibar.enable_web_search {
            let encoded = url_encode_query(query);
            let search_url = match cfg.omnibar.search_engine.as_str() {
                "Google" => format!("https://www.google.com/search?q={}", encoded),
                "Brave" => format!("https://search.brave.com/search?q={}", encoded),
                "Bing" => format!("https://www.bing.com/search?q={}", encoded),
                _ => format!("https://duckduckgo.com/?q={}", encoded),
            };

            items.push(OmnibarItem {
                title: format!("Web'de ara ({}): {}", cfg.omnibar.search_engine, query),
                subtitle: format!("{} ile varsayılan tarayıcıda aç", cfg.omnibar.search_engine),
                badge: "Web".to_string(),
                icon_name: Some("web-browser-symbolic".to_string()),
                item_type: OmnibarItemType::WebSearch(query.to_string(), search_url),
            });
        }

        if !cfg.omnibar.ai_api_key.trim().is_empty() {
            items.push(OmnibarItem {
                title: format!("Yapay zekaya sor: {}", query),
                subtitle: "Tarayıcıda yapay zeka yanıtı aç".to_string(),
                badge: "Yapay zeka".to_string(),
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

    // Check functions like sqrt(X)
    if clean.starts_with("sqrt(") && clean.ends_with(')') {
        let inner = &clean[5..clean.len() - 1];
        let val = inner.parse::<f64>().ok().or_else(|| evaluate_simple_math(inner))?;
        let res = val.sqrt();
        return if res.is_finite() { Some(res) } else { None };
    }

    // Precedence levels (lowest scanned first, right-to-left for left-associativity)
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
            // Skip unary +/- at start or after another operator
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
    // Normalize currency symbols
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
    let to_unit = if parts.len() >= 4 && (parts[2] == "to" || parts[2] == "in" || parts[2] == "->" || parts[2] == "=") {
        parts[3]
    } else if parts.len() == 3 {
        parts[2]
    } else {
        parts[parts.len() - 1]
    };

    // Standard offline conversion rates
    // Currencies
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

    // Crypto
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

    // Precious metals
    if (from_unit == "gold" || from_unit == "xau" || from_unit == "ons") && to_unit == "usd" {
        let res = val * 2700.0;
        return Some((format!("{:.2} USD ($)", res), format!("{:.2}", res)));
    }
    if (from_unit == "gold" || from_unit == "altin" || from_unit == "gram") && to_unit == "try" {
        let res = val * 3350.0;
        return Some((format!("{:.2} TRY (TL)", res), format!("{:.2}", res)));
    }

    // Distance
    if from_unit == "km" && (to_unit == "mi" || to_unit == "miles") {
        let res = val * 0.621371;
        return Some((format!("{:.2} Mil", res), format!("{:.2}", res)));
    }
    if (from_unit == "mi" || from_unit == "miles") && to_unit == "km" {
        let res = val * 1.60934;
        return Some((format!("{:.2} km", res), format!("{:.2}", res)));
    }

    // Temperature
    if from_unit == "c" && to_unit == "f" {
        let res = (val * 9.0 / 5.0) + 32.0;
        return Some((format!("{:.1} °F", res), format!("{:.1}", res)));
    }
    if from_unit == "f" && to_unit == "c" {
        let res = (val - 32.0) * 5.0 / 9.0;
        return Some((format!("{:.1} °C", res), format!("{:.1}", res)));
    }

    // Data sizes
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
        let turkish = "Çağdaş güneş ışığı altında özgürce çalışan işlemci çekirdeği güç tüketimi ölçümü";
        // Must never panic on multi-byte UTF-8 boundaries (ç, ğ, ü, ş, ı, ö)
        for max_len in [0, 1, 2, 5, 10, 15, 25, 50, 65, 100] {
            let out = truncate_chars(turkish, max_len);
            assert!(out.chars().count() <= max_len + 3);
        }
        let exact = truncate_chars("ğüşıöçĞÜŞİÖÇ", 6);
        assert_eq!(exact, "ğüşıöç...");

        // Also verify generate_omnibar_items with Turkish clipboard prefix does not panic
        let items = generate_omnibar_items("pano ğüşıöç", &[]);
        let _ = items.len();
    }
}

