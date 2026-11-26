use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, Label, Orientation, ScrolledWindow, SizeGroup,
    SizeGroupMode, Window,
};
use solar_common::SolarConfig;

use crate::living_flow::{
    apply_living_flow_theme, build_living_flow_topbar, load_png_emblem,
};

const WELCOME_EMBLEM_PNG: &[u8] =
    include_bytes!("../../../data/icons/living-flow/solar-welcome-64.png");

const WELCOME_EXTRA_CSS: &str = r#"
    .welcome-body {
        padding: 16px 24px 20px 24px;
    }

    .welcome-header {
        margin-bottom: 16px;
    }

    .welcome-title {
        color: #ededed;
        font-size: 20px;
        font-weight: 600;
        letter-spacing: 0px;
    }

    .welcome-subtitle {
        color: #a1a1a6;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
    }

    .shortcut-grid-card {
        background-color: #19191b;
        background-image: none;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 12px;
        padding: 8px 12px;
        margin-bottom: 16px;
        box-shadow: none;
    }

    .shortcut-row {
        padding: 4px 8px;
        border-bottom: 1px solid rgba(237, 237, 237, 0.06);
        transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
    }

    .shortcut-row:last-child {
        border-bottom: none;
    }

    .shortcut-row:hover {
        background-color: rgba(237, 237, 237, 0.03);
    }

    .shortcut-desc {
        color: #a1a1a6;
        font-size: 13px;
        font-weight: 400;
        letter-spacing: 0px;
    }
"#;

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

pub fn launch_welcome_window() {
    glib::set_prgname(Some("solar-welcome"));
    glib::set_application_name("SolarUI");

    if let Err(err) = gtk4::init() {
        eprintln!("Failed to initialize GTK4: {}", err);
        return;
    }

    let main_loop = glib::MainLoop::new(None, false);
    build_welcome_ui(main_loop.clone());
    main_loop.run();
}

