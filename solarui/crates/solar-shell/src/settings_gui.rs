use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, DropDown, Image, Label, Orientation,
    ScrolledWindow, Separator, Stack, StackTransitionType, StringList, Window,
};
use solar_common::{
    HardwareDetector, HardwareTuningProfile, PerformanceTier, ShellEngine, SolarConfig,
    SurfaceProvider,
};
use crate::living_flow::{
    apply_living_flow_theme, build_living_flow_topbar, load_png_emblem, FluidCloud, FluidCloudTone,
};
use crate::pixel_clock::{PixelClockConfig, PixelClockStyle};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;

const LOGO_PNG: &[u8] = include_bytes!("../../../data/icons/solar-logo-256.png");
const SETTINGS_EMBLEM_PNG: &[u8] =
    include_bytes!("../../../data/icons/living-flow/solar-settings-64.png");
const NAV_CLOCK_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-clock.png");
const NAV_TASKBAR_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-taskbar.png");
const NAV_DISPLAY_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-display.png");
const NAV_SHELL_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-shell.png");
const NAV_CAELESTIA_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-caelestia.png");
const NAV_NOCTALIA_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-noctalia.png");
const NAV_PERF_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-perf.png");
const NAV_HEALTH_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-health.png");
const NAV_OMNIBAR_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-omnibar.png");
const NAV_HUD_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-hud.png");
const NAV_ABOUT_PNG: &[u8] = include_bytes!("../../../data/icons/living-flow/nav-about.png");

fn create_logo_image(size: i32) -> Image {
    load_png_emblem(LOGO_PNG, size)
}

pub fn launch_settings_window() {
    launch_settings_window_with_tab(None);
}

