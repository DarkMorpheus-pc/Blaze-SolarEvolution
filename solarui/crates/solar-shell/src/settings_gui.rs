use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, DropDown, Entry, Image, Label, Orientation,
    PasswordEntry, ScrolledWindow, Separator, Stack, StackTransitionType, StringList, Window,
};
use solar_common::{
    BlazeAiProvider, CompanionExpression, CompanionShape, HardwareDetector, HardwareTuningProfile,
    PerformanceTier, ShellEngine, SolarConfig, SurfaceProvider,
};
use std::cell::Cell;
use crate::living_flow::{
    apply_living_flow_theme, build_living_flow_topbar, load_png_emblem, FluidCloud, FluidCloudTone,
};
use crate::pixel_clock::{PixelClockConfig, PixelClockStyle};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

const LOGO_PNG: &[u8] = include_bytes!("../../../data/icons/solar-logo-256.png");

fn create_logo_image(size: i32) -> Image {
    load_png_emblem(LOGO_PNG, size)
}

fn create_symbolic_icon(icon_name: &str, css_class: &str) -> Image {
    let img = Image::from_icon_name(icon_name);
    img.set_pixel_size(16);
    img.set_size_request(16, 16);
    img.set_valign(gtk4::Align::Center);
    img.add_css_class(css_class);
    img
}

fn build_page_header(title: &str, subtitle: &str) -> GtkBox {
    let header = GtkBox::new(Orientation::Vertical, 4);
    header.add_css_class("settings-page-header");

    let t = Label::new(Some(title));
    t.add_css_class("settings-page-title");
    t.set_halign(gtk4::Align::Start);
    header.append(&t);

    if !subtitle.trim().is_empty() {
        let s = Label::new(Some(subtitle));
        s.add_css_class("settings-page-subtitle");
        s.set_wrap(true);
        s.set_xalign(0.0);
        s.set_halign(gtk4::Align::Start);
        header.append(&s);
    }

    header
}

fn build_kv_row(key: &str, value: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 16);
    row.add_css_class("settings-kv-row");

    let k_lbl = Label::new(Some(key));
    k_lbl.add_css_class("kv-key");
    k_lbl.set_halign(gtk4::Align::Start);
    k_lbl.set_hexpand(true);
    row.append(&k_lbl);

    let v_lbl = Label::new(Some(value));
    v_lbl.add_css_class("kv-val");
    v_lbl.set_halign(gtk4::Align::End);
    v_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    row.append(&v_lbl);

    row
}

fn build_keycap_combo(chords: &[&[&str]]) -> GtkBox {
    let combo = GtkBox::new(Orientation::Horizontal, 4);
    combo.set_halign(gtk4::Align::Start);
    combo.set_valign(gtk4::Align::Center);

    for (chord_idx, chord) in chords.iter().enumerate() {
        if chord_idx > 0 {
            let sep = Label::new(Some("/"));
            sep.add_css_class("lf-keycap-sep");
            sep.set_valign(gtk4::Align::Center);
            combo.append(&sep);
        }

        for (key_idx, key) in chord.iter().enumerate() {
            if key_idx > 0 {
                let plus = Label::new(Some("+"));
                plus.add_css_class("lf-keycap-plus");
                plus.set_valign(gtk4::Align::Center);
                combo.append(&plus);
            }

            let cap = Label::new(Some(key));
            cap.add_css_class("lf-keycap");
            cap.set_valign(gtk4::Align::Center);
            combo.append(&cap);
        }
    }

    combo
}

pub fn launch_settings_window() {
    launch_settings_window_with_tab(None);
}

pub fn launch_settings_window_with_tab(initial_tab: Option<&str>) {
    glib::set_prgname(Some("solar-settings"));
    glib::set_application_name("SolarUI ayarları");

    if let Err(err) = gtk4::init() {
        eprintln!("Failed to initialize GTK4: {}", err);
        return;
    }

    let main_loop = glib::MainLoop::new(None, false);
    build_settings_ui(main_loop.clone(), initial_tab);
    main_loop.run();
}

fn restart_taskbar_if_enabled() {
    let cfg = SolarConfig::load();
    // Kill existing taskbar instance
    let _ = Command::new("pkill")
        .args(["-f", "solar-shell (run|gui|taskbar|window)"])
        .status();

    if cfg.taskbar.enabled {
        // Spawn fresh taskbar instance
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let _ = Command::new("solar-shell").arg("run").spawn();
        });
    }
}

