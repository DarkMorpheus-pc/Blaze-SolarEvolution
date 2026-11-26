use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CssProvider, Grid, IconTheme, Label, Orientation, Window,
};
use std::process::Command;

pub fn launch_snap_window() {
    glib::set_prgname(Some("solar-snap"));
    glib::set_application_name("SolarUI");

    if let Err(err) = gtk4::init() {
        eprintln!("Failed to initialize GTK4 for Snap HUD: {}", err);
        return;
    }

    // Find the window that was focused before opening the HUD
    let target_win_id = get_target_window_id();

    let main_loop = glib::MainLoop::new(None, false);
    build_snap_ui(main_loop.clone(), target_win_id);
    main_loop.run();
}

fn get_target_window_id() -> Option<u64> {
    let output = Command::new("niri")
        .args(["msg", "-j", "windows"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let windows: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).ok()?;
    for win in windows {
        let is_focused = win.get("is_focused").and_then(|v| v.as_bool()).unwrap_or(false);
        let app_id = win.get("app_id").and_then(|v| v.as_str()).unwrap_or("");
        if is_focused && !app_id.contains("solar-shell") && !app_id.contains("solar-snap") {
            return win.get("id").and_then(|v| v.as_u64());
        }
    }
    None
}

fn execute_snap_action(action: &'static str, target_id: Option<u64>) {
    let target = target_id;
    std::thread::spawn(move || {
        if let Some(id) = target {
            // First ensure target window is focused
            let _ = Command::new("niri")
                .args(["msg", "action", "focus-window", "--id", &id.to_string()])
                .status();
        }

        match action {
            // Template 1: 50 / 50
            "half-left" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-left"]).status();
            }
            "half-right" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-right"]).status();
            }

            // Template 2: 70 / 30
            "two-thirds-left" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "67%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-left"]).status();
            }
            "one-third-right" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "33%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-right"]).status();
            }

            // Template 3: 33 / 33 / 33
            "third-left" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "33.33%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-left"]).status();
            }
            "third-center" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "33.33%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "center-column"]).status();
            }
            "third-right" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "33.33%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-right"]).status();
            }

            // Template 4: 50% Left Full, Right Split Top/Bottom
            "split-left-full" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "reset-window-height"]).status();
            }
            "split-right-top" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "-50%"]).status();
            }
            "split-right-bottom" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "-50%"]).status();
            }

            // Template 5: 2x2 Grid (Four Quadrants)
            "quad-top-left" => {
                let _ = Command::new("niri").args(["msg", "action", "move-window-to-floating"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-floating-window", "-x", "-500", "-y", "-300"]).status();
            }
            "quad-top-right" => {
                let _ = Command::new("niri").args(["msg", "action", "move-window-to-floating"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-floating-window", "-x", "+500", "-y", "-300"]).status();
            }
            "quad-bottom-left" => {
                let _ = Command::new("niri").args(["msg", "action", "move-window-to-floating"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-floating-window", "-x", "-500", "-y", "+300"]).status();
            }
            "quad-bottom-right" => {
                let _ = Command::new("niri").args(["msg", "action", "move-window-to-floating"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "set-window-height", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-floating-window", "-x", "+500", "-y", "+300"]).status();
            }

            // Template 6: Center Focus (25% / 50% / 25%)
            "focus-left-sidebar" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "25%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-left"]).status();
            }
            "focus-center-main" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "50%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "center-column"]).status();
            }
            "focus-right-sidebar" => {
                let _ = Command::new("niri").args(["msg", "action", "set-column-width", "25%"]).status();
                let _ = Command::new("niri").args(["msg", "action", "move-column-right"]).status();
            }

            _ => {}
        }
    });
}