fn build_welcome_ui(main_loop: glib::MainLoop) {
    apply_living_flow_theme(WELCOME_EXTRA_CSS);

    let window = Window::builder()
        .title("SolarUI Masaüstü Rehberi")
        .decorated(false)
        .default_width(750)
        .default_height(590)
        .resizable(false)
        .build();

    let loop_quit = main_loop.clone();
    window.connect_close_request(move |_| {
        loop_quit.quit();
        glib::Propagation::Proceed
    });

    let key_controller = gtk4::EventControllerKey::new();
    let win_weak_key = window.downgrade();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape || key == gdk::Key::Return || key == gdk::Key::KP_Enter {
            if let Some(win) = win_weak_key.upgrade() {
                win.close();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key_controller);

    let shell_box = GtkBox::new(Orientation::Vertical, 0);
    shell_box.add_css_class("lf-window-shell");

    let top_emblem = load_png_emblem(WELCOME_EMBLEM_PNG, 24);
    let loop_top = main_loop.clone();
    let topbar = build_living_flow_topbar(
        &window,
        Some(top_emblem),
        "SolarUI",
        "Blaze SolarEvolution",
        None,
        Some(Box::new(move || loop_top.quit())),
    );
    shell_box.append(&topbar);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("welcome-body");
    body.set_vexpand(true);

    // Calm page header directly on graphite surface
    let header = GtkBox::new(Orientation::Vertical, 4);
    header.add_css_class("welcome-header");

    let title = Label::new(Some("SolarUI kısayolları"));
    title.add_css_class("welcome-title");
    title.set_halign(gtk4::Align::Start);
    header.append(&title);

    let subtitle = Label::new(Some("Sık kullanılan masaüstü, pencere ve oturum kısayolları"));
    subtitle.add_css_class("welcome-subtitle");
    subtitle.set_halign(gtk4::Align::Start);
    header.append(&subtitle);
    body.append(&header);

    // Signature keycap shortcut sheet with aligned two-column rows
    let scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .build();
    scroll.add_css_class("shortcut-grid-card");

    let rows_box = GtkBox::new(Orientation::Vertical, 0);
    let key_col_group = SizeGroup::new(SizeGroupMode::Horizontal);

    let shortcuts: &[(&[&[&str]], &str)] = &[
        (&[&["Super", "Space"]], "Omnibar uygulama ve sistem araması"),
        (&[&["Super", "Shift", "Space"]], "Omnibar'ı doğrudan aç"),
        (&[&["Super", "G"]], "GameZone oyun başlatıcısı"),
        (
            &[&["Super", "I"], &["Super", "Shift", "S"]],
            "SolarUI masaüstü ve kabuk ayarları",
        ),
        (&[&["Super", "S"]], "Kontrol merkezi, ses ve ağ"),
        (&[&["Super", "Enter"]], "Alacritty uçbirim"),
        (&[&["Super", "T"], &["Super", "F"]], "Yüzen pencere modunu aç veya kapat"),
        (
            &[&["Super", "Sol tık"], &["Sağ tık"]],
            "Pencereyi taşı veya boyutlandır",
        ),
        (
            &[&["Super", "Y"], &["Ctrl", "↓"]],
            "Pencereyi görev çubuğuna küçült",
        ),
        (&[&["Alt", "F4"], &["Super", "Q"]], "Etkin pencereyi kapat"),
        (&[&["Super", "Alt", "K"]], "SolarEvolution retro modu"),
        (&[&["Super", "Shift", "Q"]], "Oturum ve güç menüsü"),
    ];

    for (chords, desc) in shortcuts {
        let row = GtkBox::new(Orientation::Horizontal, 20);
        row.add_css_class("shortcut-row");

        let key_wrap = GtkBox::new(Orientation::Horizontal, 0);
        key_wrap.set_halign(gtk4::Align::Start);
        key_wrap.set_valign(gtk4::Align::Center);
        key_wrap.append(&build_keycap_combo(chords));
        key_col_group.add_widget(&key_wrap);

        let desc_label = Label::new(Some(desc));
        desc_label.add_css_class("shortcut-desc");
        desc_label.set_halign(gtk4::Align::Start);
        desc_label.set_valign(gtk4::Align::Center);
        desc_label.set_hexpand(true);
        desc_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);

        row.append(&key_wrap);
        row.append(&desc_label);
        rows_box.append(&row);
    }
    scroll.set_child(Some(&rows_box));
    body.append(&scroll);

    // Bottom action row
    let bottom_box = GtkBox::new(Orientation::Horizontal, 8);
    bottom_box.set_valign(gtk4::Align::Center);

    let cfg = SolarConfig::load();
    let check = CheckButton::with_label("Açılışta göster");
    check.set_active(cfg.shortcuts_hud.show_at_startup);
    check.set_hexpand(true);
    check.connect_toggled(move |c| {
        let mut updated = SolarConfig::load();
        updated.shortcuts_hud.show_at_startup = c.is_active();
        let _ = updated.save();
    });
    bottom_box.append(&check);

    let has_solar_installer = std::path::Path::new("/usr/bin/solar-installer").exists();
    let has_liveinst = std::path::Path::new("/usr/bin/liveinst").exists();
    if has_solar_installer || has_liveinst {
        let inst_btn = Button::with_label("Sistemi kur");
        inst_btn.add_css_class("lf-btn-glass");
        let win_weak_inst = window.downgrade();
        let loop_inst = main_loop.clone();
        inst_btn.connect_clicked(move |_| {
            if has_solar_installer {
                let _ = std::process::Command::new("solar-shell")
                    .arg("installer")
                    .spawn();
            } else {
                let _ = std::process::Command::new("/usr/bin/liveinst").spawn();
            }
            if let Some(win) = win_weak_inst.upgrade() {
                win.close();
            }
            loop_inst.quit();
        });
        bottom_box.append(&inst_btn);
    }

    let store_btn = Button::with_label("FireHub");
    store_btn.add_css_class("lf-btn-glass");
    store_btn.connect_clicked(move |_| {
        let _ = std::process::Command::new("firehub").spawn();
    });
    bottom_box.append(&store_btn);

    let start_btn = Button::with_label("Masaüstüne geç");
    start_btn.add_css_class("lf-btn-primary");
    let win_weak = window.downgrade();
    let loop_clone = main_loop.clone();
    start_btn.connect_clicked(move |_| {
        if let Some(win) = win_weak.upgrade() {
            win.close();
        }
        loop_clone.quit();
    });
    bottom_box.append(&start_btn);

    body.append(&bottom_box);
    shell_box.append(&body);

    window.set_child(Some(&shell_box));
    start_btn.set_can_focus(true);
    window.present();
    start_btn.grab_focus();
}