fn build_settings_ui(main_loop: glib::MainLoop, initial_tab: Option<&str>) {
    let extra_css = r#"
        .settings-stage {
            padding: 24px;
        }

        .settings-page-header {
            margin-bottom: 16px;
        }

        .settings-page-title {
            color: #ededed;
            font-size: 20px;
            font-weight: 600;
            letter-spacing: 0px;
        }

        .settings-page-subtitle {
            color: #a1a1a6;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
        }

        .settings-cat-header {
            color: #85858b;
            font-size: 12px;
            font-weight: 500;
            letter-spacing: 0px;
            margin: 20px 12px 8px 12px;
        }

        .settings-cat-header-first {
            margin-top: 4px;
        }

        .settings-nav-icon {
            color: #85858b;
            -gtk-icon-size: 16px;
            transition: color 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .lf-nav-pill:hover .settings-nav-icon,
        .lf-nav-pill.active .settings-nav-icon {
            color: #ededed;
        }

        .settings-row-icon {
            color: #85858b;
            -gtk-icon-size: 16px;
        }

        .settings-mono {
            font-feature-settings: "tnum";
        }

        .settings-meta {
            color: #85858b;
            font-size: 12px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        .settings-kv-row {
            padding: 4px 0px;
        }

        .kv-key {
            color: #85858b;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
        }

        .kv-val {
            color: #ededed;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        .monitor-card {
            background-color: #141415;
            border: 1px solid rgba(237, 237, 237, 0.08);
            border-radius: 8px;
            padding: 16px;
            transition: border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .monitor-card:hover {
            border-color: rgba(237, 237, 237, 0.12);
        }

        .monitor-name {
            color: #ededed;
            font-size: 14px;
            font-weight: 500;
            letter-spacing: 0px;
        }

        .wp-card {
            background-color: #141415;
            border: 1px solid rgba(237, 237, 237, 0.08);
            border-radius: 12px;
            padding: 0px;
            transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        border-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .wp-card:hover {
            background-color: #232325;
            border-color: rgba(237, 237, 237, 0.16);
        }

        .wp-card.wp-active {
            border-color: rgba(224, 87, 60, 0.70);
        }

        .wp-footer {
            padding: 8px 12px 12px 12px;
        }

        .wp-title {
            color: #ededed;
            font-size: 13px;
            font-weight: 500;
            letter-spacing: 0px;
        }

        .wp-apply-btn {
            min-height: 28px;
            padding: 4px 12px;
            border-radius: 6px;
            font-size: 12px;
            font-weight: 500;
        }

        .swatch-chip {
            background-color: #232325;
            background-image: none;
            color: #ededed;
            border: 1px solid rgba(237, 237, 237, 0.12);
            border-radius: 8px;
            padding: 6px 12px;
            min-height: 32px;
            font-size: 13px;
            font-weight: 500;
            letter-spacing: 0px;
            box-shadow: none;
            transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .swatch-chip:hover {
            background-color: #2c2c2f;
            border-color: rgba(237, 237, 237, 0.18);
        }

        .swatch-chip:active {
            transform: scale(0.98);
        }

        .swatch-chip.swatch-active {
            background-color: #2c2c2f;
            border-color: rgba(224, 87, 60, 0.70);
        }

        .swatch-dot {
            min-width: 12px;
            min-height: 12px;
            border-radius: 999px;
            border: 1px solid rgba(237, 237, 237, 0.24);
        }

        .swatch-dot-red    { background-color: #e81123; }
        .swatch-dot-blue   { background-color: #0078d7; }
        .swatch-dot-dark   { background-color: #2d2d30; }
        .swatch-dot-green  { background-color: #107c10; }
        .swatch-dot-purple { background-color: #881798; }
        .swatch-dot-amber  { background-color: #ff6a00; }
        .swatch-dot-teal   { background-color: #008272; }

        .status-dot {
            min-width: 8px;
            min-height: 8px;
            border-radius: 999px;
        }

        .status-dot-ok     { background-color: #4cc38a; }
        .status-dot-warn   { background-color: #d9a03f; }
        .status-dot-danger { background-color: #e5484d; }
        .status-dot-muted  { background-color: #85858b; }

        .status-text {
            color: #a1a1a6;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        .status-text-ok {
            color: #4cc38a;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        .status-text-warn {
            color: #d9a03f;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        .status-text-danger {
            color: #e5484d;
            font-size: 13px;
            font-weight: 400;
            letter-spacing: 0px;
            font-feature-settings: "tnum";
        }

        flowboxchild {
            padding: 0px;
            background-color: transparent;
            border: none;
        }

        flowboxchild:hover,
        flowboxchild:selected {
            background-color: transparent;
        }

        colorbutton > button,
        button.color {
            background-color: #232325;
            background-image: none;
            border: 1px solid rgba(237, 237, 237, 0.12);
            border-radius: 8px;
            min-height: 36px;
            padding: 4px;
            box-shadow: none;
        }
    "#;
    apply_living_flow_theme(extra_css);

    let window = Window::builder()
        .title("SolarUI ayarları")
        .decorated(false)
        .default_width(1040)
        .default_height(660)
        .resizable(true)
        .build();

    let loop_quit = main_loop.clone();
    window.connect_close_request(move |_| {
        loop_quit.quit();
        glib::Propagation::Proceed
    });

    let key_ctrl = gtk4::EventControllerKey::new();
    let win_esc = window.downgrade();
    key_ctrl.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            if let Some(w) = win_esc.upgrade() {
                w.close();
            }
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_ctrl);

    let shell_box = GtkBox::new(Orientation::Vertical, 0);
    shell_box.add_css_class("lf-window-shell");

    let fluid_cloud = FluidCloud::new("Ayarlar güncel");
    let emblem = create_symbolic_icon("preferences-system-symbolic", "lf-topbar-icon");
    let loop_top = main_loop.clone();
    let topbar = build_living_flow_topbar(
        &window,
        Some(emblem),
        "SolarUI ayarları",
        "Blaze SolarEvolution",
        Some(&fluid_cloud),
        Some(Box::new(move || loop_top.quit())),
    );
    shell_box.append(&topbar);

    // Two-Pane Graphite Layout: Left Navigation Rail + Right Settings Stage
    let split_box = GtkBox::new(Orientation::Horizontal, 0);
    split_box.set_vexpand(true);
    split_box.set_hexpand(true);

    let sidebar_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .min_content_width(256)
        .vexpand(true)
        .build();
    sidebar_scroll.add_css_class("lf-sidebar");

    let nav_list = GtkBox::new(Orientation::Vertical, 0);

    let stage_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();

    let stack = Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_transition_type(StackTransitionType::None);
    stack.set_transition_duration(0);
    stack.add_css_class("settings-stage");

    // ── BUILD ALL PAGES ───────────────────────────────────────────────────────
    let personalize_tab = build_personalize_tab();
    let clock_tab = build_pixel_clock_tab();
    let taskbar_tab = build_taskbar_tab();
    let display_tab = build_display_tab();
    let shell_tab = build_shell_tab();
    let perf_tab = build_performance_tab();
    let health_tab = build_health_tab();
    let caelestia_tab = build_caelestia_tab();

    // Noctalia Bar Quick Control Page
    let noctalia_tab = GtkBox::new(Orientation::Vertical, 8);
    noctalia_tab.append(&build_page_header(
        "Noctalia Bar",
        "Üst çubuk ayarlarını ve hızlı kontrol panellerini doğrudan açın",
    ));

    let n_sec = GtkBox::new(Orientation::Vertical, 12);
    n_sec.add_css_class("section-box");

    let n_title = Label::new(Some("Hızlı erişim"));
    n_title.add_css_class("section-title");
    n_title.set_halign(gtk4::Align::Start);
    n_sec.append(&n_title);

    let n_desc = Label::new(Some(
        "Noctalia Bar panelleri etkin oturum üzerinde anında açılır",
    ));
    n_desc.add_css_class("setting-subtext");
    n_desc.set_wrap(true);
    n_desc.set_xalign(0.0);
    n_desc.set_halign(gtk4::Align::Start);
    n_sec.append(&n_desc);

    let btn_grid = GtkBox::new(Orientation::Horizontal, 8);
    let cloud_noc = fluid_cloud.clone();
    let btn_settings = Button::with_label("Ayarları aç");
    btn_settings.add_css_class("action-btn");
    btn_settings.connect_clicked(move |_| {
        cloud_noc.pulse("Noctalia ayar paneli açıldı", FluidCloudTone::Info, false);
        let _ = Command::new("noctalia").args(["msg", "settings-toggle"]).spawn();
    });
    btn_grid.append(&btn_settings);

    let btn_cc = Button::with_label("Kontrol merkezi");
    btn_cc.add_css_class("secondary-btn");
    btn_cc.connect_clicked(|_| {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "control-center"])
            .spawn();
    });
    btn_grid.append(&btn_cc);

    let btn_wp = Button::with_label("Duvar kağıdı");
    btn_wp.add_css_class("secondary-btn");
    btn_wp.connect_clicked(|_| {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "wallpaper"])
            .spawn();
    });
    btn_grid.append(&btn_wp);

    let btn_audio = Button::with_label("Ses mikseri");
    btn_audio.add_css_class("secondary-btn");
    btn_audio.connect_clicked(|_| {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "control-center", "audio"])
            .spawn();
    });
    btn_grid.append(&btn_audio);

    n_sec.append(&btn_grid);
    noctalia_tab.append(&n_sec);

    let omnibar_tab = build_omnibar_tab();

    // Shortcuts & Welcome HUD Page
    let hud_tab = GtkBox::new(Orientation::Vertical, 8);
    hud_tab.append(&build_page_header(
        "Kısayollar",
        "Başlangıç karşılama penceresi ve sistem klavye kısayolları",
    ));

    let h_sec = GtkBox::new(Orientation::Vertical, 12);
    h_sec.add_css_class("section-box");

    let h_title = Label::new(Some("Karşılama ve kısayol rehberi"));
    h_title.add_css_class("section-title");
    h_title.set_halign(gtk4::Align::Start);
    h_sec.append(&h_title);

    let h_sub = Label::new(Some(
        "Klavye kısayol özetini ve ilk kurulum rehberini istediğiniz zaman açabilirsiniz",
    ));
    h_sub.add_css_class("setting-subtext");
    h_sub.set_wrap(true);
    h_sub.set_xalign(0.0);
    h_sub.set_halign(gtk4::Align::Start);
    h_sec.append(&h_sub);

    let open_welcome_btn = Button::with_label("Rehberi aç");
    open_welcome_btn.add_css_class("action-btn");
    open_welcome_btn.set_halign(gtk4::Align::Start);
    open_welcome_btn.connect_clicked(|_| {
        let _ = Command::new("solar-shell").arg("welcome").spawn();
    });
    h_sec.append(&open_welcome_btn);

    let cfg = SolarConfig::load();
    let chk_startup = CheckButton::with_label("Oturum açılışında karşılama penceresini göster");
    chk_startup.set_active(cfg.shortcuts_hud.show_at_startup);
    let cloud_hud = fluid_cloud.clone();
    chk_startup.connect_toggled(move |cb| {
        let mut c = SolarConfig::load();
        c.shortcuts_hud.show_at_startup = cb.is_active();
        let _ = c.save();
        cloud_hud.pulse("Karşılama tercihi kaydedildi", FluidCloudTone::Success, false);
    });
    h_sec.append(&chk_startup);
    hud_tab.append(&h_sec);

    // About Page
    let about_tab = GtkBox::new(Orientation::Vertical, 8);
    about_tab.append(&build_page_header(
        "Hakkında",
        "SolarUI sürüm ve sistem altyapı bilgileri",
    ));

    let a_sec = GtkBox::new(Orientation::Vertical, 12);
    a_sec.add_css_class("section-box");
    a_sec.set_halign(gtk4::Align::Fill);

    let about_id = GtkBox::new(Orientation::Horizontal, 16);
    let big_logo = create_logo_image(56);
    big_logo.set_valign(gtk4::Align::Center);
    about_id.append(&big_logo);

    let about_titles = GtkBox::new(Orientation::Vertical, 4);
    about_titles.set_valign(gtk4::Align::Center);

    let a_name = Label::new(Some("SolarUI"));
    a_name.add_css_class("section-title");
    a_name.set_halign(gtk4::Align::Start);
    about_titles.append(&a_name);

    let a_sig = Label::new(Some("Blaze SolarEvolution 5.05 masaüstü ortamı"));
    a_sig.add_css_class("setting-subtext");
    a_sig.set_halign(gtk4::Align::Start);
    about_titles.append(&a_sig);
    about_id.append(&about_titles);
    a_sec.append(&about_id);

    let sep = Separator::new(Orientation::Horizontal);
    a_sec.append(&sep);

    let kern_ver = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "7.2.x".to_string());

    a_sec.append(&build_kv_row("Sürüm", "1.1.0"));
    a_sec.append(&build_kv_row("Çekirdek", &format!("Linux {}", kern_ver)));
    a_sec.append(&build_kv_row("Altyapı", "Rust, GTK4, Niri Wayland"));
    a_sec.append(&build_kv_row("Paket sistemi", "Portadora (.blz), Flathub"));
    a_sec.append(&build_kv_row(
        "Geliştirici",
        "Bulut Ars. E. (DarkMorpheus), BlazeOS Project",
    ));
    about_tab.append(&a_sec);

    // Register pages and build uniform symbolic navigation rail
    let default_page = match initial_tab.map(|t| t.trim().to_ascii_lowercase()) {
        Some(ref t) if t == "display" || t == "resolution" || t == "ekran" => "display",
        Some(ref t) if t == "personalize" || t == "wallpaper" || t == "theme" || t == "we10x" => "personalize",
        Some(ref t) if t == "shell" => "shell",
        Some(ref t) if t == "taskbar" => "taskbar",
        Some(ref t) if t == "clock" => "clock",
        Some(ref t) if t == "perf" => "perf",
        Some(ref t) if t == "health" => "health",
        Some(ref t) if t == "omnibar" => "omnibar",
        Some(ref t) if t == "hud" => "hud",
        Some(ref t) if t == "about" => "about",
        _ => "personalize",
    };

    let pages: Vec<(&str, &str, &str, &str, GtkBox)> = vec![
        ("Görünüm", "personalize", "preferences-desktop-wallpaper-symbolic", "Kişiselleştirme", personalize_tab),
        ("", "display", "video-display-symbolic", "Ekran", display_tab),
        ("", "clock", "preferences-system-time-symbolic", "Masaüstü saati", clock_tab),
        ("", "taskbar", "view-paged-symbolic", "Görev çubuğu", taskbar_tab),
        ("Kabuk", "shell", "user-desktop-symbolic", "Masaüstü kabuğu", shell_tab),
        ("", "caelestia", "view-grid-symbolic", "Caelestia", caelestia_tab),
        ("", "noctalia", "weather-clear-night-symbolic", "Noctalia Bar", noctalia_tab),
        ("Sistem", "perf", "power-profile-performance-symbolic", "Performans", perf_tab),
        ("", "health", "security-high-symbolic", "Sistem sağlığı", health_tab),
        ("", "omnibar", "system-search-symbolic", "Omnibar", omnibar_tab),
        ("", "hud", "input-keyboard-symbolic", "Kısayollar", hud_tab),
        ("", "about", "help-about-symbolic", "Hakkında", about_tab),
    ];

    let mut nav_buttons: Vec<(String, String, Button)> = Vec::new();
    let mut is_first_group = true;
    for (group_hdr, page_id, icon_name, label_txt, widget_box) in pages {
        if !group_hdr.is_empty() {
            let hdr = Label::new(Some(group_hdr));
            hdr.add_css_class("settings-cat-header");
            if is_first_group {
                hdr.add_css_class("settings-cat-header-first");
                is_first_group = false;
            }
            hdr.set_halign(gtk4::Align::Start);
            nav_list.append(&hdr);
        }
        stack.add_named(&widget_box, Some(page_id));

        let btn = Button::new();
        btn.add_css_class("lf-nav-pill");
        let row_box = GtkBox::new(Orientation::Horizontal, 8);
        let nav_icon = create_symbolic_icon(icon_name, "settings-nav-icon");
        row_box.append(&nav_icon);

        let b_lbl = Label::new(Some(label_txt));
        b_lbl.set_halign(gtk4::Align::Start);
        b_lbl.set_valign(gtk4::Align::Center);
        b_lbl.set_hexpand(true);
        b_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        row_box.append(&b_lbl);
        btn.set_child(Some(&row_box));

        if page_id == default_page {
            btn.add_css_class("active");
        }
        nav_list.append(&btn);
        nav_buttons.push((page_id.to_string(), label_txt.to_string(), btn));
    }

    let all_btns = Rc::new(nav_buttons);
    for (page_id, title_str, btn) in all_btns.iter() {
        let pid = page_id.clone();
        let t_str = title_str.clone();
        let btns_ref = all_btns.clone();
        let stack_ref = stack.clone();
        let stage_ref = stage_scroll.clone();
        let cloud_nav = fluid_cloud.clone();
        btn.connect_clicked(move |_| {
            for (other_id, _, other_btn) in btns_ref.iter() {
                if other_id == &pid {
                    other_btn.add_css_class("active");
                } else {
                    other_btn.remove_css_class("active");
                }
            }
            stack_ref.set_visible_child_name(&pid);
            stage_ref.vadjustment().set_value(0.0);
            cloud_nav.pulse(&t_str, FluidCloudTone::Info, false);
        });
    }

    stack.set_visible_child_name(default_page);
    sidebar_scroll.set_child(Some(&nav_list));
    stage_scroll.set_child(Some(&stack));

    split_box.append(&sidebar_scroll);
    split_box.append(&stage_scroll);
    shell_box.append(&split_box);

    window.set_child(Some(&shell_box));
    window.present();
}

fn build_performance_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Performans",
        "Donanım profili, efekt düzeyi ve arka plan kaynak yönetimi",
    ));

    let hw = HardwareDetector::detect();
    let current_profile = HardwareTuningProfile::load();

    // 1. Detected Hardware Card
    let hw_sec = GtkBox::new(Orientation::Vertical, 8);
    hw_sec.add_css_class("section-box");

    let hw_title = Label::new(Some("Algılanan donanım"));
    hw_title.add_css_class("section-title");
    hw_title.set_halign(gtk4::Align::Start);
    hw_sec.append(&hw_title);

    let gpu_devices = if hw.gpu_devices.is_empty() {
        "Tümleşik DRM".to_string()
    } else {
        hw.gpu_devices.join(", ")
    };
    let power_str = if hw.on_battery {
        "Pil"
    } else {
        "AC adaptör"
    };

    hw_sec.append(&build_kv_row("GPU", &hw.gpu_vendor));
    hw_sec.append(&build_kv_row("Aygıtlar", &gpu_devices));
    hw_sec.append(&build_kv_row(
        "Bellek",
        &format!("{:.1} GB", (hw.total_ram_mb as f64) / 1024.0),
    ));
    hw_sec.append(&build_kv_row(
        "İşlemci",
        &format!("{} çekirdek", hw.cpu_cores),
    ));
    hw_sec.append(&build_kv_row("Güç kaynağı", power_str));

    let sep_hw = Separator::new(Orientation::Horizontal);
    sep_hw.set_margin_top(4);
    sep_hw.set_margin_bottom(4);
    hw_sec.append(&sep_hw);

    let rec_box = GtkBox::new(Orientation::Horizontal, 8);
    let rec_title = Label::new(Some("Önerilen profil"));
    rec_title.add_css_class("kv-key");
    rec_title.set_hexpand(true);
    rec_title.set_halign(gtk4::Align::Start);
    rec_box.append(&rec_title);

    let rec_val = Label::new(Some(hw.recommended_tier.name_tr()));
    rec_val.add_css_class("status-badge-info");
    rec_box.append(&rec_val);
    hw_sec.append(&rec_box);

    tab.append(&hw_sec);

    // 2. Profile Selection Card
    let prof_sec = GtkBox::new(Orientation::Vertical, 12);
    prof_sec.add_css_class("section-box");

    let prof_title = Label::new(Some("Performans profili"));
    prof_title.add_css_class("section-title");
    prof_title.set_halign(gtk4::Align::Start);
    prof_sec.append(&prof_title);

    let prof_sub = Label::new(Some(
        "Bulanıklık ve geçiş süreleri donanıma göre ayarlanır; ekran yenileme hızı monitörün yerel frekansında kalır",
    ));
    prof_sub.add_css_class("setting-subtext");
    prof_sub.set_wrap(true);
    prof_sub.set_xalign(0.0);
    prof_sub.set_halign(gtk4::Align::Start);
    prof_sec.append(&prof_sub);

    let tier_items = StringList::new(&[
        "Hafif (bulanıklık ve gölge kapalı, eski donanım)",
        "Dengeli (standart geçişler, hafif bulanıklık)",
        "Yüksek (tam bulanıklık ve derinlik efektleri)",
    ]);
    let tier_dropdown = DropDown::new(Some(tier_items), None::<gtk4::Expression>);

    match current_profile.tier {
        PerformanceTier::Legacy => tier_dropdown.set_selected(0),
        PerformanceTier::Balanced => tier_dropdown.set_selected(1),
        PerformanceTier::Ultra => tier_dropdown.set_selected(2),
    }

    tier_dropdown.connect_selected_notify(|dd| {
        let tier = match dd.selected() {
            0 => PerformanceTier::Legacy,
            1 => PerformanceTier::Balanced,
            _ => PerformanceTier::Ultra,
        };
        let p = HardwareTuningProfile::for_tier(tier);
        let _ = p.save();
        restart_taskbar_if_enabled();
    });

    prof_sec.append(&tier_dropdown);
    tab.append(&prof_sec);

    // 3. Stability & Resource Optimization Card
    let ash_sec = GtkBox::new(Orientation::Vertical, 12);
    ash_sec.add_css_class("section-box");

    let ash_title = Label::new(Some("Kararlılık ve kaynak yönetimi"));
    ash_title.add_css_class("section-title");
    ash_title.set_halign(gtk4::Align::Start);
    ash_sec.append(&ash_title);

    let ash_sub = Label::new(Some(
        "Çöken kabuk süreçlerini otomatik yeniden başlatır ve boşta arka plan yükünü azaltır",
    ));
    ash_sub.add_css_class("setting-subtext");
    ash_sub.set_wrap(true);
    ash_sub.set_xalign(0.0);
    ash_sub.set_halign(gtk4::Align::Start);
    ash_sec.append(&ash_sub);

    // Watchdog Auto-Healing
    let chk_watchdog = CheckButton::with_label(
        "Çöken arayüz süreçlerini 3 sn içinde otomatik yeniden başlat",
    );
    chk_watchdog.set_active(current_profile.watchdog_enabled);
    chk_watchdog.connect_toggled(|cb| {
        let mut p = HardwareTuningProfile::load();
        p.watchdog_enabled = cb.is_active();
        let _ = p.save();
    });
    ash_sec.append(&chk_watchdog);

    // Idle Resource Throttling
    let chk_idle = CheckButton::with_label("Boşta arka plan yoklamasını ve çizimi azalt");
    chk_idle.set_active(current_profile.idle_throttling_enabled);
    chk_idle.connect_toggled(|cb| {
        let mut p = HardwareTuningProfile::load();
        p.idle_throttling_enabled = cb.is_active();
        let _ = p.save();
    });
    ash_sec.append(&chk_idle);

    // OOM Killer Shield
    let chk_oom = CheckButton::with_label(
        "Bellek baskısında pencere yöneticisi ve kabuk süreçlerini koru",
    );
    chk_oom.set_active(current_profile.oom_shield_enabled);
    chk_oom.connect_toggled(|cb| {
        let mut p = HardwareTuningProfile::load();
        p.oom_shield_enabled = cb.is_active();
        let _ = p.save();
        apply_oom_shield_live(cb.is_active());
    });
    ash_sec.append(&chk_oom);

    tab.append(&ash_sec);

    tab
}

pub fn apply_oom_shield_live(enabled: bool) {
    let score = if enabled { "-500" } else { "0" };
    // Apply to current process first
    let _ = std::fs::write("/proc/self/oom_score_adj", score);
    // Apply to core desktop processes owned by user or via privileged helper
    for proc_name in ["niri", "solar-shell", "noctalia", "quickshell", "solar-core"] {
        if let Ok(out) = Command::new("pgrep").args(["-x", proc_name]).output() {
            if let Ok(pids) = String::from_utf8(out.stdout) {
                for pid in pids.lines().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    let path = format!("/proc/{}/oom_score_adj", pid);
                    if std::fs::write(&path, score).is_err() {
                        let _ = Command::new("sudo")
                            .args(["-n", "sh", "-c", &format!("echo {} > {}", score, path)])
                            .status();
                    }
                }
            }
        }
    }
}

fn parse_sentinel_posture_counts(val: &serde_json::Value) -> (u64, u64) {
    let passed = val
        .get("passed")
        .or_else(|| val.get("passed_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let total = val
        .get("total")
        .or_else(|| val.get("total_count"))
        .and_then(|v| v.as_u64())
        .unwrap_or(7);
    (passed, total)
}

fn probe_live_health_components() -> Vec<(&'static str, String, &'static str)> {
    // 1. SolarCore / Daemon check
    let xdg_runtime = std::env::var("XDG_RUNTIME_DIR")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| format!("/run/user/{}", solar_common::current_uid()));
    let canonical_sock = solar_common::get_solar_core_socket_path();
    let sock_paths = [
        canonical_sock,
        PathBuf::from(format!("{}/solar.sock", xdg_runtime)),
        PathBuf::from(format!("{}/solar-core.sock", xdg_runtime)),
        PathBuf::from("/tmp/solar-core.sock"),
    ];
    let has_sock = sock_paths.iter().any(|p| p.exists());
    let has_core_proc = Command::new("pgrep")
        .args(["-f", "solar-core|solar-daemon"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let core_status = if has_sock {
        ("Çalışıyor (IPC soketi)".to_string(), "status-dot-ok")
    } else if has_core_proc {
        ("Çalışıyor".to_string(), "status-dot-ok")
    } else {
        ("Beklemede".to_string(), "status-dot-muted")
    };

    // 2. Active Shell Engine check
    let shell_status = if Command::new("pgrep").args(["-x", "noctalia"]).output().map(|o| o.status.success()).unwrap_or(false) {
        ("Etkin (Noctalia)".to_string(), "status-dot-ok")
    } else if Command::new("pgrep").args(["-f", "quickshell"]).output().map(|o| o.status.success()).unwrap_or(false) {
        ("Etkin (Quickshell)".to_string(), "status-dot-ok")
    } else {
        ("Etkin (SolarShell)".to_string(), "status-dot-ok")
    };

    // 3. Wayland Compositor (Niri) check
    let niri_alive = std::env::var("NIRI_SOCKET")
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false)
        || Command::new("pgrep").args(["-x", "niri"]).output().map(|o| o.status.success()).unwrap_or(false);
    let wm_status = if niri_alive {
        ("Etkin (Niri Wayland)".to_string(), "status-dot-ok")
    } else {
        ("Wayland oturumu".to_string(), "status-dot-muted")
    };

    // 4. Blaze Sentinel Live Security Audit
    let sentinel_status = if let Ok(out) = Command::new("blaze-sentinel").args(["status", "--json"]).output() {
        if let Ok(txt) = String::from_utf8(out.stdout) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                let (passed, total) = parse_sentinel_posture_counts(&val);
                (
                    format!("{}/{} güvenlik kontrolü geçti", passed, total),
                    if passed == total { "status-dot-ok" } else { "status-dot-warn" },
                )
            } else {
                ("Etkin".to_string(), "status-dot-ok")
            }
        } else {
            ("Etkin".to_string(), "status-dot-ok")
        }
    } else {
        ("Hazır".to_string(), "status-dot-muted")
    };

    vec![
        ("SolarCore servisi", core_status.0, core_status.1),
        ("Masaüstü kabuğu", shell_status.0, shell_status.1),
        ("Pencere yöneticisi", wm_status.0, wm_status.1),
        ("Blaze Sentinel", sentinel_status.0, sentinel_status.1),
    ]
}

fn build_health_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Sistem sağlığı",
        "Temel masaüstü süreçleri, USB depolama kilidi ve kurtarma testi",
    ));

    // 1. Health Status Card
    let stat_sec = GtkBox::new(Orientation::Vertical, 12);
    stat_sec.add_css_class("section-box");

    let stat_title = Label::new(Some("Bileşen durumu"));
    stat_title.add_css_class("section-title");
    stat_title.set_halign(gtk4::Align::Start);
    stat_sec.append(&stat_title);

    let stat_desc = Label::new(Some(
        "Çalışan oturum bileşenlerinin ve Blaze Sentinel denetimlerinin anlık durumu",
    ));
    stat_desc.add_css_class("setting-subtext");
    stat_desc.set_wrap(true);
    stat_desc.set_xalign(0.0);
    stat_desc.set_halign(gtk4::Align::Start);
    stat_sec.append(&stat_desc);

    let placeholder_names = [
        "SolarCore servisi",
        "Masaüstü kabuğu",
        "Pencere yöneticisi",
        "Blaze Sentinel",
    ];
    let mut status_widgets: Vec<(GtkBox, Label)> = Vec::new();

    for comp in placeholder_names {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.set_margin_top(4);
        row.set_margin_bottom(4);

        let c_lbl = Label::new(Some(comp));
        c_lbl.add_css_class("setting-label");
        c_lbl.set_hexpand(true);
        c_lbl.set_halign(gtk4::Align::Start);
        row.append(&c_lbl);

        let state_box = GtkBox::new(Orientation::Horizontal, 8);
        state_box.set_halign(gtk4::Align::End);
        state_box.set_valign(gtk4::Align::Center);

        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("status-dot");
        dot.add_css_class("status-dot-muted");
        dot.set_size_request(8, 8);
        dot.set_halign(gtk4::Align::Center);
        dot.set_valign(gtk4::Align::Center);
        dot.set_hexpand(false);
        dot.set_vexpand(false);
        state_box.append(&dot);

        let s_lbl = Label::new(Some("Kontrol ediliyor..."));
        s_lbl.add_css_class("status-text");
        s_lbl.set_halign(gtk4::Align::End);
        state_box.append(&s_lbl);

        row.append(&state_box);
        stat_sec.append(&row);
        status_widgets.push((dot, s_lbl));
    }

    // Run subprocess health checks off the GTK startup thread so window.present() is instant
    let (health_tx, health_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = health_tx.send(probe_live_health_components());
    });
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        match health_rx.try_recv() {
            Ok(results) => {
                for ((dot, s_lbl), (_comp, status, dot_cls)) in
                    status_widgets.iter().zip(results.into_iter())
                {
                    dot.remove_css_class("status-dot-muted");
                    dot.remove_css_class("status-dot-ok");
                    dot.remove_css_class("status-dot-warn");
                    dot.add_css_class(dot_cls);
                    s_lbl.set_text(&status);
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });

    tab.append(&stat_sec);

    // 1.5 USB Storage Guard
    let usb_sec = GtkBox::new(Orientation::Vertical, 12);
    usb_sec.add_css_class("section-box");

    let usb_title = Label::new(Some("USB depolama kilidi"));
    usb_title.add_css_class("section-title");
    usb_title.set_halign(gtk4::Align::Start);
    usb_sec.append(&usb_title);

    let usb_desc = Label::new(Some(
        "Yeni takılan USB belleklerin çekirdek düzeyinde bağlanmasını engeller",
    ));
    usb_desc.add_css_class("setting-subtext");
    usb_desc.set_wrap(true);
    usb_desc.set_xalign(0.0);
    usb_desc.set_halign(gtk4::Align::Start);
    usb_sec.append(&usb_desc);

    let usb_locked = std::fs::read_to_string("/etc/modprobe.d/blaze-usb-guard.conf")
        .map(|c| c.contains("install usb-storage /bin/true"))
        .unwrap_or(false);
    let chk_usb_guard = CheckButton::with_label("Yeni USB depolama aygıtlarını engelle");
    chk_usb_guard.set_active(usb_locked);
    chk_usb_guard.connect_toggled(|cb| {
        let lock = cb.is_active();
        std::thread::spawn(move || {
            let cmd_str = if lock {
                "printf 'install usb-storage /bin/true\\nblacklist usb-storage\\n' > /etc/modprobe.d/blaze-usb-guard.conf"
            } else {
                "rm -f /etc/modprobe.d/blaze-usb-guard.conf && modprobe usb-storage 2>/dev/null || true"
            };
            let _ = Command::new("sudo").args(["-n", "sh", "-c", cmd_str]).status();
        });
    });
    usb_sec.append(&chk_usb_guard);

    let usb_note_row = GtkBox::new(Orientation::Horizontal, 8);
    let lock_icon = create_symbolic_icon("changes-prevent-symbolic", "settings-row-icon");
    usb_note_row.append(&lock_icon);
    let usb_note = Label::new(Some("Günlük kullanım için kapalı tutmanız önerilir"));
    usb_note.add_css_class("settings-meta");
    usb_note.set_halign(gtk4::Align::Start);
    usb_note_row.append(&usb_note);
    usb_sec.append(&usb_note_row);

    tab.append(&usb_sec);

    // 2. Self-Healing Crash Test Card
    let test_sec = GtkBox::new(Orientation::Vertical, 12);
    test_sec.add_css_class("section-box");

    let test_title = Label::new(Some("Kurtarma testi"));
    test_title.add_css_class("section-title");
    test_title.set_halign(gtk4::Align::Start);
    test_sec.append(&test_title);

    let test_info = Label::new(Some(
        "Görev çubuğu sürecini sonlandırıp otomatik yeniden başlatmayı doğrular; açık pencereleriniz etkilenmez",
    ));
    test_info.add_css_class("setting-subtext");
    test_info.set_wrap(true);
    test_info.set_xalign(0.0);
    test_info.set_halign(gtk4::Align::Start);
    test_sec.append(&test_info);

    let crash_btn = Button::with_label("Kabuk kurtarmayı test et");
    crash_btn.add_css_class("danger-btn");
    crash_btn.set_halign(gtk4::Align::Start);
    crash_btn.connect_clicked(|_| {
        std::thread::spawn(|| {
            // Kill solar-shell run instance
            let _ = Command::new("pkill")
                .args(["-9", "-f", "solar-shell (run|gui|taskbar|window)"])
                .status();
            std::thread::sleep(std::time::Duration::from_millis(150));
            // Ensure watchdog brings it back
            let _ = Command::new("solar-shell").arg("run").spawn();
        });
    });
    test_sec.append(&crash_btn);

    tab.append(&test_sec);

    tab
}

fn build_pixel_clock_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Masaüstü saati",
        "Duvar kağıdı üzerinde çalışan saat bileşeninin stilini ve renklerini ayarlayın",
    ));

    // Section 1: Style Selection
    let style_sec = GtkBox::new(Orientation::Vertical, 12);
    style_sec.add_css_class("section-box");

    let s_title = Label::new(Some("Saat stili"));
    s_title.add_css_class("section-title");
    s_title.set_halign(gtk4::Align::Start);
    style_sec.append(&s_title);

    let s_sub = Label::new(Some("Masaüstü saat görünümü için dijital veya analog kadran seçin"));
    s_sub.add_css_class("setting-subtext");
    s_sub.set_wrap(true);
    s_sub.set_xalign(0.0);
    s_sub.set_halign(gtk4::Align::Start);
    style_sec.append(&s_sub);

    let style_choices = StringList::new(&[
        "Kalın yatay (saat, pil ve tarih)",
        "Dikey iki satır (büyük rakamlar)",
        "İçi boş kontur",
        "Analog kalın (12, 3, 6, 9)",
        "Analog kontur",
        "Analog takimetre",
    ]);
    let style_dd = DropDown::new(Some(style_choices), None::<gtk4::Expression>);

    let clock_cfg = PixelClockConfig::load();
    let initial_idx = match clock_cfg.style {
        PixelClockStyle::BoldHorizontal => 0,
        PixelClockStyle::StackedVertical => 1,
        PixelClockStyle::Outline => 2,
        PixelClockStyle::AnalogBold => 3,
        PixelClockStyle::AnalogOutline => 4,
        PixelClockStyle::AnalogTachymeter => 5,
    };
    style_dd.set_selected(initial_idx);
    style_dd.connect_selected_notify(|dd| {
        let mut cfg = PixelClockConfig::load();
        cfg.style = match dd.selected() {
            0 => PixelClockStyle::BoldHorizontal,
            1 => PixelClockStyle::StackedVertical,
            2 => PixelClockStyle::Outline,
            3 => PixelClockStyle::AnalogBold,
            4 => PixelClockStyle::AnalogOutline,
            _ => PixelClockStyle::AnalogTachymeter,
        };
        let _ = cfg.save();
    });
    style_sec.append(&style_dd);
    tab.append(&style_sec);

    // Section 2: Colors & Material You
    let color_sec = GtkBox::new(Orientation::Vertical, 12);
    color_sec.add_css_class("section-box");

    let c_title = Label::new(Some("Renklendirme"));
    c_title.add_css_class("section-title");
    c_title.set_halign(gtk4::Align::Start);
    color_sec.append(&c_title);

    let color_choices = StringList::new(&[
        "Dinamik (Material You, duvar kağıdına uyumlu)",
        "Açık mavi (#80d4ff)",
        "Lavanta (#c084fc)",
        "Nane yeşili (#4ade80)",
        "Mercan (#fb923c)",
        "Beyaz (#ffffff)",
    ]);
    let color_dd = DropDown::new(Some(color_choices), None::<gtk4::Expression>);
    let color_idx = if clock_cfg.color_mode == "dynamic" {
        0
    } else {
        match clock_cfg.custom_color.as_str() {
            "#80d4ff" => 1,
            "#c084fc" => 2,
            "#4ade80" => 3,
            "#fb923c" => 4,
            _ => 5,
        }
    };
    color_dd.set_selected(color_idx);
    color_dd.connect_selected_notify(|dd| {
        let mut cfg = PixelClockConfig::load();
        match dd.selected() {
            0 => {
                cfg.color_mode = "dynamic".to_string();
            }
            1 => {
                cfg.color_mode = "custom".to_string();
                cfg.custom_color = "#80d4ff".to_string();
            }
            2 => {
                cfg.color_mode = "custom".to_string();
                cfg.custom_color = "#c084fc".to_string();
            }
            3 => {
                cfg.color_mode = "custom".to_string();
                cfg.custom_color = "#4ade80".to_string();
            }
            4 => {
                cfg.color_mode = "custom".to_string();
                cfg.custom_color = "#fb923c".to_string();
            }
            _ => {
                cfg.color_mode = "custom".to_string();
                cfg.custom_color = "#ffffff".to_string();
            }
        }
        let _ = cfg.save();
    });
    color_sec.append(&color_dd);
    tab.append(&color_sec);

    // Section 3: Opacity, Battery Pill, Date
    let opts_sec = GtkBox::new(Orientation::Vertical, 12);
    opts_sec.add_css_class("section-box");

    let o_title = Label::new(Some("Opaklık ve göstergeler"));
    o_title.add_css_class("section-title");
    o_title.set_halign(gtk4::Align::Start);
    opts_sec.append(&o_title);

    let row1 = GtkBox::new(Orientation::Horizontal, 12);
    let op_lbl = Label::new(Some("Rakam opaklığı"));
    op_lbl.add_css_class("setting-label");
    op_lbl.set_hexpand(true);
    op_lbl.set_halign(gtk4::Align::Start);
    row1.append(&op_lbl);

    let op_choices = StringList::new(&[
        "%100 (tam opak)",
        "%95 (önerilen)",
        "%80 (hafif saydam)",
        "%60 (yarı saydam)",
        "%40 (düşük)",
    ]);
    let op_dd = DropDown::new(Some(op_choices), None::<gtk4::Expression>);
    let op_idx = if clock_cfg.numeral_opacity >= 0.98 {
        0
    } else if clock_cfg.numeral_opacity >= 0.88 {
        1
    } else if clock_cfg.numeral_opacity >= 0.70 {
        2
    } else if clock_cfg.numeral_opacity >= 0.50 {
        3
    } else {
        4
    };
    op_dd.set_selected(op_idx);
    op_dd.connect_selected_notify(|dd| {
        let mut cfg = PixelClockConfig::load();
        cfg.numeral_opacity = match dd.selected() {
            0 => 1.0,
            1 => 0.95,
            2 => 0.80,
            3 => 0.60,
            _ => 0.40,
        };
        let _ = cfg.save();
    });
    row1.append(&op_dd);
    opts_sec.append(&row1);

    let row2 = GtkBox::new(Orientation::Horizontal, 20);
    let bat_chk = CheckButton::with_label("Pil göstergesini göster");
    bat_chk.set_active(clock_cfg.show_battery_pill);
    bat_chk.connect_toggled(|cb| {
        let mut cfg = PixelClockConfig::load();
        cfg.show_battery_pill = cb.is_active();
        let _ = cfg.save();
    });
    row2.append(&bat_chk);

    let date_chk = CheckButton::with_label("Tarihi göster");
    date_chk.set_active(clock_cfg.show_date);
    date_chk.connect_toggled(|cb| {
        let mut cfg = PixelClockConfig::load();
        cfg.show_date = cb.is_active();
        let _ = cfg.save();
    });
    row2.append(&date_chk);
    opts_sec.append(&row2);
    tab.append(&opts_sec);

    // Section 4: Action Buttons (Launch / Restart widget)
    let act_sec = GtkBox::new(Orientation::Horizontal, 8);
    act_sec.set_halign(gtk4::Align::Start);

    let launch_btn = Button::with_label("Saati başlat");
    launch_btn.add_css_class("action-btn");
    launch_btn.connect_clicked(|_| {
        let _ = Command::new("pkill").args(["-f", "solar-shell (clock|pixel-clock)"]).status();
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let _ = Command::new("solar-shell").arg("clock").spawn();
        });
    });
    act_sec.append(&launch_btn);
    tab.append(&act_sec);

    tab
}