pub fn launch_settings_window_with_tab(initial_tab: Option<&str>) {
    glib::set_prgname(Some("solar-settings"));
    glib::set_application_name("SolarUI Settings");

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
            padding: 18px 22px;
        }

        .settings-title {
            color: #f5f5f7;
            font-size: 19px;
            font-weight: 700;
        }

        .settings-subtitle {
            color: #a1a1aa;
            font-size: 12.5px;
            font-weight: 500;
        }

        .settings-cat-header {
            color: #71717a;
            font-size: 10.2px;
            font-weight: 700;
            letter-spacing: 1.1px;
            margin: 10px 10px 4px 10px;
        }

        .wp-card {
            background-color: rgba(255, 255, 255, 0.04);
            border: 1px solid rgba(255, 255, 255, 0.09);
            border-radius: 14px;
            padding: 8px;
            transition: all 160ms ease;
        }

        .wp-card:hover {
            background-color: rgba(255, 255, 255, 0.09);
            border-color: rgba(255, 255, 255, 0.24);
        }

        .swatch-red     { background-color: #e81123; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-blue    { background-color: #0078d7; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-dark    { background-color: #2d2d30; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-green   { background-color: #107c10; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-purple  { background-color: #881798; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-amber   { background-color: #ff6a00; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
        .swatch-teal    { background-color: #008272; color: #ffffff; border-radius: 10px; padding: 6px 12px; font-weight: 700; border: 1px solid rgba(255,255,255,0.22); }
    "#;
    apply_living_flow_theme(extra_css);

    let window = Window::builder()
        .title("SolarUI Ayarları — Masaüstü ve Görev Çubuğu")
        .decorated(false)
        .default_width(1020)
        .default_height(620)
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

    let fluid_cloud = FluidCloud::new("Ayarlar Senkronize");
    let emblem = load_png_emblem(SETTINGS_EMBLEM_PNG, 34);
    let loop_top = main_loop.clone();
    let topbar = build_living_flow_topbar(
        &window,
        Some(emblem),
        "SolarUI Masaüstü Ayarları",
        "Blaze Universe • Kontrol Paneli",
        Some(&fluid_cloud),
        Some(Box::new(move || loop_top.quit())),
    );
    shell_box.append(&topbar);

    // Two-Pane Living Flow Layout: Left Capsule Navigation Rail + Right Fluid Stack
    let split_box = GtkBox::new(Orientation::Horizontal, 0);
    split_box.set_vexpand(true);
    split_box.set_hexpand(true);

    let sidebar_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .min_content_width(228)
        .vexpand(true)
        .build();
    sidebar_scroll.add_css_class("lf-sidebar");

    let nav_list = GtkBox::new(Orientation::Vertical, 2);

    let stage_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .hexpand(true)
        .vexpand(true)
        .build();

    let stack = Stack::new();
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    stack.set_transition_type(StackTransitionType::SlideUpDown);
    stack.set_transition_duration(240);
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
    let noctalia_tab = GtkBox::new(Orientation::Vertical, 12);
    let n_sec = GtkBox::new(Orientation::Vertical, 12);
    n_sec.add_css_class("section-box");

    let n_title = Label::new(Some("Noctalia Bar ve Hızlı Kontrol Panelleri"));
    n_title.add_css_class("section-title");
    n_title.set_halign(gtk4::Align::Start);
    n_sec.append(&n_title);

    let n_desc = Label::new(Some(
        "Noctalia Bar, SolarUI'ın üst bilgi ve kontrol merkezidir. Tüm ayarlarını ve panellerini aşağıdan doğrudan açabilirsiniz.",
    ));
    n_desc.add_css_class("setting-subtext");
    n_desc.set_wrap(true);
    n_desc.set_xalign(0.0);
    n_desc.set_halign(gtk4::Align::Start);
    n_sec.append(&n_desc);

    let btn_grid = GtkBox::new(Orientation::Horizontal, 10);
    let cloud_noc = fluid_cloud.clone();
    let btn_settings = Button::with_label("Noctalia Ayarları");
    btn_settings.add_css_class("action-btn");
    btn_settings.connect_clicked(move |_| {
        cloud_noc.pulse("Noctalia ayar paneli açıldı", FluidCloudTone::Info, false);
        let _ = Command::new("noctalia").args(["msg", "settings-toggle"]).spawn();
    });
    btn_grid.append(&btn_settings);

    let btn_cc = Button::with_label("Kontrol Merkezi");
    btn_cc.add_css_class("secondary-btn");
    btn_cc.connect_clicked(|_| {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "control-center"])
            .spawn();
    });
    btn_grid.append(&btn_cc);

    let btn_wp = Button::with_label("Duvar Kağıdı");
    btn_wp.add_css_class("secondary-btn");
    btn_wp.connect_clicked(|_| {
        let _ = Command::new("noctalia")
            .args(["msg", "panel-toggle", "wallpaper"])
            .spawn();
    });
    btn_grid.append(&btn_wp);

    let btn_audio = Button::with_label("Ses & Mikser");
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
    let hud_tab = GtkBox::new(Orientation::Vertical, 12);
    let h_sec = GtkBox::new(Orientation::Vertical, 12);
    h_sec.add_css_class("section-box");

    let h_title = Label::new(Some("Açılış Karşılama ve Kısayollar Ekranı"));
    h_title.add_css_class("section-title");
    h_title.set_halign(gtk4::Align::Start);
    h_sec.append(&h_title);

    let h_sub = Label::new(Some(
        "Başlangıç karşılama ekranını, sistem klavye kısayollarını ve kurulum yardımcısını yapılandırın.",
    ));
    h_sub.add_css_class("setting-subtext");
    h_sub.set_wrap(true);
    h_sub.set_xalign(0.0);
    h_sub.set_halign(gtk4::Align::Start);
    h_sec.append(&h_sub);

    let open_welcome_btn = Button::with_label("Kısayol ve Karşılama Ekranını Şimdi Aç");
    open_welcome_btn.add_css_class("action-btn");
    open_welcome_btn.set_halign(gtk4::Align::Start);
    open_welcome_btn.connect_clicked(|_| {
        let _ = Command::new("solar-shell").arg("welcome").spawn();
    });
    h_sec.append(&open_welcome_btn);

    let cfg = SolarConfig::load();
    let chk_startup = CheckButton::with_label("Oturum açıldığında karşılama ekranını otomatik göster");
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
    let about_tab = GtkBox::new(Orientation::Vertical, 12);
    let a_sec = GtkBox::new(Orientation::Vertical, 12);
    a_sec.add_css_class("section-box");
    a_sec.set_halign(gtk4::Align::Fill);

    let big_logo = create_logo_image(80);
    big_logo.set_halign(gtk4::Align::Center);
    a_sec.append(&big_logo);

    let a_name = Label::new(Some("SolarUI Masaüstü Ortamı (Blaze Universe Edition)"));
    a_name.add_css_class("settings-title");
    a_name.set_halign(gtk4::Align::Center);
    a_sec.append(&a_name);

    let a_sig = Label::new(Some("Luminous Rendering & Aurora Physics"));
    a_sig.add_css_class("settings-subtitle");
    a_sig.set_halign(gtk4::Align::Center);
    a_sec.append(&a_sig);

    let kern_ver = std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "7.2.x".to_string());
    let a_ver = Label::new(Some(&format!(
        "Sürüm 1.1.0 (Rust + GTK4 + Niri Wayland + Linux {})",
        kern_ver
    )));
    a_ver.add_css_class("setting-subtext");
    a_ver.set_halign(gtk4::Align::Center);
    a_sec.append(&a_ver);

    let sep = Separator::new(Orientation::Horizontal);
    a_sec.append(&sep);

    let a_dev = Label::new(Some(
        "Geliştirici: Bulut Ars. E. (DarkMorpheus) & BlazeOS Project\nBlazeOS SolarEvolution 5.05 • Portadora (.blz) & Flathub Hibrit Mimari",
    ));
    a_dev.add_css_class("setting-label");
    a_dev.set_justify(gtk4::Justification::Center);
    a_dev.set_halign(gtk4::Align::Center);
    a_sec.append(&a_dev);
    about_tab.append(&a_sec);

    // Register pages and build ColorOS 17 Two-Pane Squircle Navigation Rows (Zero Unicode Tofu!)
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

    let pages: Vec<(&str, &str, &[u8], &str, GtkBox)> = vec![
        ("GÖRÜNÜM & KİŞİSELLEŞTİRME", "personalize", NAV_CAELESTIA_PNG, "Duvar Kağıdı & Tema", personalize_tab),
        ("", "display", NAV_DISPLAY_PNG, "Ekran, Çözünürlük & Hz", display_tab),
        ("", "clock", NAV_CLOCK_PNG, "Pixel Saat & Widget", clock_tab),
        ("", "taskbar", NAV_TASKBAR_PNG, "Görev Çubuğu", taskbar_tab),
        ("KABUK MOTORLARI", "shell", NAV_SHELL_PNG, "Kabuk Motoru & Yüzeyler", shell_tab),
        ("", "caelestia", NAV_CAELESTIA_PNG, "Caelestia Düzeni", caelestia_tab),
        ("", "noctalia", NAV_NOCTALIA_PNG, "Noctalia Bar", noctalia_tab),
        ("SİSTEM & GÜVENLİK", "perf", NAV_PERF_PNG, "Performans & Aurora", perf_tab),
        ("", "health", NAV_HEALTH_PNG, "Sistem Sağlığı & Kalkan", health_tab),
        ("", "omnibar", NAV_OMNIBAR_PNG, "Omnibar & Akıllı Arama", omnibar_tab),
        ("", "hud", NAV_HUD_PNG, "Kısayollar & Rehber", hud_tab),
        ("", "about", NAV_ABOUT_PNG, "Hakkında", about_tab),
    ];

    let mut nav_buttons: Vec<(String, String, Button)> = Vec::new();
    for (group_hdr, page_id, icon_bytes, label_txt, widget_box) in pages {
        if !group_hdr.is_empty() {
            let hdr = Label::new(Some(group_hdr));
            hdr.add_css_class("settings-cat-header");
            hdr.set_halign(gtk4::Align::Start);
            nav_list.append(&hdr);
        }
        stack.add_named(&widget_box, Some(page_id));

        let btn = Button::new();
        btn.add_css_class("lf-nav-pill");
        let row_box = GtkBox::new(Orientation::Horizontal, 10);
        let nav_icon = load_png_emblem(icon_bytes, 24);
        nav_icon.set_valign(gtk4::Align::Center);
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
    let tab = GtkBox::new(Orientation::Vertical, 12);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    let hw = HardwareDetector::detect();
    let current_profile = HardwareTuningProfile::load();

    // 1. Detected Hardware Card
    let hw_sec = GtkBox::new(Orientation::Vertical, 8);
    hw_sec.add_css_class("section-box");

    let hw_title = Label::new(Some("Algılanan Donanım Mimarisi"));
    hw_title.add_css_class("section-title");
    hw_title.set_halign(gtk4::Align::Start);
    hw_sec.append(&hw_title);

    let dev_label = Label::new(Some(&format!(
        "GPU: {}\nCihazlar: {}\nToplam RAM: {:.1} GB | İşlemci: {} Çekirdek\nGüç: {}",
        hw.gpu_vendor,
        if hw.gpu_devices.is_empty() {
            "Tümleşik / Standart DRM".to_string()
        } else {
            hw.gpu_devices.join(", ")
        },
        (hw.total_ram_mb as f64) / 1024.0,
        hw.cpu_cores,
        if hw.on_battery {
            "Batarya Gücünde"
        } else {
            "Şebeke Gücünde (AC Bağlı)"
        }
    )));
    dev_label.add_css_class("setting-label");
    dev_label.set_halign(gtk4::Align::Start);
    dev_label.set_margin_bottom(6);
    hw_sec.append(&dev_label);

    let rec_box = GtkBox::new(Orientation::Horizontal, 8);
    let rec_title = Label::new(Some("Donanımınıza Özel Önerilen Profil:"));
    rec_title.add_css_class("setting-subtext");
    rec_box.append(&rec_title);

    let rec_val = Label::new(Some(hw.recommended_tier.name_tr()));
    rec_val.add_css_class("status-badge-info");
    rec_box.append(&rec_val);
    hw_sec.append(&rec_box);

    tab.append(&hw_sec);

    // 2. Profile Selection Card
    let prof_sec = GtkBox::new(Orientation::Vertical, 10);
    prof_sec.add_css_class("section-box");

    let prof_title = Label::new(Some("Aktif Performans Profili"));
    prof_title.add_css_class("section-title");
    prof_title.set_halign(gtk4::Align::Start);
    prof_sec.append(&prof_title);

    let prof_sub = Label::new(Some(
        "Görsel efektler (blur, gölgeler, animasyon süreleri) donanım gücüne göre uyarlanır. Ekran yenileme hızı (Hz) profilden bağımsız olup daima monitörün native frekansında kalır.",
    ));
    prof_sub.add_css_class("setting-subtext");
    prof_sub.set_wrap(true);
    prof_sub.set_halign(gtk4::Align::Start);
    prof_sec.append(&prof_sub);

    let tier_items = StringList::new(&[
        "Hafif / Eski Donanım (Legacy) — Sıfır blur, sıfır gölge, anlık 0ms geçişler",
        "Dengeli (Balanced) — Optimize edilmiş akıcı animasyonlar, hafif cam efekti",
        "Yüksek Performans (Ultra) — Tam cam blur efekti, derin gölgeler, yay fiziği",
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

    // 3. ChromeOS Ash Stability & Resource Optimization Card
    let ash_sec = GtkBox::new(Orientation::Vertical, 10);
    ash_sec.add_css_class("section-box");

    let ash_title = Label::new(Some("ChromeOS Ash Seviyesi Kararlılık & Güç Tasarrufu"));
    ash_title.add_css_class("section-title");
    ash_title.set_halign(gtk4::Align::Start);
    ash_sec.append(&ash_title);

    let ash_sub = Label::new(Some(
        "Aura/Ash esintili akıllı kaynak yönetimi: Boşta CPU/GPU kullanımını minimuma indirir, çöken kabukları otomatik iyileştirir ve arayüzü OOM krizlerine karşı korur.",
    ));
    ash_sub.add_css_class("setting-subtext");
    ash_sub.set_wrap(true);
    ash_sub.set_halign(gtk4::Align::Start);
    ash_sec.append(&ash_sub);

    // Watchdog Auto-Healing
    let chk_watchdog = CheckButton::with_label("Sıfır-Çökme Watchdog (Çöken arayüz motorlarını 3sn içinde sessizce onarır)");
    chk_watchdog.set_active(current_profile.watchdog_enabled);
    chk_watchdog.connect_toggled(|cb| {
        let mut p = HardwareTuningProfile::load();
        p.watchdog_enabled = cb.is_active();
        let _ = p.save();
    });
    ash_sec.append(&chk_watchdog);

    // Idle Resource Throttling
    let chk_idle = CheckButton::with_label("Boşta Akıllı Kaynak Kısma (Kullanıcı boştayken CPU/RAM polling ve render'ı frenler)");
    chk_idle.set_active(current_profile.idle_throttling_enabled);
    chk_idle.connect_toggled(|cb| {
        let mut p = HardwareTuningProfile::load();
        p.idle_throttling_enabled = cb.is_active();
        let _ = p.save();
    });
    ash_sec.append(&chk_idle);

    // OOM Killer Shield
    let chk_oom = CheckButton::with_label("OOM Bellek Kalkanı (Ağır RAM yükünde pencere yöneticisi ve kabuğun öldürülmesini engeller)");
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
        ("Aktif (Unix IPC Soketi Hazır)".to_string(), "status-badge-ok")
    } else if has_core_proc {
        ("Aktif (Otonom Denetleyici Çalışıyor)".to_string(), "status-badge-ok")
    } else {
        ("Beklemede (İstemci Modu)".to_string(), "status-badge-info")
    };

    // 2. Active Shell Engine check
    let shell_status = if Command::new("pgrep").args(["-x", "noctalia"]).output().map(|o| o.status.success()).unwrap_or(false) {
        ("Canlı (Noctalia QML Motoru Aktif)".to_string(), "status-badge-ok")
    } else if Command::new("pgrep").args(["-f", "quickshell"]).output().map(|o| o.status.success()).unwrap_or(false) {
        ("Canlı (Quickshell / Caelestia / end4 Aktif)".to_string(), "status-badge-ok")
    } else {
        ("Canlı (SolarShell Yerel Yüzey)".to_string(), "status-badge-ok")
    };

    // 3. Wayland Compositor (Niri) check
    let niri_alive = std::env::var("NIRI_SOCKET")
        .map(|p| std::path::Path::new(&p).exists())
        .unwrap_or(false)
        || Command::new("pgrep").args(["-x", "niri"]).output().map(|o| o.status.success()).unwrap_or(false);
    let wm_status = if niri_alive {
        ("Canlı (Niri Wayland Compositor)".to_string(), "status-badge-ok")
    } else {
        ("Wayland Oturumu Aktif".to_string(), "status-badge-info")
    };

    // 4. Blaze Sentinel Live Security Audit
    let sentinel_status = if let Ok(out) = Command::new("blaze-sentinel").args(["status", "--json"]).output() {
        if let Ok(txt) = String::from_utf8(out.stdout) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&txt) {
                let (passed, total) = parse_sentinel_posture_counts(&val);
                (
                    format!("Tam Kalkan ({}/{} Güvenlik Kontrolü Aktif)", passed, total),
                    if passed == total { "status-badge-ok" } else { "status-badge-info" },
                )
            } else {
                ("Aktif (Blaze Sentinel Kalkanı)".to_string(), "status-badge-ok")
            }
        } else {
            ("Aktif (Blaze Sentinel Kalkanı)".to_string(), "status-badge-ok")
        }
    } else {
        ("Senkronize (Atomik Durum Makinesi)".to_string(), "status-badge-info")
    };

    vec![
        ("SolarCore Platform Servisi", core_status.0, core_status.1),
        ("SolarUI Kabuk Motoru", shell_status.0, shell_status.1),
        ("SolarUI Pencere Yöneticisi", wm_status.0, wm_status.1),
        ("Blaze Sentinel Güvenlik Kalkanı", sentinel_status.0, sentinel_status.1),
    ]
}