fn build_snap_ui(main_loop: glib::MainLoop, target_win_id: Option<u64>) {
    let provider = CssProvider::new();
    let css = r#"
        window {
            background-color: transparent;
        }

        *:focus-visible {
            outline: 2px solid rgba(224, 87, 60, 0.65);
            outline-offset: 2px;
        }

        .snap-card {
            background-color: #141415;
            border: 1px solid rgba(237, 237, 237, 0.12);
            border-radius: 16px;
            padding: 16px;
            box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
        }

        .snap-header-label {
            color: #ededed;
            font-size: 14px;
            font-weight: 500;
            letter-spacing: 0px;
        }

        .snap-close-btn {
            background-color: transparent;
            background-image: none;
            color: #85858b;
            border: 1px solid transparent;
            border-radius: 8px;
            min-width: 28px;
            min-height: 28px;
            padding: 0px;
            font-size: 13px;
            font-weight: 400;
            box-shadow: none;
            transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .snap-close-btn image {
            -gtk-icon-size: 16px;
        }

        .snap-close-btn:hover {
            background-color: #232325;
            border-color: rgba(229, 72, 77, 0.35);
            color: #e5484d;
        }

        .snap-close-btn:active {
            transform: scale(0.98);
        }

        .snap-template-frame {
            background-color: #19191b;
            border: 1px solid rgba(237, 237, 237, 0.08);
            border-radius: 8px;
            padding: 4px;
        }

        .snap-zone {
            background-color: #232325;
            background-image: none;
            border: 1px solid rgba(237, 237, 237, 0.10);
            border-radius: 4px;
            box-shadow: none;
            padding: 0px;
            margin: 0px;
            transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                        transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        .snap-zone:hover {
            background-color: #c83b22;
            border-color: #e0573c;
            box-shadow: none;
        }

        .snap-zone:active {
            transform: scale(0.98);
        }
    "#;
    provider.load_from_string(css);
    let display = gdk::Display::default().expect("No default display");
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let window = Window::builder()
        .title("Pencere yerleşimi")
        .default_width(452)
        .default_height(268)
        .resizable(false)
        .decorated(false)
        .build();

    let loop_quit = main_loop.clone();
    window.connect_close_request(move |_| {
        loop_quit.quit();
        glib::Propagation::Proceed
    });

    // Escape key closes HUD
    let key_controller = gtk4::EventControllerKey::new();
    let loop_quit_key = main_loop.clone();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gdk::Key::Escape {
            loop_quit_key.quit();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    let root_card = GtkBox::new(Orientation::Vertical, 12);
    root_card.add_css_class("snap-card");

    // Header with title and symbolic close button
    let header_box = GtkBox::new(Orientation::Horizontal, 8);
    let title_label = Label::new(Some("Pencere yerleşimi"));
    title_label.add_css_class("snap-header-label");
    title_label.set_hexpand(true);
    title_label.set_halign(gtk4::Align::Start);
    title_label.set_valign(gtk4::Align::Center);
    header_box.append(&title_label);

    let icon_theme = IconTheme::for_display(&display);
    let close_btn = if icon_theme.has_icon("window-close-symbolic") {
        Button::from_icon_name("window-close-symbolic")
    } else {
        Button::with_label("✕")
    };
    close_btn.add_css_class("snap-close-btn");
    close_btn.set_tooltip_text(Some("Kapat"));
    let loop_quit_btn = main_loop.clone();
    close_btn.connect_clicked(move |_| {
        loop_quit_btn.quit();
    });
    header_box.append(&close_btn);
    root_card.append(&header_box);

    // 6 Template Cards Grid (3 Columns x 2 Rows)
    let grid = Grid::new();
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_hexpand(true);
    grid.set_vexpand(true);

    // Helper closure to create zone buttons
    let create_zone = |action_id: &'static str,
                       tooltip: &'static str,
                       target_id: Option<u64>,
                       loop_handle: glib::MainLoop|
     -> Button {
        let btn = Button::new();
        btn.add_css_class("snap-zone");
        btn.set_tooltip_text(Some(tooltip));
        btn.connect_clicked(move |_| {
            execute_snap_action(action_id, target_id);
            loop_handle.quit();
        });
        btn
    };

    // Template 1: [ 1/2 | 1/2 ]
    {
        let frame = GtkBox::new(Orientation::Horizontal, 4);
        frame.add_css_class("snap-template-frame");
        frame.set_size_request(128, 84);

        let z1 = create_zone("half-left", "Sol yarı (%50)", target_win_id, main_loop.clone());
        z1.set_hexpand(true);
        z1.set_vexpand(true);
        frame.append(&z1);

        let z2 = create_zone("half-right", "Sağ yarı (%50)", target_win_id, main_loop.clone());
        z2.set_hexpand(true);
        z2.set_vexpand(true);
        frame.append(&z2);

        grid.attach(&frame, 0, 0, 1, 1);
    }

    // Template 2: [ 67% | 33% ]
    {
        let frame = GtkBox::new(Orientation::Horizontal, 4);
        frame.add_css_class("snap-template-frame");
        frame.set_size_request(128, 84);

        let z1 = create_zone(
            "two-thirds-left",
            "Sol geniş (%67)",
            target_win_id,
            main_loop.clone(),
        );
        z1.set_hexpand(true);
        z1.set_vexpand(true);
        z1.set_size_request(80, 76);
        frame.append(&z1);

        let z2 = create_zone(
            "one-third-right",
            "Sağ dar (%33)",
            target_win_id,
            main_loop.clone(),
        );
        z2.set_hexpand(true);
        z2.set_vexpand(true);
        z2.set_size_request(36, 76);
        frame.append(&z2);

        grid.attach(&frame, 1, 0, 1, 1);
    }

    // Template 3: [ 33% | 33% | 33% ]
    {
        let frame = GtkBox::new(Orientation::Horizontal, 4);
        frame.add_css_class("snap-template-frame");
        frame.set_size_request(128, 84);

        let z1 = create_zone("third-left", "Sol sütun (%33)", target_win_id, main_loop.clone());
        z1.set_hexpand(true);
        z1.set_vexpand(true);
        frame.append(&z1);

        let z2 = create_zone(
            "third-center",
            "Orta sütun (%33)",
            target_win_id,
            main_loop.clone(),
        );
        z2.set_hexpand(true);
        z2.set_vexpand(true);
        frame.append(&z2);

        let z3 = create_zone(
            "third-right",
            "Sağ sütun (%33)",
            target_win_id,
            main_loop.clone(),
        );
        z3.set_hexpand(true);
        z3.set_vexpand(true);
        frame.append(&z3);

        grid.attach(&frame, 2, 0, 1, 1);
    }

    // Template 4: [ 50% Left Full | 50% Right Split (Top/Bottom) ]
    {
        let frame = GtkBox::new(Orientation::Horizontal, 4);
        frame.add_css_class("snap-template-frame");
        frame.set_size_request(128, 84);

        let z_left = create_zone(
            "split-left-full",
            "Sol tam (%50)",
            target_win_id,
            main_loop.clone(),
        );
        z_left.set_hexpand(true);
        z_left.set_vexpand(true);
        frame.append(&z_left);

        let right_box = GtkBox::new(Orientation::Vertical, 4);
        right_box.set_hexpand(true);
        right_box.set_vexpand(true);

        let z_top = create_zone("split-right-top", "Sağ üst", target_win_id, main_loop.clone());
        z_top.set_hexpand(true);
        z_top.set_vexpand(true);
        right_box.append(&z_top);

        let z_bottom = create_zone(
            "split-right-bottom",
            "Sağ alt",
            target_win_id,
            main_loop.clone(),
        );
        z_bottom.set_hexpand(true);
        z_bottom.set_vexpand(true);
        right_box.append(&z_bottom);

        frame.append(&right_box);

        grid.attach(&frame, 0, 1, 1, 1);
    }

    // Template 5: 2x2 Grid (Four Quadrants)
    {
        let frame = Grid::new();
        frame.add_css_class("snap-template-frame");
        frame.set_column_spacing(4);
        frame.set_row_spacing(4);
        frame.set_size_request(128, 84);

        let z_tl = create_zone("quad-top-left", "Sol üst çeyrek", target_win_id, main_loop.clone());
        z_tl.set_hexpand(true);
        z_tl.set_vexpand(true);
        frame.attach(&z_tl, 0, 0, 1, 1);

        let z_tr = create_zone("quad-top-right", "Sağ üst çeyrek", target_win_id, main_loop.clone());
        z_tr.set_hexpand(true);
        z_tr.set_vexpand(true);
        frame.attach(&z_tr, 1, 0, 1, 1);

        let z_bl = create_zone(
            "quad-bottom-left",
            "Sol alt çeyrek",
            target_win_id,
            main_loop.clone(),
        );
        z_bl.set_hexpand(true);
        z_bl.set_vexpand(true);
        frame.attach(&z_bl, 0, 1, 1, 1);

        let z_br = create_zone(
            "quad-bottom-right",
            "Sağ alt çeyrek",
            target_win_id,
            main_loop.clone(),
        );
        z_br.set_hexpand(true);
        z_br.set_vexpand(true);
        frame.attach(&z_br, 1, 1, 1, 1);

        grid.attach(&frame, 1, 1, 1, 1);
    }

    // Template 6: Center Focus (25% / 50% / 25%)
    {
        let frame = GtkBox::new(Orientation::Horizontal, 4);
        frame.add_css_class("snap-template-frame");
        frame.set_size_request(128, 84);

        let z1 = create_zone(
            "focus-left-sidebar",
            "Sol yan sütun (%25)",
            target_win_id,
            main_loop.clone(),
        );
        z1.set_hexpand(true);
        z1.set_vexpand(true);
        z1.set_size_request(28, 76);
        frame.append(&z1);

        let z2 = create_zone(
            "focus-center-main",
            "Orta ana alan (%50)",
            target_win_id,
            main_loop.clone(),
        );
        z2.set_hexpand(true);
        z2.set_vexpand(true);
        z2.set_size_request(56, 76);
        frame.append(&z2);

        let z3 = create_zone(
            "focus-right-sidebar",
            "Sağ yan sütun (%25)",
            target_win_id,
            main_loop.clone(),
        );
        z3.set_hexpand(true);
        z3.set_vexpand(true);
        z3.set_size_request(28, 76);
        frame.append(&z3);

        grid.attach(&frame, 2, 1, 1, 1);
    }

    root_card.append(&grid);
    window.set_child(Some(&root_card));
    window.present();
}