fn build_taskbar_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Görev çubuğu",
        "Noctalia görev çubuğunun yüzey stilini, saydamlığını ve boyutunu ayarlayın",
    ));

    // Section 1: Appearance Style Presets
    let preset_sec = GtkBox::new(Orientation::Vertical, 12);
    preset_sec.add_css_class("section-box");

    let p_title = Label::new(Some("Görünüm stili"));
    p_title.add_css_class("section-title");
    p_title.set_halign(gtk4::Align::Start);
    preset_sec.append(&p_title);

    let p_sub = Label::new(Some(
        "Düz mat, buzlu cam, tam saydam veya duvar kağıdına uyarlanan çubuk stilini seçin",
    ));
    p_sub.add_css_class("setting-subtext");
    p_sub.set_wrap(true);
    p_sub.set_xalign(0.0);
    p_sub.set_halign(gtk4::Align::Start);
    preset_sec.append(&p_sub);

    let preset_choices = StringList::new(&[
        "Buzlu cam (%32 saydamlık)",
        "Düz mat (%100 opak)",
        "Tam saydam (yalnızca simgeler)",
        "Material 3 dinamik renk",
    ]);
    let preset_dd = DropDown::new(Some(preset_choices), None::<gtk4::Expression>);
    preset_dd.set_selected(0);
    preset_dd.connect_selected_notify(|dd| {
        match dd.selected() {
            0 => apply_taskbar_preset("glass"),
            1 => apply_taskbar_preset("solid"),
            2 => apply_taskbar_preset("transparent"),
            _ => apply_taskbar_preset("m3-adaptive"),
        }
    });
    preset_sec.append(&preset_dd);
    tab.append(&preset_sec);

    // Section 2: Fine-grained Customization (Opacity, Corner Radius, Height)
    let fine_sec = GtkBox::new(Orientation::Vertical, 12);
    fine_sec.add_css_class("section-box");

    let f_title = Label::new(Some("Boyut ve saydamlık"));
    f_title.add_css_class("section-title");
    f_title.set_halign(gtk4::Align::Start);
    fine_sec.append(&f_title);

    // Opacity
    let op_row = GtkBox::new(Orientation::Horizontal, 16);
    let op_l = Label::new(Some("Saydamlık"));
    op_l.add_css_class("setting-label");
    op_l.set_hexpand(true);
    op_l.set_halign(gtk4::Align::Start);
    op_row.append(&op_l);

    let op_list = StringList::new(&[
        "%10 (çok saydam)",
        "%25 (hafif saydam)",
        "%32 (varsayılan)",
        "%50 (yarı saydam)",
        "%75 (koyu cam)",
        "%100 (mat)",
    ]);
    let op_dd = DropDown::new(Some(op_list), None::<gtk4::Expression>);
    op_dd.set_selected(2);
    op_dd.connect_selected_notify(|dd| {
        let op = match dd.selected() {
            0 => 0.10,
            1 => 0.25,
            2 => 0.32,
            3 => 0.50,
            4 => 0.75,
            _ => 1.0,
        };
        update_noctalia_taskbar_opacity(op);
    });
    op_row.append(&op_dd);
    fine_sec.append(&op_row);

    // Corner Radius
    let rad_row = GtkBox::new(Orientation::Horizontal, 16);
    let rad_l = Label::new(Some("Köşe yarıçapı"));
    rad_l.add_css_class("setting-label");
    rad_l.set_hexpand(true);
    rad_l.set_halign(gtk4::Align::Start);
    rad_row.append(&rad_l);

    let rad_list = StringList::new(&[
        "0 px (keskin)",
        "8 px (hafif yuvarlak)",
        "16 px (varsayılan)",
        "24 px (yuvarlak)",
        "40 px (kapsül)",
    ]);
    let rad_dd = DropDown::new(Some(rad_list), None::<gtk4::Expression>);
    rad_dd.set_selected(2);
    rad_dd.connect_selected_notify(|dd| {
        let r = match dd.selected() {
            0 => 0,
            1 => 8,
            2 => 16,
            3 => 24,
            _ => 40,
        };
        update_noctalia_taskbar_radius(r);
    });
    rad_row.append(&rad_dd);
    fine_sec.append(&rad_row);

    // Height / Thickness
    let h_row = GtkBox::new(Orientation::Horizontal, 16);
    let h_l = Label::new(Some("Çubuk yüksekliği"));
    h_l.add_css_class("setting-label");
    h_l.set_hexpand(true);
    h_l.set_halign(gtk4::Align::Start);
    h_row.append(&h_l);

    let h_list = StringList::new(&[
        "36 px (kompakt)",
        "40 px (varsayılan)",
        "44 px (rahat)",
        "48 px (büyük)",
        "56 px (geniş)",
    ]);
    let h_dd = DropDown::new(Some(h_list), None::<gtk4::Expression>);
    h_dd.set_selected(1);
    h_dd.connect_selected_notify(|dd| {
        let th = match dd.selected() {
            0 => 36,
            1 => 40,
            2 => 44,
            3 => 48,
            _ => 56,
        };
        update_noctalia_taskbar_thickness(th);
    });
    h_row.append(&h_dd);
    fine_sec.append(&h_row);

    tab.append(&fine_sec);

    // Section 3: Open Full Noctalia Bar Settings Button
    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::Start);
    let full_btn = Button::with_label("Çubuk düzenleyiciyi aç");
    full_btn.add_css_class("action-btn");
    full_btn.connect_clicked(|_| {
        let _ = Command::new("noctalia").args(["msg", "settings-open", "bar"]).spawn();
    });
    btn_row.append(&full_btn);
    tab.append(&btn_row);

    tab
}

fn noctalia_settings_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    format!("{}/.local/state/noctalia/settings.toml", home)
}

fn noctalia_json_settings_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    format!("{}/.config/noctalia/settings.json", home)
}