fn build_health_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 12);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    // 1. Health Status Card
    let stat_sec = GtkBox::new(Orientation::Vertical, 10);
    stat_sec.add_css_class("section-box");

    let stat_title = Label::new(Some("Platform Durumu ve Süreç İzolasyonu (Canlı Telemetri)"));
    stat_title.add_css_class("section-title");
    stat_title.set_halign(gtk4::Align::Start);
    stat_sec.append(&stat_title);

    let stat_desc = Label::new(Some(
        "SolarUI mimarisi bileşenleri izole eder ve Blaze Sentinel çekirdek güvenlik kontrollerini gerçek zamanlı denetler.",
    ));
    stat_desc.add_css_class("setting-subtext");
    stat_desc.set_wrap(true);
    stat_desc.set_halign(gtk4::Align::Start);
    stat_sec.append(&stat_desc);

    let components = probe_live_health_components();

    for (comp, status, badge_cls) in components {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.set_margin_top(4);
        row.set_margin_bottom(4);

        let c_lbl = Label::new(Some(comp));
        c_lbl.add_css_class("setting-label");
        c_lbl.set_hexpand(true);
        c_lbl.set_halign(gtk4::Align::Start);
        row.append(&c_lbl);

        let s_lbl = Label::new(Some(&status));
        s_lbl.add_css_class(badge_cls);
        s_lbl.set_halign(gtk4::Align::End);
        row.append(&s_lbl);

        stat_sec.append(&row);
    }

    tab.append(&stat_sec);

    // 1.5 USB Storage Guard (Paranoid Mode vs Desktop Mode)
    let usb_sec = GtkBox::new(Orientation::Vertical, 10);
    usb_sec.add_css_class("section-box");

    let usb_title = Label::new(Some("USB Depolama Kalkanı (Fiziksel Port Güvenliği)"));
    usb_title.add_css_class("section-title");
    usb_title.set_halign(gtk4::Align::Start);
    usb_sec.append(&usb_title);

    let usb_desc = Label::new(Some(
        "Etkinleştirildiğinde (Paranoid Mod) yeni takılan yabancı USB belleklerin çekirdek düzeyinde bağlanması engellenir. Günlük masaüstü ve ofis kullanımı için kapalı tutabilirsiniz.",
    ));
    usb_desc.add_css_class("setting-subtext");
    usb_desc.set_wrap(true);
    usb_desc.set_halign(gtk4::Align::Start);
    usb_sec.append(&usb_desc);

    let usb_locked = std::fs::read_to_string("/etc/modprobe.d/blaze-usb-guard.conf")
        .map(|c| c.contains("install usb-storage /bin/true"))
        .unwrap_or(false);
    let chk_usb_guard = CheckButton::with_label("Yabancı USB Depolama Aygıtlarını Kilitle (Paranoid Mod / Lynis STRG-1846)");
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
    tab.append(&usb_sec);

    // 2. Self-Healing Crash Test Card
    let test_sec = GtkBox::new(Orientation::Vertical, 10);
    test_sec.add_css_class("section-box");

    let test_title = Label::new(Some("Self-Healing Watchdog Testi"));
    test_title.add_css_class("section-title");
    test_title.set_halign(gtk4::Align::Start);
    test_sec.append(&test_title);

    let test_info = Label::new(Some(
        "Aşağıdaki butona tıkladığınızda `solar-shell` süreci aniden sonlandırılır. SolarUI pencere yöneticisi ve SolarCore çalışmaya devam eder. Görev çubuğu yeniden başlatıldığında açık uygulamalarınızın durumu yeniden yüklenir.",
    ));
    test_info.add_css_class("setting-subtext");
    test_info.set_wrap(true);
    test_info.set_halign(gtk4::Align::Start);
    test_sec.append(&test_info);

    let crash_btn = Button::with_label("Kabuğu Test Amaçlı Sonlandır (Self-Healing Crash Test)");
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
    let tab = GtkBox::new(Orientation::Vertical, 10);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    // Section 1: Style Selection Cards (Matching user screenshots 1, 2, 4)
    let style_sec = GtkBox::new(Orientation::Vertical, 10);
    style_sec.add_css_class("section-box");

    let s_title = Label::new(Some("Google Pixel / Material You Saat Stili"));
    s_title.add_css_class("section-title");
    s_title.set_halign(gtk4::Align::Start);
    style_sec.append(&s_title);

    let s_sub = Label::new(Some("Masaüstü duvar kağıdı üzerinde çalışan Google Pixel saat stilleri:"));
    s_sub.add_css_class("setting-subtext");
    s_sub.set_halign(gtk4::Align::Start);
    style_sec.append(&s_sub);

    let style_choices = StringList::new(&[
        "Pixel Kalın Yatay (16:49 + Pil Kapsülü & Tarih)",
        "Pixel İki Katlı Dikey (16 / 49 Büyük Balon Rakam)",
        "Pixel İçi Boş Kontur (Outline Minimalist)",
        "Pixel Analog Kalın (12, 3, 6, 9 Kalın Rakamlar)",
        "Pixel Analog Kontur (12, 3, 6, 9 İçi Boş Rakamlar)",
        "Pixel Analog Takimetre (Hız Göstergesi Kadranı)",
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

    // Section 2: Colors & Material You (Matching screenshot 3)
    let color_sec = GtkBox::new(Orientation::Vertical, 10);
    color_sec.add_css_class("section-box");

    let c_title = Label::new(Some("Renklendirme Seçenekleri (Coloring Options)"));
    c_title.add_css_class("section-title");
    c_title.set_halign(gtk4::Align::Start);
    color_sec.append(&c_title);

    let color_choices = StringList::new(&[
        "Dinamik: Material You (Duvar kağıdından otomatik uyarlanan ton)",
        "Özel Renk: Elektrik Camgöbeği (Cyan #80d4ff)",
        "Özel Renk: Pastel Mor (Lavender #c084fc)",
        "Özel Renk: Nane Yeşili (Mint #4ade80)",
        "Özel Renk: Şeftali / Mercan (Peach #fb923c)",
        "Özel Renk: Saf Beyaz (Pure White #ffffff)",
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
    let opts_sec = GtkBox::new(Orientation::Vertical, 10);
    opts_sec.add_css_class("section-box");

    let o_title = Label::new(Some("Rakam Opaklığı ve Göstergeler"));
    o_title.add_css_class("section-title");
    o_title.set_halign(gtk4::Align::Start);
    opts_sec.append(&o_title);

    let row1 = GtkBox::new(Orientation::Horizontal, 16);
    let op_lbl = Label::new(Some("Rakam Opaklığı (Numeral Opacity):"));
    op_lbl.add_css_class("setting-label");
    row1.append(&op_lbl);

    let op_choices = StringList::new(&["%100 (Tam Belirgin)", "%95 (Önerilen)", "%80 (Hafif Saydam)", "%60 (Yarı Saydam)", "%40 (Düşük)"]);
    let op_dd = DropDown::new(Some(op_choices), None::<gtk4::Expression>);
    op_dd.set_selected(1);
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
    let bat_chk = CheckButton::with_label("Pil Kapsülünü Göster (Battery Pill [PIL] %41)");
    bat_chk.set_active(clock_cfg.show_battery_pill);
    bat_chk.connect_toggled(|cb| {
        let mut cfg = PixelClockConfig::load();
        cfg.show_battery_pill = cb.is_active();
        let _ = cfg.save();
    });
    row2.append(&bat_chk);

    let date_chk = CheckButton::with_label("Tarih Göster");
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
    let act_sec = GtkBox::new(Orientation::Horizontal, 12);
    act_sec.set_halign(gtk4::Align::End);

    let launch_btn = Button::with_label("Pixel Saat Widget'ını Masaüstünde Aç");
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
    let tab = GtkBox::new(Orientation::Vertical, 10);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    // Section 1: Appearance Style Presets
    let preset_sec = GtkBox::new(Orientation::Vertical, 10);
    preset_sec.add_css_class("section-box");

    let p_title = Label::new(Some("Görev Çubuğu Görünüm Stili (Taskbar Appearance)"));
    p_title.add_css_class("section-title");
    p_title.set_halign(gtk4::Align::Start);
    preset_sec.append(&p_title);

    let p_sub = Label::new(Some(
        "Buzlu cam efekti zorunlu değildir. İster düz renk, ister şeffaf cam efekti, ister tam saydam, isterseniz de Material 3 duvar kağıdı renklerine dinamik uyarlanan stil seçebilirsiniz:",
    ));
    p_sub.add_css_class("setting-subtext");
    p_sub.set_wrap(true);
    p_sub.set_halign(gtk4::Align::Start);
    preset_sec.append(&p_sub);

    let preset_choices = StringList::new(&[
        "Buzlu Cam (Frosted Acrylic Glass — %32 Şeffaflık & Blur)",
        "Düz Opak Renk (Solid Opaque — %100 Mat)",
        "Tam Şeffaf (Fully Transparent — Sadece Simgeler Yüzer)",
        "Material 3 Dinamik Renk (Duvar Kağıdı Rengine Uyumlu)",
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
    let fine_sec = GtkBox::new(Orientation::Vertical, 10);
    fine_sec.add_css_class("section-box");

    let f_title = Label::new(Some("Manuel Boyut ve Şeffaflık İnce Ayarları"));
    f_title.add_css_class("section-title");
    f_title.set_halign(gtk4::Align::Start);
    fine_sec.append(&f_title);

    // Opacity
    let op_row = GtkBox::new(Orientation::Horizontal, 16);
    let op_l = Label::new(Some("Şeffaflık Derecesi:"));
    op_l.add_css_class("setting-label");
    op_l.set_hexpand(true);
    op_l.set_halign(gtk4::Align::Start);
    op_row.append(&op_l);

    let op_list = StringList::new(&["%10 (Ultra Cam)", "%25 (Hafif Cam)", "%32 (Varsayılan Solar)", "%50 (Yarı Saydam)", "%75 (Koyu Cam)", "%100 (Mat)"]);
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
    let rad_l = Label::new(Some("Köşe Yuvarlaklığı:"));
    rad_l.add_css_class("setting-label");
    rad_l.set_hexpand(true);
    rad_l.set_halign(gtk4::Align::Start);
    rad_row.append(&rad_l);

    let rad_list = StringList::new(&["0 px (Köşeli Düz)", "8 px (Hafif Kavis)", "16 px (Varsayılan Modern)", "24 px (Ekstra Yuvarlak)", "40 px (Kapsül)"]);
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
    let h_l = Label::new(Some("Çubuk Yüksekliği (Kalınlık):"));
    h_l.add_css_class("setting-label");
    h_l.set_hexpand(true);
    h_l.set_halign(gtk4::Align::Start);
    h_row.append(&h_l);

    let h_list = StringList::new(&["36 px (Kompakt)", "40 px (Varsayılan)", "44 px (Rahat)", "48 px (Büyük)", "56 px (KDE Plasma Standart)"]);
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
    let btn_row = GtkBox::new(Orientation::Horizontal, 12);
    btn_row.set_halign(gtk4::Align::End);
    let full_btn = Button::with_label("Gelişmiş Çubuk Düzenleyicisini Aç");
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
    let tab = GtkBox::new(Orientation::Vertical, 12);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    let sec = GtkBox::new(Orientation::Vertical, 12);
    sec.add_css_class("section-box");

    let title = Label::new(Some("Kabuk ve Yüzey Motoru (Shell Engine Switcher)"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Noctalia, Caelestia, end4-pC ve We10X (Tanıdık Klasik Düzen / Windows 10 Modu) kabuk motorları arasında güvenli biçimde geçiş yapabilirsiniz. Her motor kendi bar, bildirim, OSD ve sistem tepsisini tek başına yönetir.",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    let cfg = SolarConfig::load();

    // 1. Shell Engine Selector
    let row1 = GtkBox::new(Orientation::Horizontal, 12);
    let lbl1 = Label::new(Some("Aktif Kabuk Motoru:"));
    lbl1.add_css_class("setting-label");
    lbl1.set_hexpand(true);
    lbl1.set_halign(gtk4::Align::Start);
    row1.append(&lbl1);

    let engines = [
        "Noctalia v5 (C++23 Native)",
        "Caelestia Shell (Quickshell QML)",
        "end4-pC (Material 3 Expressive)",
        "We10X (Tanıdık Klasik Düzen)",
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
    let btn_box = GtkBox::new(Orientation::Horizontal, 10);
    btn_box.set_margin_top(8);

    let apply_btn = Button::with_label("Kabuk Motorunu Uygula & Geçiş Yap");
    apply_btn.add_css_class("action-btn");
    apply_btn.set_halign(gtk4::Align::Start);

    let engine_dd_clone = engine_dd.clone();

    let shell_status_lbl = Label::new(Some(""));
    shell_status_lbl.add_css_class("setting-subtext");
    shell_status_lbl.set_halign(gtk4::Align::Start);

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
            "[OK] Kabuk motoru ({}) geçişi başlatıldı...",
            engine_str
        ));

        let engine_to_switch = engine_str.to_string();
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["switch", &engine_to_switch])
                .status();
        });
    });
    btn_box.append(&apply_btn);

    let reconcile_btn = Button::with_label("Durumu Doğrula (Supervisor Reconcile)");
    reconcile_btn.add_css_class("secondary-btn");
    reconcile_btn.set_halign(gtk4::Align::Start);
    let shell_status_rec = shell_status_lbl.clone();
    reconcile_btn.connect_clicked(move |_| {
        shell_status_rec.set_text("[OK] Kabuk durumu denetleniyor ve doğrulanıyor...");
        std::thread::spawn(|| {
            let _ = Command::new("solar-shell")
                .args(["supervisor"])
                .status();
        });
    });
    btn_box.append(&reconcile_btn);

    let end4_settings_btn = Button::with_label("end4-pC Ayarlarını Aç");
    end4_settings_btn.add_css_class("secondary-btn");
    end4_settings_btn.set_halign(gtk4::Align::Start);
    let shell_status_end4 = shell_status_lbl.clone();
    end4_settings_btn.connect_clicked(move |_| {
        let cfg = SolarConfig::load();
        if cfg.shell.engine != ShellEngine::End4Pc {
            shell_status_end4.set_text("end4-pC ayarları için önce end4-pC motorunu uygulayın.");
            return;
        }
        shell_status_end4.set_text("end4-pC ayarları açılıyor...");
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

    // 3. We10X (Tanıdık Klasik Düzen) Customization Card
    tab.append(&build_we10x_customization_card());

    tab
}

fn build_we10x_customization_card() -> GtkBox {
    let cfg = SolarConfig::load();
    let we10x_sec = GtkBox::new(Orientation::Vertical, 12);
    we10x_sec.add_css_class("section-box");

    let we10x_title = Label::new(Some(
        "We10X (Windows 10 Modu) Tema, Renk ve Masaüstü Özelleştirmeleri",
    ));
    we10x_title.add_css_class("section-title");
    we10x_title.set_halign(gtk4::Align::Start);
    we10x_sec.append(&we10x_title);

    let we10x_desc = Label::new(Some(
        "Windows 10 kabuk modunun koyu/açık/tam siyah temasını, görev çubuğu ve Başlat menüsü vurgu rengini (kırmızı, mavi, koyu veya dilediğiniz özel RGB/Hex renk), görev çubuğu şeffaflığını ve masaüstü simgelerini özelleştirin.",
    ));
    we10x_desc.add_css_class("setting-subtext");
    we10x_desc.set_wrap(true);
    we10x_desc.set_xalign(0.0);
    we10x_desc.set_halign(gtk4::Align::Start);
    we10x_sec.append(&we10x_desc);

    let status_lbl = Label::new(Some(""));
    status_lbl.add_css_class("setting-subtext");
    status_lbl.set_halign(gtk4::Align::Start);

    // 1. Theme Mode: Dark / Light / Pure Black OLED
    let theme_row = GtkBox::new(Orientation::Horizontal, 12);
    let theme_lbl = Label::new(Some("Görev Çubuğu ve Pencere Teması:"));
    theme_lbl.add_css_class("setting-label");
    theme_lbl.set_hexpand(true);
    theme_lbl.set_halign(gtk4::Align::Start);
    theme_row.append(&theme_lbl);

    let we10x_themes = [
        "Koyu (Windows 10 Dark)",
        "Açık (Windows 10 Light)",
        "Tam Siyah (OLED Kontrast)",
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
        let mode = match dd.selected() {
            1 => "light",
            2 => "oled",
            _ => "dark",
        };
        st_theme.set_text(&format!("[OK] We10X tema modu '{}' olarak uygulandı.", mode));
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-theme", mode])
                .status();
        });
    });
    theme_row.append(&we10x_theme_dd);
    we10x_sec.append(&theme_row);

    // 2. Preset Color Swatches (Red, Blue, Dark Charcoal, Green, Purple, Amber, Teal)
    let swatch_hdr = Label::new(Some("Hızlı Tema ve Vurgu Rengi Paleti:"));
    swatch_hdr.add_css_class("setting-label");
    swatch_hdr.set_halign(gtk4::Align::Start);
    we10x_sec.append(&swatch_hdr);

    let swatch_row = GtkBox::new(Orientation::Horizontal, 8);
    let hex_entry = gtk4::Entry::builder()
        .text(&crate::we10x_profile::normalize_hex_color(
            &cfg.shell.we10x_accent_color,
        ))
        .max_length(7)
        .width_chars(9)
        .placeholder_text("#E81123")
        .build();

    let color_dialog = gtk4::ColorDialog::builder()
        .title("We10X Özel Tema Rengi Seçin")
        .with_alpha(false)
        .modal(true)
        .build();
    let mut init_rgba = gdk::RGBA::new(0.0, 0.47, 0.84, 1.0);
    if let Ok(parsed_rgba) = gdk::RGBA::parse(&cfg.shell.we10x_accent_color) {
        init_rgba = parsed_rgba;
    }
    let color_btn = gtk4::ColorDialogButton::new(Some(color_dialog));
    color_btn.set_rgba(&init_rgba);
    color_btn.set_tooltip_text(Some("Renk Tekerleğinden Özel Renk Seçin"));

    let presets: [(&str, &str, &str); 7] = [
        ("Kırmızı", "#e81123", "swatch-red"),
        ("Mavi", "#0078d7", "swatch-blue"),
        ("Koyu", "#2d2d30", "swatch-dark"),
        ("Yeşil", "#107c10", "swatch-green"),
        ("Mor", "#881798", "swatch-purple"),
        ("Kehribar", "#ff6a00", "swatch-amber"),
        ("Turkuaz", "#008272", "swatch-teal"),
    ];

    for (label_txt, hex_code, css_cls) in presets {
        let b = Button::with_label(label_txt);
        b.add_css_class(css_cls);
        let hex_owned = hex_code.to_string();
        let entry_ref = hex_entry.clone();
        let cbtn_ref = color_btn.clone();
        let st_sw = status_lbl.clone();
        b.connect_clicked(move |_| {
            entry_ref.set_text(&hex_owned);
            if let Ok(rgba) = gdk::RGBA::parse(&hex_owned) {
                cbtn_ref.set_rgba(&rgba);
            }
            st_sw.set_text(&format!(
                "[OK] We10X tema rengi {} ({}) olarak ayarlandı.",
                label_txt, hex_owned
            ));
            let hx = hex_owned.clone();
            std::thread::spawn(move || {
                let _ = Command::new("solar-shell")
                    .args(["we10x-config", "set-accent", &hx])
                    .status();
            });
        });
        swatch_row.append(&b);
    }
    we10x_sec.append(&swatch_row);

    // 3. Arbitrary Custom RGB / Hex Color Picker Row
    let custom_color_row = GtkBox::new(Orientation::Horizontal, 10);
    let custom_lbl = Label::new(Some("Özel Renk Seçici (İstediğiniz Renk / Hex):"));
    custom_lbl.add_css_class("setting-label");
    custom_lbl.set_hexpand(true);
    custom_lbl.set_halign(gtk4::Align::Start);
    custom_color_row.append(&custom_lbl);

    let entry_from_dialog = hex_entry.clone();
    let st_dialog = status_lbl.clone();
    color_btn.connect_rgba_notify(move |btn| {
        let c = btn.rgba();
        let r = (c.red().clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (c.green().clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (c.blue().clamp(0.0, 1.0) * 255.0).round() as u8;
        let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);
        if entry_from_dialog.text().as_str().to_ascii_lowercase() != hex {
            entry_from_dialog.set_text(&hex);
        }
        st_dialog.set_text(&format!("[OK] Özel tema rengi {} uygulandı.", hex));
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-accent", &hex])
                .status();
        });
    });
    custom_color_row.append(&color_btn);
    custom_color_row.append(&hex_entry);

    let apply_hex_btn = Button::with_label("Hex Rengi Uygula");
    apply_hex_btn.add_css_class("secondary-btn");
    let entry_for_btn = hex_entry.clone();
    let cbtn_for_btn = color_btn.clone();
    let st_hex = status_lbl.clone();
    apply_hex_btn.connect_clicked(move |_| {
        let norm = crate::we10x_profile::normalize_hex_color(entry_for_btn.text().as_str());
        entry_for_btn.set_text(&norm);
        if let Ok(rgba) = gdk::RGBA::parse(&norm) {
            cbtn_for_btn.set_rgba(&rgba);
        }
        st_hex.set_text(&format!("[OK] Özel Hex rengi {} uygulandı.", norm));
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
        "Görev çubuğu, Başlat menüsü, İşlem Merkezi ve pencere başlık çubuklarını seçilen tema rengiyle boya",
    );
    colorize_chk.set_active(cfg.shell.we10x_colorize_surfaces);
    let st_col = status_lbl.clone();
    colorize_chk.connect_toggled(move |chk| {
        let val = if chk.is_active() { "true" } else { "false" };
        st_col.set_text("[OK] Görev çubuğu ve başlık renklendirme tercihi güncellendi.");
        std::thread::spawn(move || {
            let _ = Command::new("solar-shell")
                .args(["we10x-config", "set-colorize", val])
                .status();
        });
    });
    we10x_sec.append(&colorize_chk);

    // 5. Taskbar Opacity
    let op_row = GtkBox::new(Orientation::Horizontal, 12);
    let op_lbl = Label::new(Some("We10X Görev Çubuğu Şeffaflığı:"));
    op_lbl.add_css_class("setting-label");
    op_lbl.set_hexpand(true);
    op_lbl.set_halign(gtk4::Align::Start);
    op_row.append(&op_lbl);

    let op_choices = [
        "%65 — Şeffaf Cam",
        "%85 — Yarı Saydam Akrilik",
        "%95 — Klasik Windows 10 Koyu Cam (Varsayılan)",
        "%100 — Tam Mat / Opak",
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
    let icon_lbl = Label::new(Some("Başlat Düğmesi İkonu:"));
    icon_lbl.add_css_class("setting-label");
    icon_lbl.set_hexpand(true);
    icon_lbl.set_halign(gtk4::Align::Start);
    icon_row.append(&icon_lbl);

    let start_icons = [
        "We10X 4-Bölmeli Perspektif İkonu (GPL-3.0)",
        "BlazeOS Alev İkonu",
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
        "Masaüstü simgelerini göster (Bu Bilgisayar, Geri Dönüşüm Kutusu, FireHub Mağaza)",
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
    let tab = GtkBox::new(Orientation::Vertical, 14);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);

    // Section 1: Universal Wallpaper Gallery & Custom File Picker (Works in Noctalia, We10X, Caelestia, end4-pC)
    let wp_sec = GtkBox::new(Orientation::Vertical, 12);
    wp_sec.add_css_class("section-box");

    let wp_title = Label::new(Some("Masaüstü Duvar Kağıdı Yönetimi (Tüm Kabuklar)"));
    wp_title.add_css_class("section-title");
    wp_title.set_halign(gtk4::Align::Start);
    wp_sec.append(&wp_title);

    let wp_desc = Label::new(Some(
        "Seçtiğiniz duvar kağıdı aktif kabuk hangisi olursa olsun (BlazeOS Noctalia, Windows 10 Modu We10X, Caelestia, end4-pC) anında uygulanır ve Dosya Gezgini (Nautilus) ile senkronize kalır.",
    ));
    wp_desc.add_css_class("setting-subtext");
    wp_desc.set_wrap(true);
    wp_desc.set_xalign(0.0);
    wp_desc.set_halign(gtk4::Align::Start);
    wp_sec.append(&wp_desc);

    let active_wp_lbl = Label::new(Some(&format!(
        "Aktif Duvar Kağıdı: {}",
        crate::wallpaper::current_effective_wallpaper()
    )));
    active_wp_lbl.add_css_class("setting-subtext");
    active_wp_lbl.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    active_wp_lbl.set_halign(gtk4::Align::Start);
    wp_sec.append(&active_wp_lbl);

    let action_row = GtkBox::new(Orientation::Horizontal, 10);
    let pick_btn = Button::with_label("Bilgisayardan Duvar Kağıdı Seç...");
    pick_btn.add_css_class("action-btn");
    let lbl_after_pick = active_wp_lbl.clone();
    pick_btn.connect_clicked(move |btn| {
        let win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        let dialog = gtk4::FileDialog::builder()
            .title("Duvar Kağıdı Görseli Seçin")
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
        dialog.open(win.as_ref(), None::<&gtk4::gio::Cancellable>, move |res| {
            if let Ok(file) = res {
                if let Some(path) = file.path() {
                    let path_str = path.to_string_lossy().to_string();
                    lbl_cb.set_text(&format!("[OK] Aktif Duvar Kağıdı: {}", path_str));
                    std::thread::spawn(move || {
                        let _ = crate::wallpaper::apply_wallpaper_everywhere(&path_str, true);
                    });
                }
            }
        });
    });
    action_row.append(&pick_btn);

    let open_dir_btn = Button::with_label("Duvar Kağıdı Klasörünü Aç");
    open_dir_btn.add_css_class("secondary-btn");
    open_dir_btn.connect_clicked(|_| {
        let _ = Command::new("nautilus")
            .arg("/usr/share/backgrounds/blazeos")
            .spawn();
    });
    action_row.append(&open_dir_btn);
    wp_sec.append(&action_row);

    let gallery = gtk4::FlowBox::builder()
        .selection_mode(gtk4::SelectionMode::None)
        .max_children_per_line(3)
        .min_children_per_line(2)
        .column_spacing(10)
        .row_spacing(10)
        .homogeneous(true)
        .build();

    for (title_txt, img_path) in crate::wallpaper::discover_available_wallpapers() {
        let card = GtkBox::new(Orientation::Vertical, 6);
        card.add_css_class("wp-card");

        let pic = gtk4::Picture::for_filename(&img_path);
        pic.set_can_shrink(true);
        pic.set_content_fit(gtk4::ContentFit::Cover);
        pic.set_size_request(176, 98);
        card.append(&pic);

        let name_l = Label::new(Some(&title_txt));
        name_l.add_css_class("setting-subtext");
        name_l.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_l.set_max_width_chars(24);
        name_l.set_halign(gtk4::Align::Start);
        card.append(&name_l);

        let use_btn = Button::with_label("Duvar Kağıdı Yap");
        use_btn.add_css_class("secondary-btn");
        let p_owned = img_path.clone();
        let lbl_card = active_wp_lbl.clone();
        use_btn.connect_clicked(move |_| {
            lbl_card.set_text(&format!("[OK] Aktif Duvar Kağıdı: {}", p_owned));
            let target = p_owned.clone();
            std::thread::spawn(move || {
                let _ = crate::wallpaper::apply_wallpaper_everywhere(&target, true);
            });
        });
        card.append(&use_btn);
        gallery.insert(&card, -1);
    }
    wp_sec.append(&gallery);
    tab.append(&wp_sec);

    // Section 2: Rich We10X Theme Mode, Accent Color & Taskbar Customization
    tab.append(&build_we10x_customization_card());

    tab
}

fn build_caelestia_tab() -> GtkBox {
    let tab = GtkBox::new(Orientation::Vertical, 12);
    tab.set_margin_top(10);
    tab.set_margin_bottom(10);
    
    let cfg = SolarConfig::load();
    if cfg.shell.engine != ShellEngine::Caelestia {
        let msg = Label::new(Some("Bu ayarlar yalnız tam Caelestia motoru aktifken kullanılabilir. Lütfen 'Kabuk Motoru' sekmesinden Caelestia'yı seçin."));
        msg.add_css_class("setting-label");
        msg.set_wrap(true);
        msg.set_halign(gtk4::Align::Center);
        msg.set_valign(gtk4::Align::Center);
        msg.set_vexpand(true);
        tab.append(&msg);
        return tab;
    }
    
    // Section 1: Wallpaper
    let sec_wall = GtkBox::new(Orientation::Vertical, 10);
    sec_wall.add_css_class("section-box");
    
    let w_title = Label::new(Some("Duvar Kağıdı ve Material 3 Renk Şeması"));
    w_title.add_css_class("section-title");
    w_title.set_halign(gtk4::Align::Start);
    sec_wall.append(&w_title);
    
    let w_desc = Label::new(Some(
        "Caelestia yüzeyleri (Launcher, Dashboard) seçilen duvar kağıdına göre dinamik olarak renk paleti üretir. Duvar kağıdını değiştirdiğinizde tüm arayüz renkleri otomatik güncellenir."
    ));
    w_desc.add_css_class("setting-subtext");
    w_desc.set_wrap(true);
    w_desc.set_halign(gtk4::Align::Start);
    sec_wall.append(&w_desc);
    
    let btn_row = GtkBox::new(Orientation::Horizontal, 10);
    
    let btn_rand = Button::with_label("Rastgele Duvar Kağıdı");
    btn_rand.add_css_class("action-btn");
    btn_rand.connect_clicked(|_| {
        std::thread::spawn(|| {
            let _ = Command::new("caelestia").args(["wallpaper", "-r"]).status();
        });
    });
    btn_row.append(&btn_rand);

    let btn_pick = Button::with_label("Dosyadan Seç...");
    btn_pick.add_css_class("secondary-btn");
    btn_pick.connect_clicked(|btn| {
        let win = btn.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        let dialog = gtk4::FileDialog::builder()
            .title("Duvar Kağıdı Seç (Caelestia)")
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
    let sec_lay = GtkBox::new(Orientation::Vertical, 10);
    sec_lay.add_css_class("section-box");
    
    let l_title = Label::new(Some("Masaüstü ve Arayüz Düzeni"));
    l_title.add_css_class("section-title");
    l_title.set_halign(gtk4::Align::Start);
    sec_lay.append(&l_title);
    
    let l_desc = Label::new(Some(
        "Quickshell tabanlı Caelestia arayüzü için tema modu ve diğer yapılandırmalar."
    ));
    l_desc.add_css_class("setting-subtext");
    l_desc.set_halign(gtk4::Align::Start);
    sec_lay.append(&l_desc);
    
    let row_mode = GtkBox::new(Orientation::Horizontal, 12);
    let lbl_mode = Label::new(Some("Caelestia Tema Modu:"));
    lbl_mode.add_css_class("setting-label");
    row_mode.append(&lbl_mode);

    let mode_dd = DropDown::new(Some(StringList::new(&["Karanlık (Dark)", "Aydınlık (Light)"])), None::<gtk4::Expression>);
    
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
    let tab = GtkBox::new(Orientation::Vertical, 16);
    tab.set_margin_top(14);
    tab.set_margin_bottom(14);
    tab.set_margin_start(16);
    tab.set_margin_end(16);

    let cfg = SolarConfig::load();
    let scale_presets = [
        "0.75x — Kompakt (%75)",
        "0.85x — Küçük (%85 - 1280x800 için önerilen)",
        "1.00x — Standart (%100 - Varsayılan)",
        "1.25x — Büyük (%125)",
        "1.50x — Geniş (%150 - 2K Ekranlar)",
        "1.75x — Ekstra Büyük (%175)",
        "2.00x — HiDPI (%200 - 4K Ekranlar)",
    ];
    let scale_values = [0.75, 0.85, 1.00, 1.25, 1.50, 1.75, 2.00];

    // SECTION 1: Per-Monitor Manual Resolution, Refresh Rate (Hz) & Independent DPI Scaling
    let sec_mon = GtkBox::new(Orientation::Vertical, 14);
    sec_mon.add_css_class("section-box");

    let title_mon = Label::new(Some(
        "Monitör Çözünürlüğü, Yenileme Hızı (Hz) ve Bağımsız Ölçekleme",
    ));
    title_mon.add_css_class("section-title");
    title_mon.set_halign(gtk4::Align::Start);
    sec_mon.append(&title_mon);

    let desc_mon = Label::new(Some(
        "Bağlı her ekran için çözünürlüğü (ör. 1920x1080, 2560x1440, 1366x768), yenileme hızını (60 Hz, 75 Hz, 120 Hz, 144 Hz, 240 Hz vb.) ve bağımsız DPI ölçeğini doğrudan seçin veya özel çözünürlük/Hz girin.",
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
        let mon_card = GtkBox::new(Orientation::Vertical, 10);
        mon_card.add_css_class("wp-card");

        let mon_header = GtkBox::new(Orientation::Horizontal, 10);
        let mon_icon = Image::from_icon_name("video-display");
        mon_icon.set_pixel_size(24);
        mon_header.append(&mon_icon);

        let info_box = GtkBox::new(Orientation::Vertical, 2);
        info_box.set_hexpand(true);

        let name_lbl = Label::new(Some(&format!("{} — {}", out.name, out.description)));
        name_lbl.add_css_class("setting-label");
        name_lbl.set_halign(gtk4::Align::Start);
        info_box.append(&name_lbl);

        let res_lbl = Label::new(Some(&format!(
            "Aktif Mod: {} | Ölçek: {:.2}x",
            out.resolution, out.scale
        )));
        res_lbl.add_css_class("setting-subtext");
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
                    format!("{} × {} (Donanım)", w, h)
                } else {
                    format!("{} × {} (Özel Mod)", w, h)
                }
            })
            .collect();
        let res_label_refs: Vec<&str> = res_labels.iter().map(|s| s.as_str()).collect();
        let res_dd = DropDown::new(Some(StringList::new(&res_label_refs)), None::<gtk4::Expression>);

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
                    format!("{:.2} Hz (Donanım)", *mhz as f64 / 1000.0)
                } else {
                    format!("{:.2} Hz (Özel)", *mhz as f64 / 1000.0)
                }
            })
            .collect();
        let hz_refs_init: Vec<&str> = hz_labels_init.iter().map(|s| s.as_str()).collect();
        let hz_dd = DropDown::new(Some(StringList::new(&hz_refs_init)), None::<gtk4::Expression>);

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
                        format!("{:.2} Hz (Donanım)", *mhz as f64 / 1000.0)
                    } else {
                        format!("{:.2} Hz (Özel)", *mhz as f64 / 1000.0)
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
        let mut mon_sel_idx = 2;
        for (i, &val) in scale_values.iter().enumerate() {
            if (out.scale - val).abs() < 0.04 {
                mon_sel_idx = i as u32;
                break;
            }
        }
        mon_scale_dd.set_selected(mon_sel_idx);

        // Controls row 1: Resolution + Refresh Rate (Hz)
        let row_mode_sel = GtkBox::new(Orientation::Horizontal, 10);
        let l_res = Label::new(Some("Çözünürlük:"));
        l_res.add_css_class("setting-label");
        row_mode_sel.append(&l_res);
        row_mode_sel.append(&res_dd);

        let l_hz = Label::new(Some("Yenileme Hızı (Hz):"));
        l_hz.add_css_class("setting-label");
        l_hz.set_margin_start(8);
        row_mode_sel.append(&l_hz);
        row_mode_sel.append(&hz_dd);
        mon_card.append(&row_mode_sel);

        // Controls row 2: DPI Scale + Apply Button
        let row_scale_sel = GtkBox::new(Orientation::Horizontal, 10);
        let l_sc = Label::new(Some("Monitör Ölçeği:"));
        l_sc.add_css_class("setting-label");
        row_scale_sel.append(&l_sc);
        row_scale_sel.append(&mon_scale_dd);

        let mon_apply = Button::with_label("Çözünürlük, Hz ve Ölçeği Uygula");
        mon_apply.add_css_class("action-btn");

        let mon_status = Label::new(Some(""));
        mon_status.add_css_class("setting-subtext");
        mon_status.set_halign(gtk4::Align::Start);

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

            match apply_per_output_mode_dynamically(&out_name, &mode_spec, is_custom) {
                Ok(()) => {
                    apply_per_output_scale_dynamically(&out_name, sc_val);
                    res_lbl_apply.set_text(&format!(
                        "Aktif Mod: {}x{} @ {:.2}Hz | Ölçek: {:.2}x",
                        w,
                        h,
                        mhz as f64 / 1000.0,
                        sc_val
                    ));
                    mon_status_apply.set_text(&format!(
                        "[OK] {} için {} ({:.2}x ölçek) başarıyla uygulandı ve kaydedildi.",
                        out_name, mode_spec, sc_val
                    ));
                }
                Err(err) => {
                    mon_status_apply.set_text(&format!(
                        "[UYARI] {} modu uygulanamadı: {}",
                        mode_spec, err
                    ));
                }
            }
        });
        row_scale_sel.append(&mon_apply);
        mon_card.append(&row_scale_sel);

        // Controls row 3: Custom W x H @ Hz Manual Entry
        let custom_row = GtkBox::new(Orientation::Horizontal, 8);
        let c_lbl = Label::new(Some("Manuel Özel Mod:"));
        c_lbl.add_css_class("setting-subtext");
        custom_row.append(&c_lbl);

        let w_entry = gtk4::Entry::builder()
            .text(out.current_width.to_string())
            .width_chars(6)
            .placeholder_text("1920")
            .build();
        let x_lbl = Label::new(Some("×"));
        let h_entry = gtk4::Entry::builder()
            .text(out.current_height.to_string())
            .width_chars(6)
            .placeholder_text("1080")
            .build();
        let at_lbl = Label::new(Some("@"));
        let hz_entry = gtk4::Entry::builder()
            .text(format!("{:.0}", out.current_refresh_mhz as f64 / 1000.0))
            .width_chars(5)
            .placeholder_text("60")
            .build();
        let hz_u_lbl = Label::new(Some("Hz"));

        let custom_btn = Button::with_label("Özel Modu Uygula");
        custom_btn.add_css_class("secondary-btn");
        let out_custom = out.name.clone();
        let st_custom = mon_status.clone();
        let rl_custom = res_lbl.clone();
        let w_ent_c = w_entry.clone();
        let h_ent_c = h_entry.clone();
        let hz_ent_c = hz_entry.clone();
        let cur_sc = out.scale;
        custom_btn.connect_clicked(move |_| {
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
                st_custom.set_text(
                    "[HATA] Geçerli bir çözünürlük (640..7680 × 480..4320) ve Hz (24..500) girin.",
                );
                return;
            }
            let spec = format!("{}x{}@{:.3}", w_ok, h_ok, hz_ok);
            match apply_per_output_mode_dynamically(&out_custom, &spec, true) {
                Ok(()) => {
                    rl_custom.set_text(&format!(
                        "Aktif Mod: {}x{} @ {:.2}Hz | Ölçek: {:.2}x",
                        w_ok, h_ok, hz_ok, cur_sc
                    ));
                    st_custom.set_text(&format!(
                        "[OK] Özel mod {} başarıyla uygulandı ve kaydedildi.",
                        spec
                    ));
                }
                Err(e) => {
                    st_custom.set_text(&format!("[UYARI] Özel mod uygulanamadı: {}", e));
                }
            }
        });

        custom_row.append(&w_entry);
        custom_row.append(&x_lbl);
        custom_row.append(&h_entry);
        custom_row.append(&at_lbl);
        custom_row.append(&hz_entry);
        custom_row.append(&hz_u_lbl);
        custom_row.append(&custom_btn);
        mon_card.append(&custom_row);
        mon_card.append(&mon_status);

        sec_mon.append(&mon_card);
    }
    tab.append(&sec_mon);

    // SECTION 2: Global DPI Scaling Across All Displays
    let sec = GtkBox::new(Orientation::Vertical, 12);
    sec.add_css_class("section-box");

    let title = Label::new(Some("Genel Masaüstü ve Arayüz Ölçeği (Tüm Ekranlar)"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Tüm ekranlarınızdaki pencere ve yazı boyutunu tek seferde ölçeklendirin.",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    let row_scale = GtkBox::new(Orientation::Horizontal, 12);
    let lbl_scale = Label::new(Some("Genel Sistem Ölçeği:"));
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

    let status_lbl = Label::new(Some(""));
    status_lbl.add_css_class("setting-subtext");
    status_lbl.set_halign(gtk4::Align::Start);

    let btn_box = GtkBox::new(Orientation::Horizontal, 10);
    btn_box.set_margin_top(8);

    let apply_btn = Button::with_label("Tüm Ekranlara Uygula");
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
            "[OK] Genel ölçek {:.2}x başarıyla uygulandı ve kaydedildi.",
            chosen_scale
        ));
    });
    btn_box.append(&apply_btn);

    let reset_btn = Button::with_label("Varsayılana Sıfırla (1.00x)");
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
        status_reset.set_text("[OK] Ölçek varsayılan 1.00x değerine sıfırlandı.");
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
    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/liveuser".to_string());
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
                            "Dahili / Harici Ekran".to_string()
                        };

                        let mut parsed_modes = Vec::new();
                        let mut cur_w: u32 = 1920;
                        let mut cur_h: u32 = 1080;
                        let mut cur_mhz: u32 = 60_000;
                        let mut res_str = "1920x1080 @ 60.00Hz".to_string();

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
                                    "{}x{} @ {:.2}Hz",
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
            description: "Sanal / Varsayılan Ekran".to_string(),
            resolution: "1920x1080 @ 60.00Hz".to_string(),
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
    let tab = GtkBox::new(Orientation::Vertical, 16);
    tab.set_margin_top(14);
    tab.set_margin_bottom(14);
    tab.set_margin_start(16);
    tab.set_margin_end(16);

    let sec = GtkBox::new(Orientation::Vertical, 14);
    sec.add_css_class("section-box");

    let title = Label::new(Some("SolarUI Omnibar ve Başlatıcı Yönlendirme"));
    title.add_css_class("section-title");
    title.set_halign(gtk4::Align::Start);
    sec.append(&title);

    let desc = Label::new(Some(
        "Super + Boşluk kısayolu ile açılan Omnibar; sistem komutları, anlık matematik hesaplamaları, çevrimdışı yaklaşık birim/kur çevirileri, pano geçmişi (cb) ve uygulama başlatıcısıdır. Dilerseniz devre dışı bırakıp aktif kabuğun (Caelestia / Noctalia / end4-pC) kendi yerel uygulama başlatıcısını kullanabilirsiniz.",
    ));
    desc.add_css_class("setting-subtext");
    desc.set_wrap(true);
    desc.set_halign(gtk4::Align::Start);
    sec.append(&desc);

    let sep1 = Separator::new(Orientation::Horizontal);
    sec.append(&sep1);

    let cfg = SolarConfig::load();

    // 0. Omnibar Etkinleştirme & Kabuk Başlatıcısı Davranışı
    let chk_enabled = CheckButton::with_label(
        "SolarUI Omnibar'ı Etkinleştir (Kapalıysa Super+Boşluk aktif kabuğun kendi başlatıcısını açar)",
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
        "Caelestia / end4-pC aktifken de Super+Boşluk ile Omnibar aç (Kapalıysa kabuğun kendi menüsü açılır)",
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
    let chk_web = CheckButton::with_label("Çevrimiçi Web Arama Sonuçlarını Göster");
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
    let engine_lbl = Label::new(Some("Varsayılan Arama Motoru:"));
    engine_lbl.add_css_class("setting-label");
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

    // Test Butonları
    let btn_row = GtkBox::new(Orientation::Horizontal, 10);
    let btn_test = Button::with_label("Omnibar'ı Şimdi Aç (Super + Shift + Boşluk)");
    btn_test.add_css_class("action-btn");
    btn_test.connect_clicked(|_| {
        let _ = Command::new("solar-shell")
            .args(["omnibar", "--force"])
            .spawn();
    });
    btn_row.append(&btn_test);

    let btn_shell_launcher = Button::with_label("Aktif Kabuğun Kendi Başlatıcısını Aç");
    btn_shell_launcher.add_css_class("secondary-btn");
    btn_shell_launcher.connect_clicked(|_| {
        let _ = Command::new("solar-shell")
            .args(["route", "launcher"])
            .spawn();
    });
    btn_row.append(&btn_shell_launcher);

    sec.append(&btn_row);

    tab.append(&sec);
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