fn ensure_noctalia_taskbar_tables(doc: &mut toml::Value) {
    if !doc.is_table() {
        *doc = toml::Value::Table(toml::map::Map::new());
    }
    if let Some(root) = doc.as_table_mut() {
        let bar = root
            .entry("bar".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(bt) = bar.as_table_mut() {
            bt.entry("taskbar".to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        }
        let widget = root
            .entry("widget".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(wt) = widget.as_table_mut() {
            wt.entry("taskbar".to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        }
    }
}

fn load_or_create_noctalia_toml() -> toml::Value {
    let path = noctalia_settings_path();
    if let Some(parent) = std::path::Path::new(&path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut doc = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));
    ensure_noctalia_taskbar_tables(&mut doc);
    doc
}

fn save_noctalia_toml(doc: &toml::Value) {
    let path = noctalia_settings_path();
    if let Ok(serialized) = toml::to_string_pretty(doc) {
        let _ = std::fs::write(&path, serialized);
        let _ = Command::new("noctalia").args(["msg", "templates-apply"]).spawn();
    }
}

fn sync_noctalia_json_bar(opacity: Option<f64>) {
    let jpath = noctalia_json_settings_path();
    if let Ok(content) = std::fs::read_to_string(&jpath) {
        if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(op) = opacity {
                if let Some(bar) = val.get_mut("bar").and_then(|b| b.as_object_mut()) {
                    bar.insert("backgroundOpacity".to_string(), serde_json::json!(op));
                }
            }
            if let Ok(out) = serde_json::to_string_pretty(&val) {
                let _ = std::fs::write(&jpath, out);
            }
        }
    }
}

fn apply_taskbar_preset(preset: &str) {
    let mut doc = load_or_create_noctalia_toml();
    let (op, border, radius) = match preset {
        "glass" => (0.32, "outline", 16),
        "solid" => (1.0, "none", 12),
        "transparent" => (0.0, "none", 16),
        _ => (0.42, "outline", 20),
    };
    for sec in ["bar", "widget"] {
        if let Some(t) = doc
            .get_mut(sec)
            .and_then(|b| b.get_mut("taskbar"))
            .and_then(|tb| tb.as_table_mut())
        {
            t.insert("background_opacity".to_string(), toml::Value::Float(op));
            t.insert("border".to_string(), toml::Value::String(border.to_string()));
            t.insert("radius".to_string(), toml::Value::Integer(radius));
        }
    }
    save_noctalia_toml(&doc);
    sync_noctalia_json_bar(Some(op));
}

fn update_noctalia_taskbar_opacity(opacity: f64) {
    let mut doc = load_or_create_noctalia_toml();
    for sec in ["bar", "widget"] {
        if let Some(t) = doc
            .get_mut(sec)
            .and_then(|b| b.get_mut("taskbar"))
            .and_then(|tb| tb.as_table_mut())
        {
            t.insert("background_opacity".to_string(), toml::Value::Float(opacity));
        }
    }
    save_noctalia_toml(&doc);
    sync_noctalia_json_bar(Some(opacity));
}

fn update_noctalia_taskbar_radius(radius: i32) {
    let mut doc = load_or_create_noctalia_toml();
    for sec in ["bar", "widget"] {
        if let Some(t) = doc
            .get_mut(sec)
            .and_then(|b| b.get_mut("taskbar"))
            .and_then(|tb| tb.as_table_mut())
        {
            t.insert("radius".to_string(), toml::Value::Integer(radius as i64));
        }
    }
    save_noctalia_toml(&doc);
}

fn update_noctalia_taskbar_thickness(thickness: i32) {
    let mut doc = load_or_create_noctalia_toml();
    for sec in ["bar", "widget"] {
        if let Some(t) = doc
            .get_mut(sec)
            .and_then(|b| b.get_mut("taskbar"))
            .and_then(|tb| tb.as_table_mut())
        {
            t.insert("thickness".to_string(), toml::Value::Integer(thickness as i64));
        }
    }
    save_noctalia_toml(&doc);
}

fn update_noctalia_shell_settings(engine: &str, launcher: &str, dash: &str) {
    let path = noctalia_settings_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        if let Ok(mut doc) = content.parse::<toml::Value>() {
            if let Some(table) = doc.as_table_mut() {
                if !table.contains_key("solarui") {
                    table.insert("solarui".to_string(), toml::Value::Table(toml::map::Map::new()));
                }
                if let Some(s) = table.get_mut("solarui").and_then(|v| v.as_table_mut()) {
                    s.insert("shell_engine".to_string(), toml::Value::String(engine.to_string()));
                    s.insert("launcher_provider".to_string(), toml::Value::String(launcher.to_string()));
                    s.insert("dashboard_provider".to_string(), toml::Value::String(dash.to_string()));
                }
            }
            if let Ok(serialized) = toml::to_string_pretty(&doc) {
                let _ = std::fs::write(&path, serialized);
            }
        }
    }
}

fn build_shell_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Masaüstü kabuğu",
        "Noctalia, Caelestia, end4-pC ve We10X kabukları arasında geçiş yapın",
    ));

    let sec = GtkBox::new(Orientation::Vertical, 12);
    sec.add_css_class("section-box");

    let title = Label::new(Some("Kabuk motoru"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Her kabuk kendi üst çubuğunu, bildirimlerini ve sistem tepsisini bağımsız olarak yönetir",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    let cfg = SolarConfig::load();

    // 1. Shell Engine Selector
    let row1 = GtkBox::new(Orientation::Horizontal, 12);
    let lbl1 = Label::new(Some("Etkin kabuk"));
    lbl1.add_css_class("setting-label");
    lbl1.set_hexpand(true);
    lbl1.set_halign(gtk4::Align::Start);
    row1.append(&lbl1);

    let engines = [
        "Noctalia v5",
        "Caelestia (Quickshell)",
        "end4-pC (Material 3)",
        "We10X (klasik düzen)",
    ];
    let engine_model = StringList::new(&engines);
    let engine_dd = DropDown::new(Some(engine_model), None::<gtk4::Expression>);
    let active_idx = match cfg.shell.engine {
        ShellEngine::Noctalia => 0,
        ShellEngine::Caelestia => 1,
        ShellEngine::End4Pc => 2,
        ShellEngine::We10x => 3,
    };
    engine_dd.set_selected(active_idx);
    row1.append(&engine_dd);
    sec.append(&row1);

    // 2. Action Buttons
    let btn_box = GtkBox::new(Orientation::Horizontal, 8);
    btn_box.set_margin_top(4);

    let apply_btn = Button::with_label("Kabuğu değiştir");
    apply_btn.add_css_class("action-btn");
    apply_btn.set_halign(gtk4::Align::Start);

    let engine_dd_clone = engine_dd.clone();

    let shell_status_lbl = Label::new(None);
    shell_status_lbl.add_css_class("status-text");
    shell_status_lbl.set_halign(gtk4::Align::Start);
    shell_status_lbl.set_visible(false);

    let shell_status_clone = shell_status_lbl.clone();
    apply_btn.connect_clicked(move |_| {
        let target_engine = match engine_dd_clone.selected() {
            0 => ShellEngine::Noctalia,
            1 => ShellEngine::Caelestia,
            2 => ShellEngine::End4Pc,
            _ => ShellEngine::We10x,
        };
        let target_provider = match target_engine {
            ShellEngine::Noctalia => SurfaceProvider::Noctalia,
            ShellEngine::Caelestia => SurfaceProvider::Caelestia,
            ShellEngine::End4Pc => SurfaceProvider::End4Pc,
            ShellEngine::We10x => SurfaceProvider::We10x,
        };

        // Do NOT overwrite cfg.shell.engine before running `solar-shell switch <engine>`
        // so `switcher.rs` knows the true previous engine and can roll back on failure!
        update_noctalia_shell_settings(
            target_engine.as_str(),
            target_provider.as_str(),
            target_provider.as_str(),
        );

        let engine_str = target_engine.as_str();
        shell_status_clone.set_text(&format!(
            "{} kabuğuna geçiş başlatıldı",
            engine_str
        ));
        shell_status_clone.set_visible(true);

        let engine_to_switch = engine_str.to_string();
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["switch", &engine_to_switch])
                .status();
        });
    });
    btn_box.append(&apply_btn);

    let reconcile_btn = Button::with_label("Durumu denetle");
    reconcile_btn.add_css_class("secondary-btn");
    reconcile_btn.set_halign(gtk4::Align::Start);
    let shell_status_rec = shell_status_lbl.clone();
    reconcile_btn.connect_clicked(move |_| {
        shell_status_rec.set_text("Kabuk durumu denetleniyor");
        shell_status_rec.set_visible(true);
        std::thread::spawn(|| {
            let _ = Command::new("solar-shell")
                .args(["supervisor"])
                .status();
        });
    });
    btn_box.append(&reconcile_btn);

    let end4_settings_btn = Button::with_label("end4-pC ayarlarını aç");
    end4_settings_btn.add_css_class("secondary-btn");
    end4_settings_btn.set_halign(gtk4::Align::Start);
    let shell_status_end4 = shell_status_lbl.clone();
    end4_settings_btn.connect_clicked(move |_| {
        let cfg = SolarConfig::load();
        if cfg.shell.engine != ShellEngine::End4Pc {
            shell_status_end4.set_text("end4-pC ayarları için önce end4-pC kabuğunu etkinleştirin");
            shell_status_end4.set_visible(true);
            return;
        }
        shell_status_end4.set_text("end4-pC ayarları açılıyor");
        shell_status_end4.set_visible(true);
        std::thread::spawn(|| {
            let _ = Command::new("solar-shell")
                .args(["route", "settings"])
                .status();
        });
    });
    btn_box.append(&end4_settings_btn);

    sec.append(&btn_box);
    sec.append(&shell_status_lbl);
    tab.append(&sec);

    // 3. We10X Customization Card
    tab.append(&build_we10x_customization_card());

    tab
}

fn build_we10x_customization_card() -> GtkBox {
    let cfg = SolarConfig::load();
    let we10x_sec = GtkBox::new(Orientation::Vertical, 12);
    we10x_sec.add_css_class("section-box");

    let we10x_title = Label::new(Some("We10X görünümü"));
    we10x_title.add_css_class("section-title");
    we10x_title.set_halign(gtk4::Align::Start);
    we10x_sec.append(&we10x_title);

    let we10x_desc = Label::new(Some(
        "Windows 10 klasik modunun tema tonunu, vurgu rengini, görev çubuğu saydamlığını ve simgelerini ayarlayın",
    ));
    we10x_desc.add_css_class("setting-subtext");
    we10x_desc.set_wrap(true);
    we10x_desc.set_xalign(0.0);
    we10x_desc.set_halign(gtk4::Align::Start);
    we10x_sec.append(&we10x_desc);

    let status_lbl = Label::new(None);
    status_lbl.add_css_class("status-text");
    status_lbl.set_halign(gtk4::Align::Start);
    status_lbl.set_visible(false);

    // 1. Theme Mode: Dark / Light / Pure Black OLED
    let theme_row = GtkBox::new(Orientation::Horizontal, 12);
    let theme_lbl = Label::new(Some("Tema modu"));
    theme_lbl.add_css_class("setting-label");
    theme_lbl.set_hexpand(true);
    theme_lbl.set_halign(gtk4::Align::Start);
    theme_row.append(&theme_lbl);

    let we10x_themes = [
        "Koyu",
        "Açık",
        "Tam siyah (OLED)",
    ];
    let we10x_theme_dd = DropDown::new(Some(StringList::new(&we10x_themes)), None::<gtk4::Expression>);
    let init_theme_idx = if cfg.shell.we10x_theme_mode.eq_ignore_ascii_case("light") {
        1
    } else if cfg.shell.we10x_theme_mode.eq_ignore_ascii_case("oled")
        || cfg.shell.we10x_theme_mode.eq_ignore_ascii_case("black")
    {
        2
    } else {
        0
    };
    we10x_theme_dd.set_selected(init_theme_idx);
    let st_theme = status_lbl.clone();
    we10x_theme_dd.connect_selected_notify(move |dd| {
        let (mode, mode_tr) = match dd.selected() {
            1 => ("light", "Açık"),
            2 => ("oled", "Tam siyah (OLED)"),
            _ => ("dark", "Koyu"),
        };
        st_theme.set_text(&format!("We10X tema modu {} olarak uygulandı", mode_tr));
        st_theme.set_visible(true);
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-theme", mode])
                .status();
        });
    });
    theme_row.append(&we10x_theme_dd);
    we10x_sec.append(&theme_row);

    // 2. Preset Color Swatches (Red, Blue, Dark Charcoal, Green, Purple, Amber, Teal)
    let swatch_hdr = Label::new(Some("Vurgu rengi"));
    swatch_hdr.add_css_class("setting-label");
    swatch_hdr.set_halign(gtk4::Align::Start);
    we10x_sec.append(&swatch_hdr);

    let swatch_flow = gtk4::FlowBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .max_children_per_line(7)
        .min_children_per_line(3)
        .column_spacing(8)
        .row_spacing(8)
        .homogeneous(false)
        .build();

    let current_accent = crate::we10x_profile::normalize_hex_color(&cfg.shell.we10x_accent_color);
    let hex_entry = gtk4::Entry::builder()
        .text(&current_accent)
        .max_length(7)
        .width_chars(9)
        .placeholder_text("#e81123")
        .build();
    hex_entry.add_css_class("settings-mono");

    let color_dialog = gtk4::ColorDialog::builder()
        .title("We10X özel vurgu rengi")
        .with_alpha(false)
        .modal(true)
        .build();
    let mut init_rgba = gdk::RGBA::new(0.0, 0.47, 0.84, 1.0);
    if let Ok(parsed_rgba) = gdk::RGBA::parse(&cfg.shell.we10x_accent_color) {
        init_rgba = parsed_rgba;
    }
    let color_btn = gtk4::ColorDialogButton::new(Some(color_dialog));
    color_btn.set_rgba(&init_rgba);
    color_btn.set_tooltip_text(Some("Renk seçiciden özel renk belirle"));

    let presets: [(&str, &str, &str); 7] = [
        ("Kırmızı", "#e81123", "swatch-dot-red"),
        ("Mavi", "#0078d7", "swatch-dot-blue"),
        ("Koyu", "#2d2d30", "swatch-dot-dark"),
        ("Yeşil", "#107c10", "swatch-dot-green"),
        ("Mor", "#881798", "swatch-dot-purple"),
        ("Kehribar", "#ff6a00", "swatch-dot-amber"),
        ("Turkuaz", "#008272", "swatch-dot-teal"),
    ];

    let mut swatch_btns: Vec<(String, Button)> = Vec::new();
    for (label_txt, hex_code, dot_cls) in presets {
        let b = Button::new();
        b.add_css_class("swatch-chip");
        if current_accent.eq_ignore_ascii_case(hex_code) {
            b.add_css_class("swatch-active");
        }

        let inner = GtkBox::new(Orientation::Horizontal, 8);
        inner.set_valign(gtk4::Align::Center);

        let dot = GtkBox::new(Orientation::Horizontal, 0);
        dot.add_css_class("swatch-dot");
        dot.add_css_class(dot_cls);
        dot.set_size_request(12, 12);
        dot.set_halign(gtk4::Align::Center);
        dot.set_valign(gtk4::Align::Center);
        dot.set_hexpand(false);
        dot.set_vexpand(false);
        inner.append(&dot);

        let lbl = Label::new(Some(label_txt));
        lbl.set_valign(gtk4::Align::Center);
        inner.append(&lbl);

        b.set_child(Some(&inner));
        swatch_flow.insert(&b, -1);
        swatch_btns.push((hex_code.to_string(), b));
    }

    let swatch_btns_rc = Rc::new(swatch_btns);
    let sync_active_swatch = |btns: &[(String, Button)], active_hex: &str| {
        for (hx, btn) in btns.iter() {
            if hx.eq_ignore_ascii_case(active_hex) {
                btn.add_css_class("swatch-active");
            } else {
                btn.remove_css_class("swatch-active");
            }
        }
    };

    for ((label_txt, hex_code, _), (_, btn)) in presets.iter().zip(swatch_btns_rc.iter()) {
        let hex_owned = (*hex_code).to_string();
        let lbl_owned = (*label_txt).to_string();
        let entry_ref = hex_entry.clone();
        let cbtn_ref = color_btn.clone();
        let st_sw = status_lbl.clone();
        let btns_ref = swatch_btns_rc.clone();
        btn.connect_clicked(move |_| {
            entry_ref.set_text(&hex_owned);
            if let Ok(rgba) = gdk::RGBA::parse(&hex_owned) {
                cbtn_ref.set_rgba(&rgba);
            }
            sync_active_swatch(&btns_ref, &hex_owned);
            st_sw.set_text(&format!(
                "We10X vurgu rengi {} ({}) olarak uygulandı",
                lbl_owned, hex_owned
            ));
            st_sw.set_visible(true);
            let hx = hex_owned.clone();
            std::thread::spawn(move || {
                let _ = Command::new("solar-shell")
                    .args(["we10x-config", "set-accent", &hx])
                    .status();
            });
        });
    }
    we10x_sec.append(&swatch_flow);

    // 3. Arbitrary Custom RGB / Hex Color Picker Row
    let custom_color_row = GtkBox::new(Orientation::Horizontal, 8);
    let custom_lbl = Label::new(Some("Özel renk (Hex)"));
    custom_lbl.add_css_class("setting-label");
    custom_lbl.set_hexpand(true);
    custom_lbl.set_halign(gtk4::Align::Start);
    custom_color_row.append(&custom_lbl);

    let entry_from_dialog = hex_entry.clone();
    let st_dialog = status_lbl.clone();
    let btns_from_dialog = swatch_btns_rc.clone();
    color_btn.connect_rgba_notify(move |btn| {
        let c = btn.rgba();
        let r = (c.red().clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (c.green().clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (c.blue().clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
        if entry_from_dialog.text().as_str().to_ascii_lowercase() != hex {
            entry_from_dialog.set_text(&hex);
        }
        for (hx, sbtn) in btns_from_dialog.iter() {
            if hx.eq_ignore_ascii_case(&hex) {
                sbtn.add_css_class("swatch-active");
            } else {
                sbtn.remove_css_class("swatch-active");
            }
        }
        st_dialog.set_text(&format!("Özel vurgu rengi {} uygulandı", hex));
        st_dialog.set_visible(true);
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-accent", &hex])
                .status();
        });
    });
    custom_color_row.append(&color_btn);
    custom_color_row.append(&hex_entry);

    let apply_hex_btn = Button::with_label("Uygula");
    apply_hex_btn.add_css_class("secondary-btn");
    let entry_for_btn = hex_entry.clone();
    let cbtn_for_btn = color_btn.clone();
    let st_hex = status_lbl.clone();
    let btns_for_hex = swatch_btns_rc.clone();
    apply_hex_btn.connect_clicked(move |_| {
        let norm = crate::we10x_profile::normalize_hex_color(entry_for_btn.text().as_str());
        entry_for_btn.set_text(&norm);
        if let Ok(rgba) = gdk::RGBA::parse(&norm) {
            cbtn_for_btn.set_rgba(&rgba);
        }
        for (hx, sbtn) in btns_for_hex.iter() {
            if hx.eq_ignore_ascii_case(&norm) {
                sbtn.add_css_class("swatch-active");
            } else {
                sbtn.remove_css_class("swatch-active");
            }
        }
        st_hex.set_text(&format!("Özel Hex rengi {} uygulandı", norm));
        st_hex.set_visible(true);
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-accent", &norm])
                .status();
        });
    });
    custom_color_row.append(&apply_hex_btn);
    we10x_sec.append(&custom_color_row);

    // 4. Colorize Taskbar, Start Menu & Titlebars Toggle
    let colorize_chk = CheckButton::with_label(
        "Görev çubuğu, Başlat menüsü ve pencere başlık çubuklarını vurgu rengiyle boya",
    );
    colorize_chk.set_active(cfg.shell.we10x_colorize_surfaces);
    let st_col = status_lbl.clone();
    colorize_chk.connect_toggled(move |chk| {
        let val = if chk.is_active() { "true" } else { "false" };
        st_col.set_text("Yüzey renklendirme tercihi güncellendi");
        st_col.set_visible(true);
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-colorize", val])
                .status();
        });
    });
    we10x_sec.append(&colorize_chk);

    // 5. Taskbar Opacity
    let op_row = GtkBox::new(Orientation::Horizontal, 12);
    let op_lbl = Label::new(Some("Görev çubuğu saydamlığı"));
    op_lbl.add_css_class("setting-label");
    op_lbl.set_hexpand(true);
    op_lbl.set_halign(gtk4::Align::Start);
    op_row.append(&op_lbl);

    let op_choices = [
        "%65 (şeffaf)",
        "%85 (yarı saydam)",
        "%95 (varsayılan koyu)",
        "%100 (tam mat)",
    ];
    let op_vals: [u8; 4] = [65, 85, 95, 100];
    let op_dd = DropDown::new(Some(StringList::new(&op_choices)), None::<gtk4::Expression>);
    let cur_op = cfg.shell.we10x_taskbar_opacity_pct;
    let init_op_idx = if cur_op <= 72 {
        0
    } else if cur_op <= 90 {
        1
    } else if cur_op < 99 {
        2
    } else {
        3
    };
    op_dd.set_selected(init_op_idx);
    op_dd.connect_selected_notify(move |dd| {
        let pct = op_vals.get(dd.selected() as usize).copied().unwrap_or(95);
        let pct_s = pct.to_string();
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-opacity", &pct_s])
                .status();
        });
    });
    op_row.append(&op_dd);
    we10x_sec.append(&op_row);

    // 6. Start Icon & Desktop Icons
    let icon_row = GtkBox::new(Orientation::Horizontal, 12);
    let icon_lbl = Label::new(Some("Başlat düğmesi simgesi"));
    icon_lbl.add_css_class("setting-label");
    icon_lbl.set_hexpand(true);
    icon_lbl.set_halign(gtk4::Align::Start);
    icon_row.append(&icon_lbl);

    let start_icons = [
        "We10X dört bölmeli simge",
        "BlazeOS alev simgesi",
    ];
    let start_icon_dd = DropDown::new(Some(StringList::new(&start_icons)), None::<gtk4::Expression>);
    start_icon_dd.set_selected(if cfg.shell.we10x_start_icon.eq_ignore_ascii_case("blaze") {
        1
    } else {
        0
    });
    start_icon_dd.connect_selected_notify(|dd| {
        let icon = if dd.selected() == 1 { "blaze" } else { "we10x" };
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-start-icon", icon])
                .status();
        });
    });
    icon_row.append(&start_icon_dd);
    we10x_sec.append(&icon_row);

    let desk_chk = CheckButton::with_label(
        "Masaüstü simgelerini göster (Bu Bilgisayar, Geri Dönüşüm Kutusu, FireHub)",
    );
    desk_chk.set_active(cfg.shell.we10x_desktop_icons);
    desk_chk.connect_toggled(|chk| {
        let val = if chk.is_active() { "true" } else { "false" };
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-desktop-icons", val])
                .status();
        });
    });
    we10x_sec.append(&desk_chk);
    we10x_sec.append(&status_lbl);

    we10x_sec
}

fn build_personalize_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Kişiselleştirme",
        "Masaüstü duvar kağıdını ve We10X görünüm tercihlerini yönetin",
    ));

    // Section 1: Universal Wallpaper Gallery & Custom File Picker (Works in Noctalia, We10X, Caelestia, end4-pC)
    let wp_sec = GtkBox::new(Orientation::Vertical, 12);
    wp_sec.add_css_class("section-box");

    let current_wp = crate::wallpaper::current_effective_wallpaper();

    let wp_hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    let wp_hdr_left = GtkBox::new(Orientation::Vertical, 4);
    wp_hdr_left.set_hexpand(true);

    let wp_title = Label::new(Some("Duvar kağıdı"));
    wp_title.add_css_class("section-title");
    wp_title.set_halign(gtk4::Align::Start);
    wp_hdr_left.append(&wp_title);

    let wp_desc = Label::new(Some(
        "Seçilen görsel etkin kabuğa ve Nautilus dosya yöneticisine anında uygulanır",
    ));
    wp_desc.add_css_class("setting-subtext");
    wp_desc.set_wrap(true);
    wp_desc.set_xalign(0.0);
    wp_desc.set_halign(gtk4::Align::Start);
    wp_hdr_left.append(&wp_desc);
    wp_hdr_row.append(&wp_hdr_left);

    let active_wp_lbl = Label::new(Some(&format!("Geçerli: {}", current_wp)));
    active_wp_lbl.add_css_class("settings-meta");
    active_wp_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    active_wp_lbl.set_halign(gtk4::Align::Start);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    action_row.set_valign(gtk4::Align::Start);

    let gallery = gtk4::FlowBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(2)
        .column_spacing(12)
        .row_spacing(12)
        .homogeneous(true)
        .build();

    let mut wp_cards: Vec<(String, GtkBox)> = Vec::new();
    let mut wp_apply_btns: Vec<(String, Button)> = Vec::new();
    let mut wp_pics: Vec<gtk4::Picture> = Vec::new();
    let mut thumb_jobs: Vec<(usize, String)> = Vec::new();
    let wallpapers = crate::wallpaper::discover_available_wallpapers();
    for (idx, (title_txt, img_path)) in wallpapers.iter().enumerate() {
        let card = GtkBox::new(Orientation::Vertical, 0);
        card.add_css_class("wp-card");
        card.set_overflow(gtk4::Overflow::Hidden);
        if img_path == &current_wp {
            card.add_css_class("wp-active");
        }

        // Never decode full-resolution 4K/16MB wallpapers synchronously before window.present().
        // Populate lightweight scaled thumbnails asynchronously via ~/.cache/solarui/wp-thumbs/.
        let pic = gtk4::Picture::new();
        pic.set_can_shrink(true);
        pic.set_content_fit(gtk4::ContentFit::Cover);
        pic.set_size_request(180, 100);
        card.append(&pic);

        let footer = GtkBox::new(Orientation::Horizontal, 8);
        footer.add_css_class("wp-footer");
        footer.set_valign(gtk4::Align::Center);

        let name_l = Label::new(Some(title_txt));
        name_l.add_css_class("wp-title");
        name_l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_l.set_hexpand(true);
        name_l.set_halign(gtk4::Align::Start);
        footer.append(&name_l);

        let use_btn = Button::with_label("Uygula");
        use_btn.add_css_class("secondary-btn");
        use_btn.add_css_class("wp-apply-btn");
        footer.append(&use_btn);

        card.append(&footer);
        gallery.insert(&card, -1);
        wp_cards.push((img_path.clone(), card));
        wp_apply_btns.push((img_path.clone(), use_btn));
        wp_pics.push(pic);
        thumb_jobs.push((idx, img_path.clone()));
    }

    if !thumb_jobs.is_empty() {
        let (thumb_tx, thumb_rx) = std::sync::mpsc::channel::<(usize, PathBuf)>();
        std::thread::spawn(move || {
            let cache_dir = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/tmp"))
                .join(".cache/solarui/wp-thumbs");
            let _ = std::fs::create_dir_all(&cache_dir);

            for (idx, src_path) in thumb_jobs {
                let meta = std::fs::metadata(&src_path).ok();
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let mtime = meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let mut hash: u64 = 14695981039346656037;
                for b in format!("{}:{}:{}", src_path, size, mtime).bytes() {
                    hash ^= b as u64;
                    hash = hash.wrapping_mul(1099511628211);
                }
                let thumb_path = cache_dir.join(format!("wp_{:016x}.jpg", hash));
                if thumb_path.is_file() {
                    let _ = thumb_tx.send((idx, thumb_path));
                    continue;
                }
                if let Ok(pb) = gdk_pixbuf::Pixbuf::from_file_at_scale(&src_path, 360, 200, true) {
                    let tmp_path = cache_dir.join(format!("wp_{:016x}.tmp.{}.jpg", hash, std::process::id()));
                    if pb.savev(&tmp_path, "jpeg", &[("quality", "82")]).is_ok() {
                        if std::fs::rename(&tmp_path, &thumb_path).is_ok() {
                            let _ = thumb_tx.send((idx, thumb_path));
                            continue;
                        }
                        let _ = std::fs::remove_file(&tmp_path);
                    }
                }
                let _ = thumb_tx.send((idx, PathBuf::from(src_path)));
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(20), move || {
            loop {
                match thumb_rx.try_recv() {
                    Ok((idx, path)) => {
                        if let Some(pic) = wp_pics.get(idx) {
                            pic.set_filename(Some(&path));
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return glib::ControlFlow::Break,
                }
            }
        });
    }

    let wp_cards_rc = Rc::new(wp_cards);

    // Wire per-card apply buttons with active-card highlight sync
    for (img_path, use_btn) in wp_apply_btns {
        let lbl_card = active_wp_lbl.clone();
        let cards_ref = wp_cards_rc.clone();
        use_btn.connect_clicked(move |_| {
            lbl_card.set_text(&format!("Geçerli: {}", img_path));
            for (path_k, c_box) in cards_ref.iter() {
                if path_k == &img_path {
                    c_box.add_css_class("wp-active");
                } else {
                    c_box.remove_css_class("wp-active");
                }
            }
            let target = img_path.clone();
            std::thread::spawn(move || {
                let _ = crate::wallpaper::apply_wallpaper_everywhere(&target, true);
            });
        });
    }

    let pick_btn = Button::with_label("Görsel seç");
    pick_btn.add_css_class("action-btn");
    let lbl_after_pick = active_wp_lbl.clone();
    let cards_after_pick = wp_cards_rc.clone();
    pick_btn.connect_clicked(move |btn| {
        let win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        let dialog = gtk4::FileDialog::builder()
            .title("Duvar kağıdı görseli seç")
            .modal(true)
            .build();
        let filter = gtk4::FileFilter::new();
        filter.set_name(Some("Görseller (PNG, JPG, JPEG, WEBP)"));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        filter.add_mime_type("image/webp");
        let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        let lbl_cb = lbl_after_pick.clone();
        let cards_cb = cards_after_pick.clone();
        dialog.open(win.as_ref(), None::<&gtk4::gio::Cancellable>, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    lbl_cb.set_text(&format!("Geçerli: {}", path_str));
                    for (path_k, c_box) in cards_cb.iter() {
                        if path_k == &path_str {
                            c_box.add_css_class("wp-active");
                        } else {
                            c_box.remove_css_class("wp-active");
                        }
                    }
                    std::thread::spawn(move || {
                        let _ = crate::wallpaper::apply_wallpaper_everywhere(&path_str, true);
                    });
                }
            }
        });
    });
    action_row.append(&pick_btn);

    let open_dir_btn = Button::with_label("Klasörü aç");
    open_dir_btn.add_css_class("secondary-btn");
    open_dir_btn.connect_clicked(|_| {
        let _ = Command::new("nautilus")
            .arg("/usr/share/backgrounds/blazeos")
            .spawn();
    });
    action_row.append(&open_dir_btn);
    wp_hdr_row.append(&action_row);

    wp_sec.append(&wp_hdr_row);
    wp_sec.append(&active_wp_lbl);
    wp_sec.append(&gallery);
    tab.append(&wp_sec);

    // Section 2: We10X Theme Mode, Accent Color & Taskbar Customization
    tab.append(&build_we10x_customization_card());

    tab
}

fn build_caelestia_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Caelestia",
        "Duvar kağıdı renk şeması ve tema modu",
    ));

    let cfg = SolarConfig::load();
    if cfg.shell.engine != ShellEngine::Caelestia {
        let empty_sec = GtkBox::new(Orientation::Vertical, 8);
        empty_sec.add_css_class("section-box");

        let empty_title = Label::new(Some("Caelestia etkin değil"));
        empty_title.add_css_class("section-title");
        empty_title.set_halign(gtk4::Align::Start);
        empty_sec.append(&empty_title);

        let msg = Label::new(Some(
            "Bu ayarları kullanmak için Masaüstü kabuğu bölümünden Caelestia seçeneğini etkinleştirin",
        ));
        msg.add_css_class("setting-subtext");
        msg.set_wrap(true);
        msg.set_xalign(0.0);
        msg.set_halign(gtk4::Align::Start);
        empty_sec.append(&msg);

        tab.append(&empty_sec);
        return tab;
    }

    // Section 1: Wallpaper
    let sec_wall = GtkBox::new(Orientation::Vertical, 12);
    sec_wall.add_css_class("section-box");

    let w_title = Label::new(Some("Duvar kağıdı ve renk şeması"));
    w_title.add_css_class("section-title");
    w_title.set_halign(gtk4::Align::Start);
    sec_wall.append(&w_title);

    let w_desc = Label::new(Some(
        "Duvar kağıdı değiştiğinde Caelestia arayüzünün Material 3 renk paleti otomatik güncellenir",
    ));
    w_desc.add_css_class("setting-subtext");
    w_desc.set_wrap(true);
    w_desc.set_xalign(0.0);
    w_desc.set_halign(gtk4::Align::Start);
    sec_wall.append(&w_desc);

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);

    let btn_rand = Button::with_label("Rastgele seç");
    btn_rand.add_css_class("action-btn");
    btn_rand.connect_clicked(|_| {
        std::thread::spawn(|| {
            let _ = Command::new("caelestia").args(["wallpaper", "-r"]).status();
        });
    });
    btn_row.append(&btn_rand);

    let btn_pick = Button::with_label("Dosyadan seç");
    btn_pick.add_css_class("secondary-btn");
    btn_pick.connect_clicked(|btn| {
        let win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        let dialog = gtk4::FileDialog::builder()
            .title("Caelestia duvar kağıdı seç")
            .modal(true)
            .build();
        let filter = gtk4::FileFilter::new();
        filter.set_name(Some("Görseller (PNG, JPG, WEBP)"));
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        filter.add_mime_type("image/webp");
        let filters = gtk4::gio::ListStore::new::<gtk4::FileFilter>();
        filters.append(&filter);
        dialog.set_filters(Some(&filters));
        dialog.open(win.as_ref(), None::<&gtk4::gio::Cancellable>, |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    std::thread::spawn(move || {
                        let _ = Command::new("caelestia").args(["wallpaper", "-f", &path_str]).status();
                    });
                }
            }
        });
    });
    btn_row.append(&btn_pick);

    sec_wall.append(&btn_row);
    tab.append(&sec_wall);

    // Section 2: Mode and Layout
    let sec_lay = GtkBox::new(Orientation::Vertical, 12);
    sec_lay.add_css_class("section-box");

    let l_title = Label::new(Some("Tema modu"));
    l_title.add_css_class("section-title");
    l_title.set_halign(gtk4::Align::Start);
    sec_lay.append(&l_title);

    let l_desc = Label::new(Some(
        "Caelestia ve GTK uygulamaları için koyu veya açık renk şemasını seçin",
    ));
    l_desc.add_css_class("setting-subtext");
    l_desc.set_wrap(true);
    l_desc.set_xalign(0.0);
    l_desc.set_halign(gtk4::Align::Start);
    sec_lay.append(&l_desc);

    let row_mode = GtkBox::new(Orientation::Horizontal, 12);
    let lbl_mode = Label::new(Some("Renk şeması"));
    lbl_mode.add_css_class("setting-label");
    lbl_mode.set_hexpand(true);
    lbl_mode.set_halign(gtk4::Align::Start);
    row_mode.append(&lbl_mode);

    let mode_dd = DropDown::new(Some(StringList::new(&["Koyu", "Açık"])), None::<gtk4::Expression>);

    // Initial read
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let scheme_path = format!("{}/.local/state/caelestia/scheme.json", home);
    if let Ok(content) = std::fs::read_to_string(&scheme_path) {
        if content.contains("\"mode\": \"light\"") {
            mode_dd.set_selected(1);
        }
    }

    mode_dd.connect_selected_notify(move |dd| {
        let is_light = dd.selected() == 1;
        std::thread::spawn(move || {
            let mode_str = if is_light { "light" } else { "dark" };
            let gnome_mode = if is_light { "prefer-light" } else { "prefer-dark" };

            // 1. Run caelestia scheme set CLI which re-computes full light/dark Material You colors
            let _ = Command::new("caelestia").args(["scheme", "set", "--notify", "-m", mode_str]).status();

            // 2. Set GNOME desktop color-scheme for GTK apps
            let _ = Command::new("gsettings").args(["set", "org.gnome.desktop.interface", "color-scheme", gnome_mode]).status();

            // 3. Trigger Caelestia reload
            let _ = Command::new("pkill").args(["-SIGUSR2", "-f", "quickshell.*caelestia"]).status();
        });
    });
    row_mode.append(&mode_dd);
    sec_lay.append(&row_mode);

    tab.append(&sec_lay);
    tab
}

fn build_display_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Ekran",
        "Çözünürlük, yenileme hızı ve DPI ölçeğini ekran bazında veya genel olarak ayarlayın",
    ));

    let cfg = SolarConfig::load();
    let scale_presets = [
        "0.75x (%75, kompakt)",
        "0.85x (%85, 1280×800 için önerilen)",
        "1.00x (%100, varsayılan)",
        "1.25x (%125)",
        "1.50x (%150, 2K ekranlar)",
        "1.75x (%175)",
        "2.00x (%200, 4K ekranlar)",
    ];
    let scale_values = [0.75, 0.85, 1.00, 1.25, 1.50, 1.75, 2.00];

    // SECTION 1: Per-Monitor Manual Resolution, Refresh Rate (Hz) & Independent DPI Scaling
    let sec_mon = GtkBox::new(Orientation::Vertical, 12);
    sec_mon.add_css_class("section-box");

    let title_mon = Label::new(Some("Bağlı ekranlar"));
    title_mon.add_css_class("section-title");
    title_mon.set_halign(gtk4::Align::Start);
    sec_mon.append(&title_mon);

    let desc_mon = Label::new(Some(
        "Her ekran için çözünürlük, yenileme hızı ve bağımsız DPI ölçeği seçin veya özel mod girin",
    ));
    desc_mon.add_css_class("setting-subtext");
    desc_mon.set_wrap(true);
    desc_mon.set_xalign(0.0);
    desc_mon.set_halign(gtk4::Align::Start);
    sec_mon.append(&desc_mon);

    let common_resolutions: [(u32, u32); 11] = [
        (3840, 2160),
        (2560, 1440),
        (1920, 1200),
        (1920, 1080),
        (1680, 1050),
        (1600, 900),
        (1440, 900),
        (1366, 768),
        (1280, 800),
        (1280, 720),
        (1024, 768),
    ];
    let common_hz: [u32; 7] = [60_000, 75_000, 90_000, 120_000, 144_000, 165_000, 240_000];

    let detected_outputs = get_detected_outputs();
    for out in detected_outputs {
        let mon_card = GtkBox::new(Orientation::Vertical, 12);
        mon_card.add_css_class("monitor-card");

        let mon_header = GtkBox::new(Orientation::Horizontal, 8);
        let mon_icon = create_symbolic_icon("video-display-symbolic", "settings-row-icon");
        mon_header.append(&mon_icon);

        let info_box = GtkBox::new(Orientation::Vertical, 2);
        info_box.set_hexpand(true);

        let name_lbl = Label::new(Some(&format!("{} ({})", out.name, out.description)));
        name_lbl.add_css_class("monitor-name");
        name_lbl.set_halign(gtk4::Align::Start);
        name_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        info_box.append(&name_lbl);

        let res_lbl = Label::new(Some(&format!(
            "Etkin mod: {} · Ölçek: {:.2}x",
            out.resolution, out.scale
        )));
        res_lbl.add_css_class("settings-meta");
        res_lbl.set_halign(gtk4::Align::Start);
        info_box.append(&res_lbl);
        mon_header.append(&info_box);
        mon_card.append(&mon_header);

        // Build unique resolution list: EDID hardware resolutions first, then common fallback/custom resolutions
        let mut res_items: Vec<(u32, u32, bool)> = Vec::new();
        for m in &out.modes {
            if !res_items.iter().any(|(w, h, _)| *w == m.width && *h == m.height) {
                res_items.push((m.width, m.height, true));
            }
        }
        if !res_items
            .iter()
            .any(|(w, h, _)| *w == out.current_width && *h == out.current_height)
        {
            res_items.push((out.current_width, out.current_height, true));
        }
        for (cw, ch) in common_resolutions {
            if !res_items.iter().any(|(w, h, _)| *w == cw && *h == ch) {
                res_items.push((cw, ch, false));
            }
        }
        res_items.sort_by(|a, b| (b.0 as u64 * b.1 as u64).cmp(&(a.0 as u64 * a.1 as u64)));

        let res_labels: Vec<String> = res_items
            .iter()
            .map(|(w, h, is_hw)| {
                if *is_hw {
                    format!("{} × {} (donanım)", w, h)
                } else {
                    format!("{} × {} (özel)", w, h)
                }
            })
            .collect();
        let res_label_refs: Vec<&str> = res_labels.iter().map(|s| s.as_str()).collect();
        let res_dd = DropDown::new(Some(StringList::new(&res_label_refs)), None::<gtk4::Expression>);
        res_dd.set_hexpand(true);

        let init_res_idx = res_items
            .iter()
            .position(|(w, h, _)| *w == out.current_width && *h == out.current_height)
            .unwrap_or(0);
        res_dd.set_selected(init_res_idx as u32);

        // Helper closure to compute available refresh rates `(refresh_mhz, is_hw)` for a selected `(w, h)`
        let hw_modes = out.modes.clone();
        let build_hz_options = move |w: u32, h: u32| -> Vec<(u32, bool)> {
            let mut list: Vec<(u32, bool)> = Vec::new();
            for m in &hw_modes {
                if m.width == w
                    && m.height == h
                    && !list.iter().any(|(r, _)| (*r as i64 - m.refresh_mhz as i64).abs() < 50)
                {
                    list.push((m.refresh_mhz, true));
                }
            }
            for chz in common_hz {
                if !list.iter().any(|(r, _)| (*r as i64 - chz as i64).abs() < 600) {
                    list.push((chz, false));
                }
            }
            list.sort_by(|a, b| b.0.cmp(&a.0));
            list
        };

        let init_pair = res_items.get(init_res_idx).copied().unwrap_or((1920, 1080, true));
        let init_hz_opts = build_hz_options(init_pair.0, init_pair.1);
        let hz_state: Rc<std::cell::RefCell<Vec<(u32, bool)>>> =
            Rc::new(std::cell::RefCell::new(init_hz_opts.clone()));

        let hz_labels_init: Vec<String> = init_hz_opts
            .iter()
            .map(|(mhz, is_hw)| {
                if *is_hw {
                    format!("{:.2} Hz (donanım)", *mhz as f64 / 1000.0)
                } else {
                    format!("{:.2} Hz (özel)", *mhz as f64 / 1000.0)
                }
            })
            .collect();
        let hz_refs_init: Vec<&str> = hz_labels_init.iter().map(|s| s.as_str()).collect();
        let hz_dd = DropDown::new(Some(StringList::new(&hz_refs_init)), None::<gtk4::Expression>);
        hz_dd.set_hexpand(true);

        let init_hz_idx = init_hz_opts
            .iter()
            .position(|(mhz, _)| (*mhz as i64 - out.current_refresh_mhz as i64).abs() < 600)
            .unwrap_or(0);
        hz_dd.set_selected(init_hz_idx as u32);

        // Refresh Hz dropdown dynamically whenever resolution dropdown changes
        let res_items_rc = Rc::new(res_items);
        let res_for_hz = res_items_rc.clone();
        let hz_dd_ref = hz_dd.clone();
        let hz_state_ref = hz_state.clone();
        let build_hz_cb = build_hz_options.clone();
        let pref_mhz = out.current_refresh_mhz;
        res_dd.connect_selected_notify(move |dd| {
            let idx = dd.selected() as usize;
            let (w, h, _) = res_for_hz.get(idx).copied().unwrap_or((1920, 1080, true));
            let next_opts = build_hz_cb(w, h);
            let labels: Vec<String> = next_opts
                .iter()
                .map(|(mhz, is_hw)| {
                    if *is_hw {
                        format!("{:.2} Hz (donanım)", *mhz as f64 / 1000.0)
                    } else {
                        format!("{:.2} Hz (özel)", *mhz as f64 / 1000.0)
                    }
                })
                .collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            hz_dd_ref.set_model(Some(&StringList::new(&refs)));
            let best_idx = next_opts
                .iter()
                .position(|(mhz, _)| (*mhz as i64 - pref_mhz as i64).abs() < 600)
                .unwrap_or(0);
            *hz_state_ref.borrow_mut() = next_opts;
            hz_dd_ref.set_selected(best_idx as u32);
        });

        // Scale dropdown for this monitor
        let mon_scale_dd =
            DropDown::new(Some(StringList::new(&scale_presets)), None::<gtk4::Expression>);
        mon_scale_dd.set_hexpand(true);
        let mut mon_sel_idx = 2;
        for (i, &val) in scale_values.iter().enumerate() {
            if (out.scale - val).abs() < 0.04 {
                mon_sel_idx = i as u32;
                break;
            }
        }
        mon_scale_dd.set_selected(mon_sel_idx);

        // Controls row 1: Resolution + Refresh Rate (Hz)
        let row_mode_sel = GtkBox::new(Orientation::Horizontal, 12);
        let l_res = Label::new(Some("Çözünürlük"));
        l_res.add_css_class("setting-label");
        row_mode_sel.append(&l_res);
        row_mode_sel.append(&res_dd);

        let l_hz = Label::new(Some("Yenileme hızı (Hz)"));
        l_hz.add_css_class("setting-label");
        l_hz.set_margin_start(8);
        row_mode_sel.append(&l_hz);
        row_mode_sel.append(&hz_dd);
        mon_card.append(&row_mode_sel);

        // Controls row 2: DPI Scale + Apply Button
        let row_scale_sel = GtkBox::new(Orientation::Horizontal, 12);
        let l_sc = Label::new(Some("Monitör ölçeği"));
        l_sc.add_css_class("setting-label");
        row_scale_sel.append(&l_sc);
        row_scale_sel.append(&mon_scale_dd);

        let mon_apply = Button::with_label("Uygula");
        mon_apply.add_css_class("action-btn");

        let mon_status = Label::new(None);
        mon_status.add_css_class("status-text");
        mon_status.set_halign(gtk4::Align::Start);
        mon_status.set_visible(false);

        let out_name = out.name.clone();
        let res_dd_apply = res_dd.clone();
        let hz_dd_apply = hz_dd.clone();
        let sc_dd_apply = mon_scale_dd.clone();
        let res_items_apply = res_items_rc.clone();
        let hz_state_apply = hz_state.clone();
        let mon_status_apply = mon_status.clone();
        let res_lbl_apply = res_lbl.clone();

        mon_apply.connect_clicked(move |_| {
            let r_idx = res_dd_apply.selected() as usize;
            let (w, h, is_hw_res) = res_items_apply
                .get(r_idx)
                .copied()
                .unwrap_or((1920, 1080, true));
            let h_idx = hz_dd_apply.selected() as usize;
            let (mhz, is_hw_hz) = hz_state_apply
                .borrow()
                .get(h_idx)
                .copied()
                .unwrap_or((60_000, false));
            let sc_idx = sc_dd_apply.selected() as usize;
            let sc_val = scale_values.get(sc_idx).copied().unwrap_or(1.00);

            let mode_spec = format!("{}x{}@{:.3}", w, h, mhz as f64 / 1000.0);
            let is_custom = !(is_hw_res && is_hw_hz);

            for cls in ["status-text", "status-text-ok", "status-text-warn", "status-text-danger"] {
                mon_status_apply.remove_css_class(cls);
            }

            match apply_per_output_mode_dynamically(&out_name, &mode_spec, is_custom) {
                Ok(()) => {
                    apply_per_output_scale_dynamically(&out_name, sc_val);
                    res_lbl_apply.set_text(&format!(
                        "Etkin mod: {}×{} @ {:.2} Hz · Ölçek: {:.2}x",
                        w,
                        h,
                        mhz as f64 / 1000.0,
                        sc_val
                    ));
                    mon_status_apply.add_css_class("status-text-ok");
                    mon_status_apply.set_text(&format!(
                        "{} için {} ({:.2}x ölçek) uygulandı ve kaydedildi",
                        out_name, mode_spec, sc_val
                    ));
                    mon_status_apply.set_visible(true);
                }
                Err(err) => {
                    mon_status_apply.add_css_class("status-text-warn");
                    mon_status_apply.set_text(&format!(
                        "{} modu uygulanamadı: {}",
                        mode_spec, err
                    ));
                    mon_status_apply.set_visible(true);
                }
            }
        });
        row_scale_sel.append(&mon_apply);
        mon_card.append(&row_scale_sel);

        let mon_sep = Separator::new(Orientation::Horizontal);
        mon_card.append(&mon_sep);

        // Controls row 3: Custom W x H @ Hz Manual Entry
        let custom_row = GtkBox::new(Orientation::Horizontal, 8);
        let c_lbl = Label::new(Some("Özel mod"));
        c_lbl.add_css_class("setting-subtext");
        custom_row.append(&c_lbl);

        let w_entry = gtk4::Entry::builder()
            .text(out.current_width.to_string())
            .width_chars(6)
            .placeholder_text("1920")
            .build();
        w_entry.add_css_class("settings-mono");
        let x_lbl = Label::new(Some("×"));
        x_lbl.add_css_class("setting-subtext");
        let h_entry = gtk4::Entry::builder()
            .text(out.current_height.to_string())
            .width_chars(6)
            .placeholder_text("1080")
            .build();
        h_entry.add_css_class("settings-mono");
        let at_lbl = Label::new(Some("@"));
        at_lbl.add_css_class("setting-subtext");
        let hz_entry = gtk4::Entry::builder()
            .text(format!("{:.0}", out.current_refresh_mhz as f64 / 1000.0))
            .width_chars(5)
            .placeholder_text("60")
            .build();
        hz_entry.add_css_class("settings-mono");
        let hz_u_lbl = Label::new(Some("Hz"));
        hz_u_lbl.add_css_class("setting-subtext");

        let custom_btn = Button::with_label("Özel modu uygula");
        custom_btn.add_css_class("secondary-btn");
        let out_custom = out.name.clone();
        let st_custom = mon_status.clone();
        let rl_custom = res_lbl.clone();
        let w_ent_c = w_entry.clone();
        let h_ent_c = h_entry.clone();
        let hz_ent_c = hz_entry.clone();
        let cur_sc = out.scale;
        custom_btn.connect_clicked(move |_| {
            for cls in ["status-text", "status-text-ok", "status-text-warn", "status-text-danger"] {
                st_custom.remove_css_class(cls);
            }
            let w_ok = w_ent_c.text().trim().parse::<u32>().unwrap_or(0);
            let h_ok = h_ent_c.text().trim().parse::<u32>().unwrap_or(0);
            let hz_ok = hz_ent_c
                .text()
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .unwrap_or(0.0);
            if !(640..=7680).contains(&w_ok)
                || !(480..=4320).contains(&h_ok)
                || !(24.0..=500.0).contains(&hz_ok)
            {
                st_custom.add_css_class("status-text-danger");
                st_custom.set_text(
                    "Geçerli bir çözünürlük (640..7680 × 480..4320) ve yenileme hızı (24..500 Hz) girin",
                );
                st_custom.set_visible(true);
                return;
            }
            let spec = format!("{}x{}@{:.3}", w_ok, h_ok, hz_ok);
            match apply_per_output_mode_dynamically(&out_custom, &spec, true) {
                Ok(()) => {
                    rl_custom.set_text(&format!(
                        "Etkin mod: {}×{} @ {:.2} Hz · Ölçek: {:.2}x",
                        w_ok, h_ok, hz_ok, cur_sc
                    ));
                    st_custom.add_css_class("status-text-ok");
                    st_custom.set_text(&format!(
                        "Özel mod {} uygulandı ve kaydedildi",
                        spec
                    ));
                    st_custom.set_visible(true);
                }
                Err(e) => {
                    st_custom.add_css_class("status-text-warn");
                    st_custom.set_text(&format!("Özel mod uygulanamadı: {}", e));
                    st_custom.set_visible(true);
                }
            }
        });

        custom_row.append(&w_entry);
        custom_row.append(&x_lbl);
        custom_row.append(&h_entry);
        custom_row.append(&at_lbl);
        custom_row.append(&hz_entry);
        custom_row.append(&hz_u_lbl);
        let spacer = GtkBox::new(Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        custom_row.append(&spacer);
        custom_row.append(&custom_btn);
        mon_card.append(&custom_row);
        mon_card.append(&mon_status);

        sec_mon.append(&mon_card);
    }
    tab.append(&sec_mon);

    // SECTION 2: Global DPI Scaling Across All Displays
    let sec = GtkBox::new(Orientation::Vertical, 12);
    sec.add_css_class("section-box");

    let title = Label::new(Some("Genel ölçek"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Tüm ekranlardaki pencere ve metin boyutunu tek seferde ölçeklendirin",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    let row_scale = GtkBox::new(Orientation::Horizontal, 12);
    let lbl_scale = Label::new(Some("Sistem ölçeği"));
    lbl_scale.add_css_class("setting-label");
    lbl_scale.set_hexpand(true);
    lbl_scale.set_halign(gtk4::Align::Start);
    row_scale.append(&lbl_scale);

    let scale_model = StringList::new(&scale_presets);
    let scale_dd = DropDown::new(Some(scale_model), None::<gtk4::Expression>);

    let cur_scale = cfg.display.scale;
    let mut selected_idx = 2;
    for (i, &val) in scale_values.iter().enumerate() {
        if (cur_scale - val).abs() < 0.04 {
            selected_idx = i as u32;
            break;
        }
    }
    scale_dd.set_selected(selected_idx);
    row_scale.append(&scale_dd);
    sec.append(&row_scale);

    let status_lbl = Label::new(None);
    status_lbl.add_css_class("status-text-ok");
    status_lbl.set_halign(gtk4::Align::Start);
    status_lbl.set_visible(false);

    let btn_box = GtkBox::new(Orientation::Horizontal, 8);
    btn_box.set_margin_top(4);

    let apply_btn = Button::with_label("Tüm ekranlara uygula");
    apply_btn.add_css_class("action-btn");

    let scale_dd_clone = scale_dd.clone();
    let status_clone = status_lbl.clone();

    apply_btn.connect_clicked(move |_| {
        let idx = scale_dd_clone.selected() as usize;
        let chosen_scale = if idx < scale_values.len() {
            scale_values[idx]
        } else {
            1.00
        };

        let mut c = SolarConfig::load();
        c.display.scale = chosen_scale;
        c.display.text_scale = chosen_scale;
        let _ = c.save();

        apply_display_scale_dynamically(chosen_scale);

        status_clone.set_text(&format!(
            "Genel ölçek {:.2}x uygulandı ve kaydedildi",
            chosen_scale
        ));
        status_clone.set_visible(true);
    });
    btn_box.append(&apply_btn);

    let reset_btn = Button::with_label("Sıfırla");
    reset_btn.add_css_class("secondary-btn");
    let scale_dd_reset = scale_dd.clone();
    let status_reset = status_lbl.clone();
    reset_btn.connect_clicked(move |_| {
        scale_dd_reset.set_selected(2);
        let mut c = SolarConfig::load();
        c.display.scale = 1.00;
        c.display.text_scale = 1.00;
        let _ = c.save();
        apply_display_scale_dynamically(1.00);
        status_reset.set_text("Ölçek varsayılan 1.00x değerine sıfırlandı");
        status_reset.set_visible(true);
    });
    btn_box.append(&reset_btn);

    sec.append(&btn_box);
    sec.append(&status_lbl);
    tab.append(&sec);

    tab
}

pub fn detect_screen_resolution_and_auto_scale() -> f64 {
    let cfg = SolarConfig::load();
    // If user already explicitly set a custom non-default scale, respect their preference
    if (cfg.display.scale - 1.0).abs() > 0.04 {
        return cfg.display.scale;
    }

    // 1. Try to query Niri via `niri msg --json outputs`
    if let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() {
        if out.status.success() {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(obj) = val.as_object() {
                    let mut min_height = u32::MAX;
                    for (_name, output_val) in obj {
                        let height = if let Some(current_mode_idx) = output_val.get("current_mode").and_then(|v| v.as_u64()) {
                            output_val.get("modes")
                                .and_then(|m| m.as_array())
                                .and_then(|modes| modes.get(current_mode_idx as usize))
                                .and_then(|mode| mode.get("height"))
                                .and_then(|h| h.as_u64())
                                .map(|h| h as u32)
                        } else {
                            output_val.get("modes")
                                .and_then(|m| m.as_array())
                                .and_then(|modes| modes.first())
                                .and_then(|mode| mode.get("height"))
                                .and_then(|h| h.as_u64())
                                .map(|h| h as u32)
                        };

                        if let Some(h) = height {
                            if h < min_height {
                                min_height = h;
                            }
                        }
                    }

                    if min_height != u32::MAX {
                        return scale_for_height(min_height);
                    }
                }
            }
        }
    }

    // 2. Fallback: check DRM modes in /sys/class/drm/*/modes
    if let Ok(entries) = std::fs::read_dir("/sys/class/drm") {
        let mut min_height = u32::MAX;
        for entry in entries.flatten() {
            let modes_file = entry.path().join("modes");
            if let Ok(content) = std::fs::read_to_string(modes_file) {
                for line in content.lines() {
                    if let Some((_w, h)) = line.split_once('x') {
                        if let Ok(height) = h.trim().parse::<u32>() {
                            if height < min_height {
                                min_height = height;
                            }
                        }
                    }
                }
            }
        }
        if min_height != u32::MAX {
            return scale_for_height(min_height);
        }
    }

    1.00
}

fn scale_for_height(height: u32) -> f64 {
    if height <= 720 {
        0.75
    } else if height <= 800 {
        0.85
    } else if height <= 900 {
        0.90
    } else if height >= 2160 {
        1.75
    } else if height >= 1440 {
        1.25
    } else {
        1.00
    }
}

pub fn sync_caelestia_scale(_scale: f64) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    let conf_dir = PathBuf::from(&home).join(".config/caelestia");
    let _ = std::fs::create_dir_all(&conf_dir);
    let shell_json_path = conf_dir.join("shell.json");

    let mut val: serde_json::Value = if let Ok(content) = std::fs::read_to_string(&shell_json_path) {
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if !val.is_object() {
        val = serde_json::json!({});
    }

    if !val.get("appearance").map_or(false, |v| v.is_object()) {
        val["appearance"] = serde_json::json!({});
    }

    if let Some(app) = val.get_mut("appearance").and_then(|v| v.as_object_mut()) {
        if !app.contains_key("font") || !app["font"].is_object() {
            app.insert("font".to_string(), serde_json::json!({}));
        }
        if let Some(font_obj) = app.get_mut("font").and_then(|v| v.as_object_mut()) {
            if !font_obj.contains_key("icon") || !font_obj["icon"].is_object() {
                font_obj.insert("icon".to_string(), serde_json::json!({}));
            }
            if let Some(icon_obj) = font_obj.get_mut("icon").and_then(|v| v.as_object_mut()) {
                icon_obj.insert(
                    "family".to_string(),
                    serde_json::Value::String("Material Symbols Rounded".to_string()),
                );
            }
        }
    }

    if let Ok(formatted) = serde_json::to_string_pretty(&val) {
        let _ = std::fs::write(&shell_json_path, formatted);
    }
}

#[derive(Clone, Debug)]
pub struct OutputModeEntry {
    pub width: u32,
    pub height: u32,
    pub refresh_mhz: u32,
    pub is_preferred: bool,
}

#[derive(Clone, Debug)]
pub struct OutputDetails {
    pub name: String,
    pub description: String,
    pub resolution: String,
    pub scale: f64,
    pub current_width: u32,
    pub current_height: u32,
    pub current_refresh_mhz: u32,
    pub modes: Vec<OutputModeEntry>,
}

pub fn get_detected_outputs() -> Vec<OutputDetails> {
    let mut outputs = Vec::new();
    let cfg = SolarConfig::load();

    if let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() {
        if out.status.success() {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(obj) = val.as_object() {
                    for (name, data) in obj {
                        let make = data.get("make").and_then(|v| v.as_str()).unwrap_or("");
                        let model = data.get("model").and_then(|v| v.as_str()).unwrap_or("");
                        let desc = if !make.is_empty() || !model.is_empty() {
                            format!("{} {}", make, model).trim().to_string()
                        } else {
                            "Dahili veya harici ekran".to_string()
                        };

                        let mut parsed_modes = Vec::new();
                        let mut cur_w: u32 = 1920;
                        let mut cur_h: u32 = 1080;
                        let mut cur_mhz: u32 = 60_000;
                        let mut res_str = "1920×1080 @ 60.00 Hz".to_string();

                        if let Some(modes) = data.get("modes").and_then(|v| v.as_array()) {
                            for m in modes {
                                let w = m.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let h = m.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                                let rr =
                                    m.get("refresh_rate").and_then(|v| v.as_u64()).unwrap_or(60_000)
                                        as u32;
                                let pref = m
                                    .get("is_preferred")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                if w > 0 && h > 0 {
                                    parsed_modes.push(OutputModeEntry {
                                        width: w,
                                        height: h,
                                        refresh_mhz: rr,
                                        is_preferred: pref,
                                    });
                                }
                            }
                            let cur_idx = data
                                .get("current_mode")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0) as usize;
                            if let Some(m) = modes.get(cur_idx).or_else(|| modes.first()) {
                                cur_w = m.get("width").and_then(|v| v.as_u64()).unwrap_or(1920) as u32;
                                cur_h = m.get("height").and_then(|v| v.as_u64()).unwrap_or(1080) as u32;
                                cur_mhz = m
                                    .get("refresh_rate")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(60_000) as u32;
                                res_str = format!(
                                    "{}×{} @ {:.2} Hz",
                                    cur_w,
                                    cur_h,
                                    cur_mhz as f64 / 1000.0
                                );
                            }
                        }

                        let saved_scale = cfg
                            .display
                            .outputs
                            .iter()
                            .find(|o| o.name == *name)
                            .map(|o| o.scale);
                        let logical_scale = data
                            .get("logical")
                            .and_then(|l| l.get("scale"))
                            .and_then(|v| v.as_f64());
                        let cur_scale = saved_scale.or(logical_scale).unwrap_or(cfg.display.scale);

                        outputs.push(OutputDetails {
                            name: name.clone(),
                            description: desc,
                            resolution: res_str,
                            scale: cur_scale,
                            current_width: cur_w,
                            current_height: cur_h,
                            current_refresh_mhz: cur_mhz,
                            modes: parsed_modes,
                        });
                    }
                }
            }
        }
    }

    if outputs.is_empty() {
        outputs.push(OutputDetails {
            name: "Virtual-1".to_string(),
            description: "Sanal ekran".to_string(),
            resolution: "1920×1080 @ 60.00 Hz".to_string(),
            scale: cfg.display.scale,
            current_width: 1920,
            current_height: 1080,
            current_refresh_mhz: 60_000,
            modes: vec![OutputModeEntry {
                width: 1920,
                height: 1080,
                refresh_mhz: 60_000,
                is_preferred: true,
            }],
        });
    }

    outputs
}

fn strip_kdl_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' && in_quotes {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

fn count_kdl_braces(code: &str) -> (usize, usize) {
    let mut opens = 0usize;
    let mut closes = 0usize;
    let mut in_quotes = false;
    let mut escaped = false;
    for b in code.bytes() {
        if escaped {
            escaped = false;
            continue;
        }
        if b == b'\\' && in_quotes {
            escaped = true;
            continue;
        }
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if b == b'{' {
                opens += 1;
            } else if b == b'}' {
                closes += 1;
            }
        }
    }
    (opens, closes)
}

fn rewrite_niri_kdl_output_scale(content: &str, output_name: &str, scale_str: &str) -> String {
    let mut new_lines = Vec::new();
    let mut in_target_output = false;
    let mut brace_depth: usize = 0;
    let mut replaced_in_existing = false;
    let mut target_header_line = String::new();
    let mut preserved_inner_lines: Vec<String> = Vec::new();
    let target_header = format!("output \"{}\"", output_name);

    for line in content.lines() {
        let code = strip_kdl_line_comment(line).trim();
        if !in_target_output {
            if let Some(rest) = code.strip_prefix(&target_header) {
                let rest_trimmed = rest.trim();
                let header_boundary = rest.is_empty()
                    || rest.starts_with(char::is_whitespace)
                    || rest.starts_with('{');
                if header_boundary && rest_trimmed == "{" {
                    in_target_output = true;
                    brace_depth = 1;
                    target_header_line = line.to_string();
                    preserved_inner_lines.clear();
                    continue;
                }
            }
            new_lines.push(line.to_string());
            continue;
        }

        let (opens, closes) = count_kdl_braces(code);
        let depth_before = brace_depth;
        brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);

        if brace_depth == 0 {
            in_target_output = false;
            if !replaced_in_existing {
                new_lines.push(target_header_line.clone());
                for inner in &preserved_inner_lines {
                    new_lines.push(inner.clone());
                }
                new_lines.push(format!("    scale {}", scale_str));
                new_lines.push(line.to_string());
                replaced_in_existing = true;
            }
        } else {
            let is_top_level_scale = depth_before == 1
                && opens == 0
                && closes == 0
                && (code == "scale" || code.starts_with("scale ") || code.starts_with("scale\t"));
            if !is_top_level_scale {
                preserved_inner_lines.push(line.to_string());
            }
        }
    }

    if in_target_output && !replaced_in_existing {
        new_lines.push(target_header_line);
        for inner in &preserved_inner_lines {
            new_lines.push(inner.clone());
        }
        new_lines.push(format!("    scale {}", scale_str));
        new_lines.push("}".to_string());
        replaced_in_existing = true;
    }

    if !replaced_in_existing {
        new_lines.push(format!(
            "\noutput \"{}\" {{\n    scale {}\n}}",
            output_name, scale_str
        ));
    }
    new_lines.join("\n") + "\n"
}

fn rewrite_niri_kdl_output_mode(
    content: &str,
    output_name: &str,
    mode_str: &str,
    is_custom: bool,
) -> String {
    let mode_line = if is_custom {
        format!("    mode \"{}\" custom=true", mode_str)
    } else {
        format!("    mode \"{}\"", mode_str)
    };
    let mut new_lines = Vec::new();
    let mut in_target_output = false;
    let mut brace_depth: usize = 0;
    let mut replaced_in_existing = false;
    let mut target_header_line = String::new();
    let mut preserved_inner_lines: Vec<String> = Vec::new();
    let target_header = format!("output \"{}\"", output_name);

    for line in content.lines() {
        let code = strip_kdl_line_comment(line).trim();
        if !in_target_output {
            if let Some(rest) = code.strip_prefix(&target_header) {
                let rest_trimmed = rest.trim();
                let header_boundary = rest.is_empty()
                    || rest.starts_with(char::is_whitespace)
                    || rest.starts_with('{');
                if header_boundary && rest_trimmed == "{" {
                    in_target_output = true;
                    brace_depth = 1;
                    target_header_line = line.to_string();
                    preserved_inner_lines.clear();
                    continue;
                }
            }
            new_lines.push(line.to_string());
            continue;
        }

        let (opens, closes) = count_kdl_braces(code);
        let depth_before = brace_depth;
        brace_depth = brace_depth.saturating_add(opens).saturating_sub(closes);

        if brace_depth == 0 {
            in_target_output = false;
            if !replaced_in_existing {
                new_lines.push(target_header_line.clone());
                new_lines.push(mode_line.clone());
                for inner in &preserved_inner_lines {
                    new_lines.push(inner.clone());
                }
                new_lines.push(line.to_string());
                replaced_in_existing = true;
            }
        } else {
            let is_top_level_mode = depth_before == 1
                && opens == 0
                && closes == 0
                && (code == "mode" || code.starts_with("mode ") || code.starts_with("mode\t"));
            if !is_top_level_mode {
                preserved_inner_lines.push(line.to_string());
            }
        }
    }

    if in_target_output && !replaced_in_existing {
        new_lines.push(target_header_line);
        new_lines.push(mode_line.clone());
        for inner in &preserved_inner_lines {
            new_lines.push(inner.clone());
        }
        new_lines.push("}".to_string());
        replaced_in_existing = true;
    }

    if !replaced_in_existing {
        new_lines.push(format!(
            "\noutput \"{}\" {{\n{}\n}}",
            output_name, mode_line
        ));
    }
    new_lines.join("\n") + "\n"
}

fn write_niri_kdl_both_paths(transform: impl Fn(&str) -> String) {
    if let Ok(home) = std::env::var("HOME") {
        let home_pb = PathBuf::from(home);
        let solar_kdl = home_pb.join(".config/solarui/niri.kdl");
        let niri_kdl = home_pb.join(".config/niri/config.kdl");

        if let Ok(content) = std::fs::read_to_string(&solar_kdl) {
            let updated = transform(&content);
            let _ = std::fs::write(&solar_kdl, &updated);
            // If ~/.config/niri/config.kdl is a separate regular file (not a symlink to solar_kdl), sync it too
            let is_symlink = std::fs::symlink_metadata(&niri_kdl)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            if !is_symlink && niri_kdl.exists() {
                let _ = std::fs::write(&niri_kdl, &updated);
            }
        } else if let Ok(content) = std::fs::read_to_string(&niri_kdl) {
            let updated = transform(&content);
            let _ = std::fs::write(&niri_kdl, &updated);
        }

        // Also sync We10X pre-snapshot niri_kdl if present so display mode/scale persists across shell switches!
        let snap_file = home_pb.join(".local/state/solarui/pre-we10x-snapshot.json");
        if snap_file.is_file() {
            if let Ok(bytes) = std::fs::read(&snap_file) {
                if let Ok(mut snap) =
                    serde_json::from_slice::<crate::we10x_profile::PreWe10xSnapshot>(&bytes)
                {
                    if !snap.niri_kdl.trim().is_empty() {
                        snap.niri_kdl = transform(&snap.niri_kdl);
                        if let Ok(out_bytes) = serde_json::to_vec_pretty(&snap) {
                            let _ = solar_common::persistence::atomic_write(&snap_file, &out_bytes);
                        }
                    }
                }
            }
        }
    }
}

pub fn apply_per_output_mode_dynamically(
    output_name: &str,
    mode_str: &str,
    is_custom: bool,
) -> anyhow::Result<()> {
    let primary_sub = if is_custom { "custom-mode" } else { "mode" };
    let out = Command::new("niri")
        .args(["msg", "output", output_name, primary_sub, mode_str])
        .output();

    let mut applied_custom = is_custom;
    match out {
        Ok(res) if res.status.success() => {}
        _ if is_custom => {
            // Fallback to standard `mode` in case the DRM/KMS driver lists the mode natively
            let retry = Command::new("niri")
                .args(["msg", "output", output_name, "mode", mode_str])
                .output()?;
            if !retry.status.success() {
                anyhow::bail!(
                    "{}",
                    String::from_utf8_lossy(&retry.stderr).trim().to_string()
                );
            }
            applied_custom = false;
        }
        Ok(res) => {
            anyhow::bail!(
                "{}",
                String::from_utf8_lossy(&res.stderr).trim().to_string()
            );
        }
        Err(e) => {
            anyhow::bail!("{}", e);
        }
    }

    let out_owned = output_name.to_string();
    let mode_owned = mode_str.to_string();
    write_niri_kdl_both_paths(|content| {
        rewrite_niri_kdl_output_mode(content, &out_owned, &mode_owned, applied_custom)
    });

    let mut cfg = SolarConfig::load();
    if let Some(pos) = cfg.display.outputs.iter().position(|o| o.name == output_name) {
        cfg.display.outputs[pos].mode = Some(mode_str.to_string());
        cfg.display.outputs[pos].custom_mode = applied_custom;
    } else {
        cfg.display.outputs.push(solar_common::MonitorOutputConfig {
            name: output_name.to_string(),
            scale: cfg.display.scale,
            mode: Some(mode_str.to_string()),
            custom_mode: applied_custom,
        });
    }
    let _ = cfg.save();
    Ok(())
}

pub fn apply_per_output_scale_dynamically(output_name: &str, scale: f64) {
    let scale_str = format!("{:.2}", scale);

    // 1. Run niri msg output <name> scale <scale_str>
    let _ = Command::new("niri")
        .args(["msg", "output", output_name, "scale", &scale_str])
        .status();

    // 2. Persist to ~/.config/solarui/niri.kdl (and ~/.config/niri/config.kdl if separate), preserving other output settings
    let out_owned = output_name.to_string();
    let sc_owned = scale_str.clone();
    write_niri_kdl_both_paths(|content| rewrite_niri_kdl_output_scale(content, &out_owned, &sc_owned));

    // 3. Update SolarConfig outputs list
    let mut cfg = SolarConfig::load();
    if let Some(pos) = cfg.display.outputs.iter().position(|o| o.name == output_name) {
        cfg.display.outputs[pos].scale = scale;
    } else {
        cfg.display.outputs.push(solar_common::MonitorOutputConfig {
            name: output_name.to_string(),
            scale,
            mode: None,
            custom_mode: false,
        });
    }
    let _ = cfg.save();
}

pub fn apply_display_scale_dynamically(scale: f64) {
    let scale_str = format!("{:.2}", scale);

    // 1. Update active Niri outputs (parse JSON keys + fallback prefix matching)
    let mut outputs_found = Vec::new();
    if let Ok(out) = Command::new("niri").args(["msg", "--json", "outputs"]).output() {
        if out.status.success() {
            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&out.stdout) {
                if let Some(obj) = val.as_object() {
                    for name in obj.keys() {
                        if !outputs_found.contains(name) {
                            outputs_found.push(name.clone());
                        }
                    }
                }
            }
            if outputs_found.is_empty() {
                let s = String::from_utf8_lossy(&out.stdout);
                for token in s.split('"') {
                    if token == "Virtual-1"
                        || token.starts_with("eDP")
                        || token.starts_with("HDMI")
                        || token.starts_with("DP-")
                        || token.starts_with("Virtual-")
                        || token.starts_with("DSI-")
                        || token.starts_with("DVI-")
                        || token.starts_with("WL-")
                    {
                        if !outputs_found.contains(&token.to_string()) {
                            outputs_found.push(token.to_string());
                        }
                    }
                }
            }
        }
    }
    if outputs_found.is_empty() {
        outputs_found.push("Virtual-1".to_string());
    }

    for op in &outputs_found {
        let _ = Command::new("niri")
            .args(["msg", "output", op, "scale", &scale_str])
            .status();
    }

    // 2. Persist to ~/.config/solarui/niri.kdl (and ~/.config/niri/config.kdl if separate)
    let outs_clone = outputs_found.clone();
    let sc_clone = scale_str.clone();
    write_niri_kdl_both_paths(|content| {
        let mut cur = content.to_string();
        for op in &outs_clone {
            cur = rewrite_niri_kdl_output_scale(&cur, op, &sc_clone);
        }
        cur
    });

    // 3. Set GTK font / interface scaling factor
    let _ = Command::new("gsettings")
        .args(["set", "org.gnome.desktop.interface", "text-scaling-factor", &scale_str])
        .status();

    // 4. Sync Caelestia configuration
    sync_caelestia_scale(scale);

    // 5. Update SolarConfig
    let mut cfg = SolarConfig::load();
    cfg.display.scale = scale;
    cfg.display.text_scale = scale;
    for op in &outputs_found {
        if let Some(pos) = cfg.display.outputs.iter().position(|o| o.name == *op) {
            cfg.display.outputs[pos].scale = scale;
        } else {
            cfg.display.outputs.push(solar_common::MonitorOutputConfig {
                name: op.clone(),
                scale,
                mode: None,
                custom_mode: false,
            });
        }
    }
    let _ = cfg.save();
}

fn build_omnibar_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 8);
    tab.append(&build_page_header(
        "Omnibar",
        "Uygulama başlatıcı, komut arama, hesap makinesi ve pano geçmişi",
    ));

    let sec = GtkBox::new(Orientation::Vertical, 12);
    sec.add_css_class("section-box");

    let title = Label::new(Some("Başlatıcı ve arama"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Uygulama başlatma, hesaplama, birim çevirme ve pano geçmişine (cb) hızlı erişim sağlar",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_xalign(0.0);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    // Signature keycap shortcut row
    let shortcut_row = GtkBox::new(Orientation::Horizontal, 20);
    shortcut_row.set_margin_top(4);
    shortcut_row.set_margin_bottom(4);

    let open_sc = GtkBox::new(Orientation::Horizontal, 8);
    let open_lbl = Label::new(Some("Başlatıcı"));
    open_lbl.add_css_class("kv-key");
    open_sc.append(&open_lbl);
    open_sc.append(&build_keycap_combo(&[&["Super", "Boşluk"]]));
    shortcut_row.append(&open_sc);

    let force_sc = GtkBox::new(Orientation::Horizontal, 8);
    let force_lbl = Label::new(Some("Zorla aç"));
    force_lbl.add_css_class("kv-key");
    force_sc.append(&force_lbl);
    force_sc.append(&build_keycap_combo(&[&["Super", "Shift", "Boşluk"]]));
    shortcut_row.append(&force_sc);
    sec.append(&shortcut_row);

    let sep1 = Separator::new(Orientation::Horizontal);
    sec.append(&sep1);

    let cfg = SolarConfig::load();

    // 0. Omnibar Etkinleştirme & Kabuk Başlatıcısı Davranışı
    let chk_enabled = CheckButton::with_label(
        "SolarUI Omnibar'ı etkinleştir (kapalıysa etkin kabuğun başlatıcısı açılır)",
    );
    chk_enabled.set_active(cfg.omnibar.enabled);
    chk_enabled.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.enabled = active;
        });
        if !active {
            let _ = crate::omnibar::close_running_omnibar();
        }
    });
    sec.append(&chk_enabled);

    let chk_override = CheckButton::with_label(
        "Caelestia ve end4-pC üzerinde de Omnibar kullan",
    );
    chk_override.set_active(cfg.omnibar.override_shell_launcher);
    chk_override.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.override_shell_launcher = active;
        });
    });
    sec.append(&chk_override);

    // 1. Web Arama Geçişi
    let chk_web = CheckButton::with_label("Çevrimiçi web arama sonuçlarını göster");
    chk_web.set_active(cfg.omnibar.enable_web_search);
    chk_web.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.enable_web_search = active;
        });
    });
    sec.append(&chk_web);

    // 2. Arama Motoru Seçimi
    let engine_box = GtkBox::new(Orientation::Horizontal, 12);
    let engine_lbl = Label::new(Some("Varsayılan arama motoru"));
    engine_lbl.add_css_class("setting-label");
    engine_lbl.set_hexpand(true);
    engine_lbl.set_halign(gtk4::Align::Start);
    engine_box.append(&engine_lbl);

    let engines = ["DuckDuckGo", "Google", "Brave", "Bing"];
    let str_list = StringList::new(&engines);
    let dropdown = DropDown::new(Some(str_list), None::<gtk4::Expression>);

    let current_pos = engines.iter().position(|e| *e == cfg.omnibar.search_engine).unwrap_or(0);
    dropdown.set_selected(current_pos as u32);

    dropdown.connect_selected_notify(move |dd| {
        let sel = dd.selected() as usize;
        if let Some(engine) = engines.get(sel) {
            let eng_str = engine.to_string();
            let _ = SolarConfig::update(|c| {
                c.omnibar.search_engine = eng_str;
            });
        }
    });
    engine_box.append(&dropdown);
    sec.append(&engine_box);

    let sep2 = Separator::new(Orientation::Horizontal);
    sec.append(&sep2);

    // Action Buttons
    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    let btn_test = Button::with_label("Omnibar'ı aç");
    btn_test.add_css_class("action-btn");
    btn_test.connect_clicked(|_| {
        let _ = Command::new("solar-shell")
            .args(["omnibar", "--force"])
            .spawn();
    });
    btn_row.append(&btn_test);

    let btn_shell_launcher = Button::with_label("Kabuk başlatıcısını aç");
    btn_shell_launcher.add_css_class("secondary-btn");
    btn_shell_launcher.connect_clicked(|_| {
        let _ = Command::new("solar-shell")
            .args(["route", "launcher"])
            .spawn();
    });
    btn_row.append(&btn_shell_launcher);

    sec.append(&btn_row);

    tab.append(&sec);

    // 3. Karakter ve BlazeAI Ayarları
    let _ = crate::blazeai::migrate_legacy_api_key_if_needed();
    let ai_cfg = SolarConfig::load();
    let ai_sec = GtkBox::new(Orientation::Vertical, 12);
    ai_sec.add_css_class("section-box");

    let ai_title = Label::new(Some("Karakter ve BlazeAI"));
    ai_title.add_css_class("section-title");
    ai_title.set_halign(gtk4::Align::Start);
    ai_sec.append(&ai_title);

    let ai_desc = Label::new(Some(
        "Sol paneldeki vektör karakteri, yapay zeka sağlayıcısını ve yerel hafıza tercihlerini yapılandırır",
    ));
    ai_desc.add_css_class("setting-subtext");
    ai_desc.set_wrap(true);
    ai_desc.set_xalign(0.0);
    ai_desc.set_halign(gtk4::Align::Start);
    ai_sec.append(&ai_desc);

    let chk_panel_open = CheckButton::with_label(
        "Sol karakter ve BlazeAI panelini varsayılan olarak açık tut",
    );
    chk_panel_open.set_active(ai_cfg.omnibar.companion_panel_open);
    chk_panel_open.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.companion_panel_open = active;
        });
    });
    ai_sec.append(&chk_panel_open);

    let syncing_ai_tab = Rc::new(Cell::new(false));

    // Companion Name row
    let cname_row = GtkBox::new(Orientation::Horizontal, 12);
    let cname_lbl = Label::new(Some("Karakter adı"));
    cname_lbl.add_css_class("setting-label");
    cname_lbl.set_hexpand(true);
    cname_lbl.set_halign(gtk4::Align::Start);
    cname_row.append(&cname_lbl);

    let cname_entry = Entry::new();
    cname_entry.set_width_chars(18);
    syncing_ai_tab.set(true);
    cname_entry.set_text(&ai_cfg.omnibar.effective_companion_name());
    syncing_ai_tab.set(false);
    cname_row.append(&cname_entry);

    let cname_reset_btn = Button::with_label("Varsayılan isme dön");
    cname_reset_btn.add_css_class("secondary-btn");
    cname_row.append(&cname_reset_btn);
    ai_sec.append(&cname_row);

    // Companion Shape row
    let shape_row = GtkBox::new(Orientation::Horizontal, 12);
    let shape_lbl = Label::new(Some("Karakter şekli"));
    shape_lbl.add_css_class("setting-label");
    shape_lbl.set_hexpand(true);
    shape_lbl.set_halign(gtk4::Align::Start);
    shape_row.append(&shape_lbl);

    let shape_labels: Vec<&str> = CompanionShape::ALL.iter().map(|s| s.label_tr()).collect();
    let shape_dd = DropDown::new(Some(StringList::new(&shape_labels)), None::<gtk4::Expression>);
    let cur_shape_idx = CompanionShape::ALL
        .iter()
        .position(|s| *s == ai_cfg.omnibar.companion_shape)
        .unwrap_or(1);
    shape_dd.set_selected(cur_shape_idx as u32);
    shape_dd.connect_selected_notify(|dd| {
        if let Some(shape) = CompanionShape::ALL.get(dd.selected() as usize).copied() {
            let _ = SolarConfig::update(|c| {
                c.omnibar.companion_shape = shape;
            });
        }
    });
    shape_row.append(&shape_dd);
    ai_sec.append(&shape_row);

    // Companion Expression row + live meaning caption
    let expr_row = GtkBox::new(Orientation::Horizontal, 12);
    let expr_lbl = Label::new(Some("İfade ve kişilik"));
    expr_lbl.add_css_class("setting-label");
    expr_lbl.set_hexpand(true);
    expr_lbl.set_halign(gtk4::Align::Start);
    expr_row.append(&expr_lbl);

    let expr_titles: Vec<String> = CompanionExpression::ALL
        .iter()
        .map(|e| format!("{} ({})", e.title_tr(), e.default_name()))
        .collect();
    let expr_title_refs: Vec<&str> = expr_titles.iter().map(|s| s.as_str()).collect();
    let expr_dd = DropDown::new(Some(StringList::new(&expr_title_refs)), None::<gtk4::Expression>);
    let cur_expr_idx = CompanionExpression::ALL
        .iter()
        .position(|e| *e == ai_cfg.omnibar.companion_expression)
        .unwrap_or(1);
    expr_dd.set_selected(cur_expr_idx as u32);
    expr_row.append(&expr_dd);
    ai_sec.append(&expr_row);

    let expr_meaning_lbl = Label::new(Some(ai_cfg.omnibar.companion_expression.meaning_tr()));
    expr_meaning_lbl.add_css_class("setting-subtext");
    expr_meaning_lbl.set_wrap(true);
    expr_meaning_lbl.set_xalign(0.0);
    expr_meaning_lbl.set_halign(gtk4::Align::Start);
    ai_sec.append(&expr_meaning_lbl);

    {
        let sync_flag = syncing_ai_tab.clone();
        cname_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let txt = e.text().to_string();
            let _ = SolarConfig::update(|c| {
                c.omnibar.set_custom_companion_name(&txt);
            });
        });
    }

    {
        let sync_flag = syncing_ai_tab.clone();
        let entry_c = cname_entry.clone();
        cname_reset_btn.connect_clicked(move |_| {
            let mut next_name = String::new();
            let _ = SolarConfig::update(|c| {
                c.omnibar.reset_companion_name();
                next_name = c.omnibar.effective_companion_name();
            });
            sync_flag.set(true);
            entry_c.set_text(&next_name);
            sync_flag.set(false);
        });
    }

    {
        let sync_flag = syncing_ai_tab.clone();
        let entry_c = cname_entry.clone();
        let meaning_c = expr_meaning_lbl.clone();
        expr_dd.connect_selected_notify(move |dd| {
            if let Some(expr) = CompanionExpression::ALL.get(dd.selected() as usize).copied() {
                let mut next_name = String::new();
                let mut is_custom = false;
                let _ = SolarConfig::update(|c| {
                    c.omnibar.set_companion_expression(expr);
                    next_name = c.omnibar.effective_companion_name();
                    is_custom = c.omnibar.companion_name_custom;
                });
                meaning_c.set_text(expr.meaning_tr());
                if !is_custom {
                    sync_flag.set(true);
                    entry_c.set_text(&next_name);
                    sync_flag.set(false);
                }
            }
        });
    }

    ai_sec.append(&Separator::new(Orientation::Horizontal));

    // AI Provider, Model, URL and 0600 Secrets
    let prov_row = GtkBox::new(Orientation::Horizontal, 12);
    let prov_lbl = Label::new(Some("BlazeAI sağlayıcısı"));
    prov_lbl.add_css_class("setting-label");
    prov_lbl.set_hexpand(true);
    prov_lbl.set_halign(gtk4::Align::Start);
    prov_row.append(&prov_lbl);

    let prov_labels: Vec<&str> = BlazeAiProvider::ALL
        .iter()
        .map(|p| p.display_name())
        .collect();
    let prov_dd = DropDown::new(Some(StringList::new(&prov_labels)), None::<gtk4::Expression>);
    let cur_prov_idx = BlazeAiProvider::ALL
        .iter()
        .position(|p| *p == ai_cfg.omnibar.ai_provider)
        .unwrap_or(0);
    prov_dd.set_selected(cur_prov_idx as u32);
    prov_row.append(&prov_dd);
    ai_sec.append(&prov_row);

    let model_row = GtkBox::new(Orientation::Horizontal, 12);
    let model_lbl = Label::new(Some("Model adı"));
    model_lbl.add_css_class("setting-label");
    model_lbl.set_hexpand(true);
    model_lbl.set_halign(gtk4::Align::Start);
    model_row.append(&model_lbl);

    let model_entry = Entry::new();
    model_entry.set_width_chars(24);
    syncing_ai_tab.set(true);
    model_entry.set_text(&ai_cfg.omnibar.effective_ai_model());
    syncing_ai_tab.set(false);
    model_row.append(&model_entry);
    ai_sec.append(&model_row);

    let url_row = GtkBox::new(Orientation::Horizontal, 12);
    let url_lbl = Label::new(Some("Özel API veya Ollama adresi"));
    url_lbl.add_css_class("setting-label");
    url_lbl.set_hexpand(true);
    url_lbl.set_halign(gtk4::Align::Start);
    url_row.append(&url_lbl);

    let url_entry = Entry::new();
    url_entry.set_width_chars(24);
    syncing_ai_tab.set(true);
    url_entry.set_text(&ai_cfg.omnibar.effective_ai_base_url());
    url_entry.set_sensitive(matches!(
        ai_cfg.omnibar.ai_provider,
        BlazeAiProvider::OpenAiCompatible | BlazeAiProvider::Ollama
    ));
    syncing_ai_tab.set(false);
    url_row.append(&url_entry);
    ai_sec.append(&url_row);

    let key_row = GtkBox::new(Orientation::Horizontal, 12);
    let key_lbl = Label::new(Some("API anahtarı (0600 yerel kasa)"));
    key_lbl.add_css_class("setting-label");
    key_lbl.set_hexpand(true);
    key_lbl.set_halign(gtk4::Align::Start);
    key_row.append(&key_lbl);

    let key_entry = PasswordEntry::builder()
        .placeholder_text("Anahtar girin (Ollama için gerekmez)")
        .show_peek_icon(true)
        .build();
    key_entry.set_width_chars(24);
    let init_secrets = crate::blazeai::BlazeAiSecrets::load();
    syncing_ai_tab.set(true);
    key_entry.set_text(init_secrets.key_for(ai_cfg.omnibar.ai_provider));
    key_entry.set_sensitive(ai_cfg.omnibar.ai_provider.requires_api_key());
    syncing_ai_tab.set(false);
    key_row.append(&key_entry);
    ai_sec.append(&key_row);

    {
        let sync_flag = syncing_ai_tab.clone();
        let m_ent = model_entry.clone();
        let u_ent = url_entry.clone();
        let k_ent = key_entry.clone();
        prov_dd.connect_selected_notify(move |dd| {
            if sync_flag.get() {
                return;
            }
            if let Some(provider) = BlazeAiProvider::ALL.get(dd.selected() as usize).copied() {
                sync_flag.set(true);
                let mut next_model = provider.default_model().to_string();
                let mut next_url = provider.default_base_url().to_string();
                let _ = SolarConfig::update(|c| {
                    c.omnibar.ai_provider = provider;
                    c.omnibar.ai_model = provider.default_model().to_string();
                    c.omnibar.ai_custom_base_url = provider.default_base_url().to_string();
                    next_model = c.omnibar.ai_model.clone();
                    next_url = c.omnibar.effective_ai_base_url();
                });
                let sec = crate::blazeai::BlazeAiSecrets::load();
                m_ent.set_text(&next_model);
                u_ent.set_text(&next_url);
                u_ent.set_sensitive(matches!(
                    provider,
                    BlazeAiProvider::OpenAiCompatible | BlazeAiProvider::Ollama
                ));
                k_ent.set_text(sec.key_for(provider));
                k_ent.set_sensitive(provider.requires_api_key());
                sync_flag.set(false);
            }
        });
    }

    {
        let sync_flag = syncing_ai_tab.clone();
        model_entry.connect_changed(move |e| {
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
        });
    }

    {
        let sync_flag = syncing_ai_tab.clone();
        url_entry.connect_changed(move |e| {
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
        let sync_flag = syncing_ai_tab.clone();
        key_entry.connect_changed(move |e| {
            if sync_flag.get() {
                return;
            }
            let cfg_now = SolarConfig::load();
            if !cfg_now.omnibar.ai_provider.requires_api_key() {
                return;
            }
            let mut sec = crate::blazeai::BlazeAiSecrets::load();
            sec.set_key_for(cfg_now.omnibar.ai_provider, &e.text());
            let _ = sec.save();
        });
    }

    let chk_ai_persist = CheckButton::with_label("Sohbet geçmişini koru");
    chk_ai_persist.set_active(ai_cfg.omnibar.ai_persist_chat);
    chk_ai_persist.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.ai_persist_chat = active;
        });
    });
    ai_sec.append(&chk_ai_persist);

    let chk_ai_mem = CheckButton::with_label("Uzun vadeli yerel hafıza");
    chk_ai_mem.set_active(ai_cfg.omnibar.ai_local_memory_enabled);
    chk_ai_mem.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.ai_local_memory_enabled = active;
        });
    });
    ai_sec.append(&chk_ai_mem);

    let chk_ai_guide = CheckButton::with_label("Yerleşik BlazeOS rehberini bağlama ekle");
    chk_ai_guide.set_active(ai_cfg.omnibar.ai_include_blazeos_guide);
    chk_ai_guide.connect_toggled(|cb| {
        let active = cb.is_active();
        let _ = SolarConfig::update(|c| {
            c.omnibar.ai_include_blazeos_guide = active;
        });
    });
    ai_sec.append(&chk_ai_guide);

    let clear_row = GtkBox::new(Orientation::Horizontal, 12);
    let clear_mem_btn = Button::with_label("Yerel sohbet ve hafıza verilerini temizle");
    clear_mem_btn.add_css_class("danger-btn");
    clear_row.append(&clear_mem_btn);

    let clear_status_lbl = Label::new(Some(""));
    clear_status_lbl.add_css_class("status-text");
    clear_status_lbl.set_halign(gtk4::Align::Start);
    clear_row.append(&clear_status_lbl);

    {
        let status_c = clear_status_lbl.clone();
        clear_mem_btn.connect_clicked(move |_| {
            let mut store = crate::blazeai::LocalMemoryStore::load(true);
            if store.clear_all_memory().is_ok() {
                status_c.set_text("Yerel veriler temizlendi");
            } else {
                status_c.set_text("Temizleme sırasında hata oluştu");
            }
        });
    }
    ai_sec.append(&clear_row);

    tab.append(&ai_sec);
    tab
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_niri_kdl_preserves_custom_output_inner_lines() {
        let sample = r#"output "eDP-1" {
    mode "2560x1600@165.000"
    scale 1.0
    transform "normal"
}
"#;
        let updated = rewrite_niri_kdl_output_scale(sample, "eDP-1", "1.25");
        assert!(updated.contains("mode \"2560x1600@165.000\""));
        assert!(updated.contains("transform \"normal\""));
        assert!(updated.contains("scale 1.25"));
        assert!(!updated.contains("scale 1.0"));
    }

    #[test]
    fn sentinel_report_uses_passed_and_total_keys() {
        let json = r#"{"passed":6,"total":7,"verified":true,"integrity_ok":true}"#;
        let val: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(parse_sentinel_posture_counts(&val), (6, 7));

        let legacy = r#"{"passed_count":5,"total_count":7}"#;
        let legacy_val: serde_json::Value = serde_json::from_str(legacy).unwrap();
        assert_eq!(parse_sentinel_posture_counts(&legacy_val), (5, 7));
    }

    #[test]
    fn rewrite_niri_kdl_handles_nested_braces_and_inline_comment() {
        let sample = "output \"eDP-1\" { // primary\n    hot-corners {\n        off\n    }\n    scale 1.0\n}\n";
        let updated = rewrite_niri_kdl_output_scale(sample, "eDP-1", "1.25");
        assert_eq!(updated.matches("output \"eDP-1\"").count(), 1);
        assert!(updated.contains("hot-corners {"));
        assert!(updated.contains("scale 1.25"));
        assert!(!updated.contains("scale 1.0"));
    }

    #[test]
    fn rewrite_niri_kdl_output_mode_preserves_scale_and_supports_custom_flag() {
        let sample = "output \"Virtual-1\" {\n    scale 1.00\n}\n";
        let hw = rewrite_niri_kdl_output_mode(sample, "Virtual-1", "1920x1080@60.000", false);
        assert!(hw.contains("mode \"1920x1080@60.000\""));
        assert!(!hw.contains("custom=true"));
        assert!(hw.contains("scale 1.00"));

        let custom = rewrite_niri_kdl_output_mode(&hw, "Virtual-1", "1600x900@75.000", true);
        assert!(custom.contains("mode \"1600x900@75.000\" custom=true"));
        assert!(!custom.contains("1920x1080@60.000"));
        assert!(custom.contains("scale 1.00"));
    }
}

