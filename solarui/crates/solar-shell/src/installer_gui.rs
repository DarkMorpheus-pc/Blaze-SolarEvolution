#![allow(deprecated)]
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, CheckButton, ComboBoxText, Entry, Image, Label, Orientation,
    PasswordEntry, ProgressBar, ScrolledWindow, Stack, StackTransitionType, TextView, Window,
};
use gtk4::{gdk, glib};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;

use crate::living_flow::{
    apply_living_flow_theme, build_living_flow_topbar, FluidCloud, FluidCloudTone,
};

const INSTALLER_EXTRA_CSS: &str = r#"
    .installer-body {
        padding: 20px 24px 20px 24px;
    }

    .installer-header {
        margin-bottom: 16px;
    }

    .installer-hero-title {
        font-size: 20px;
        font-weight: 600;
        color: #ededed;
        letter-spacing: 0px;
    }

    .installer-hero-sub {
        font-size: 13px;
        font-weight: 400;
        color: #a1a1a6;
        letter-spacing: 0px;
    }

    .stepper-bar {
        background-color: #19191b;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 12px;
        padding: 4px;
        margin-bottom: 16px;
    }

    .log-view,
    .log-view text {
        background-color: #0f0f10;
        color: #a1a1a6;
        font-family: monospace;
        font-size: 12px;
        font-feature-settings: "tnum";
        padding: 12px;
        border-radius: 8px;
    }

    .log-frame {
        background-color: #0f0f10;
        border: 1px solid rgba(237, 237, 237, 0.08);
        border-radius: 12px;
        padding: 4px;
    }
"#;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProbedDisk {
    pub path: String,
    pub bytes: u64,
    pub model: String,
    pub removable: bool,
    pub read_only: bool,
    pub mounted: bool,
    pub eligible: bool,
    #[serde(default)]
    pub rejection_reasons: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct InstallerAnswersPayload {
    hostname: String,
    locale: String,
    timezone: String,
    keyboard_layout: String,
    username: String,
    full_name: String,
}

fn localize_probe_message(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("disk already contains ") {
        if let Some(count_str) = rest.split_whitespace().next() {
            return format!(
                "Diskte kurulum sırasında silinecek {} mevcut bölüm bulunuyor",
                count_str
            );
        }
    }
    if let Some(rest) = trimmed.strip_prefix("partition ") {
        if let Some((dev, label_part)) = rest.split_once(" is an existing EFI System Partition ") {
            return format!(
                "{} üzerinde mevcut bir EFI sistem bölümü {} yer alıyor",
                dev, label_part
            );
        }
        if let Some((dev, label_part)) = rest.split_once(" carries Windows/Microsoft label ") {
            return format!(
                "{} bölümü Windows/Microsoft etiketi ({}) taşıyor",
                dev, label_part
            );
        }
    }
    match trimmed {
        "smaller than 16 GiB" => "Disk kapasitesi en az 16 GiB olmalı".to_string(),
        "read-only device" => "Salt okunur depolama aygıtı".to_string(),
        "device or partition is mounted" => {
            "Disk şu anda çalışan sistem tarafından kullanılıyor".to_string()
        }
        other if other.contains("removable") => {
            "Çıkarılabilir aygıt algılandı; kurulum için aşağıdaki izni etkinleştirin".to_string()
        }
        other => other.to_string(),
    }
}

fn format_disk_feedback(d: &ProbedDisk) -> (String, bool) {
    let mut parts = Vec::new();
    for r in &d.rejection_reasons {
        parts.push(localize_probe_message(r));
    }
    for w in &d.warnings {
        parts.push(localize_probe_message(w));
    }
    if parts.is_empty() {
        ("Seçilen disk temiz kurulum için hazır".to_string(), true)
    } else if !d.eligible {
        (format!("Uygun değil: {}", parts.join(" · ")), false)
    } else {
        (format!("Uyarı: {}", parts.join(" · ")), false)
    }
}

fn probe_disks() -> Vec<ProbedDisk> {
    for cmd_pair in [
        vec!["solar-installer", "probe", "--json"],
        vec!["sudo", "-n", "solar-installer", "probe", "--json"],
    ] {
        if let Ok(out) = Command::new(cmd_pair[0]).args(&cmd_pair[1..]).output() {
            if out.status.success() {
                if let Ok(list) = serde_json::from_slice::<Vec<ProbedDisk>>(&out.stdout) {
                    if !list.is_empty() {
                        return list;
                    }
                }
            }
        }
    }

    let mut disks = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("zram")
                || name.starts_with("sr")
                || name.starts_with("dm-")
            {
                continue;
            }
            let sectors = std::fs::read_to_string(entry.path().join("size"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .unwrap_or(0);
            let bytes = sectors * 512;
            let model = std::fs::read_to_string(entry.path().join("device/model"))
                .map(|s| s.trim().to_string())
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "VirtIO / NVMe depolama".to_string());
            let removable = std::fs::read_to_string(entry.path().join("removable"))
                .map(|s| s.trim() == "1")
                .unwrap_or(false);
            let ro = std::fs::read_to_string(entry.path().join("ro"))
                .map(|s| s.trim() == "1")
                .unwrap_or(false);
            let eligible = bytes >= 16 * 1024 * 1024 * 1024 && !ro;
            disks.push(ProbedDisk {
                path: format!("/dev/{}", name),
                bytes,
                model,
                removable,
                read_only: ro,
                mounted: false,
                eligible,
                rejection_reasons: if eligible {
                    vec![]
                } else {
                    vec!["En az 16 GiB yazılabilir disk gerekli".to_string()]
                },
                warnings: vec![],
            });
        }
    }
    disks
}

pub fn launch_installer_window() {
    glib::set_prgname(Some("solar-installer-gui"));
    glib::set_application_name("BlazeOS Kurulum");

    if gtk4::init().is_err() {
        eprintln!("Failed to initialize GTK4 for Solar Installer GUI");
        return;
    }

    apply_living_flow_theme(INSTALLER_EXTRA_CSS);

    let main_loop = glib::MainLoop::new(None, false);
    let window = Window::builder()
        .title("BlazeOS kurulumu")
        .decorated(false)
        .default_width(920)
        .default_height(640)
        .resizable(false)
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

    let fluid_cloud = FluidCloud::new("Adım 1/3: Hedef disk");
    let emblem = Image::from_icon_name("drive-harddisk-symbolic");
    emblem.set_pixel_size(16);
    emblem.add_css_class("lf-topbar-icon");
    let loop_top = main_loop.clone();
    let topbar = build_living_flow_topbar(
        &window,
        Some(emblem),
        "BlazeOS kurulumu",
        "Blaze SolarEvolution",
        Some(&fluid_cloud),
        Some(Box::new(move || loop_top.quit())),
    );
    shell_box.append(&topbar);

    let body = GtkBox::new(Orientation::Vertical, 0);
    body.add_css_class("installer-body");
    body.set_vexpand(true);

    // Calm page header directly on graphite surface
    let header = GtkBox::new(Orientation::Vertical, 4);
    header.add_css_class("installer-header");
    let h_title = Label::new(Some("BlazeOS sistem kurulumu"));
    h_title.add_css_class("installer-hero-title");
    h_title.set_halign(gtk4::Align::Start);
    let h_sub = Label::new(Some(
        "Hedef diski seçin, yönetici hesabını oluşturun ve kurulumu başlatın",
    ));
    h_sub.add_css_class("installer-hero-sub");
    h_sub.set_halign(gtk4::Align::Start);
    header.append(&h_title);
    header.append(&h_sub);
    body.append(&header);

    // Segmented 3-step bar
    let stepper = GtkBox::new(Orientation::Horizontal, 4);
    stepper.add_css_class("stepper-bar");
    stepper.set_halign(gtk4::Align::Start);

    let step_btn1 = Button::with_label("1. Hedef disk");
    step_btn1.add_css_class("lf-step-pill");
    step_btn1.add_css_class("active");

    let step_btn2 = Button::with_label("2. Hesap ve bölge");
    step_btn2.add_css_class("lf-step-pill");

    let step_btn3 = Button::with_label("3. Onay ve kurulum");
    step_btn3.add_css_class("lf-step-pill");

    stepper.append(&step_btn1);
    stepper.append(&step_btn2);
    stepper.append(&step_btn3);
    body.append(&stepper);

    let stack = Stack::new();
    stack.set_transition_type(StackTransitionType::SlideLeftRight);
    stack.set_transition_duration(200);
    stack.set_vexpand(true);

    let switch_step = {
        let stack_c = stack.clone();
        let b1 = step_btn1.clone();
        let b2 = step_btn2.clone();
        let b3 = step_btn3.clone();
        let cloud = fluid_cloud.clone();
        Rc::new(move |idx: usize| {
            b1.remove_css_class("active");
            b2.remove_css_class("active");
            b3.remove_css_class("active");
            match idx {
                0 => {
                    b1.add_css_class("active");
                    stack_c.set_visible_child_name("step1");
                    cloud.pulse("Adım 1/3: Hedef disk", FluidCloudTone::Info, false);
                }
                1 => {
                    b2.add_css_class("active");
                    stack_c.set_visible_child_name("step2");
                    cloud.pulse("Adım 2/3: Hesap ve bölge", FluidCloudTone::Info, false);
                }
                _ => {
                    b3.add_css_class("active");
                    stack_c.set_visible_child_name("step3");
                    cloud.pulse("Adım 3/3: Onay ve kurulum", FluidCloudTone::Success, false);
                }
            }
        })
    };

    {
        let sw = switch_step.clone();
        step_btn1.connect_clicked(move |_| sw(0));
    }
    {
        let sw = switch_step.clone();
        step_btn2.connect_clicked(move |_| sw(1));
    }
    {
        let sw = switch_step.clone();
        step_btn3.connect_clicked(move |_| sw(2));
    }

    let disks_state: Rc<RefCell<Vec<ProbedDisk>>> = Rc::new(RefCell::new(Vec::new()));

    // Step 1: Target disk selection
    let page_disk = GtkBox::new(Orientation::Vertical, 12);
    page_disk.set_margin_top(4);

    let disk_card = GtkBox::new(Orientation::Vertical, 12);
    disk_card.add_css_class("installer-card");

    let d_head = Label::new(Some("Hedef diski seçin"));
    d_head.add_css_class("section-heading");
    d_head.set_halign(gtk4::Align::Start);
    disk_card.append(&d_head);

    let d_desc = Label::new(Some(
        "Seçilen diskte GPT bölüm tablosu, 512 MiB EFI bölümü, 1 MiB BIOS önyükleme bölümü ve ext4 kök bölümü oluşturulur",
    ));
    d_desc.add_css_class("section-desc");
    d_desc.set_wrap(true);
    d_desc.set_xalign(0.0);
    d_desc.set_halign(gtk4::Align::Start);
    disk_card.append(&d_desc);

    let disk_combo = ComboBoxText::new();
    let disk_warning_lbl = Label::new(Some(""));
    disk_warning_lbl.add_css_class("disk-badge-warn");
    disk_warning_lbl.set_halign(gtk4::Align::Fill);
    disk_warning_lbl.set_xalign(0.0);
    disk_warning_lbl.set_wrap(true);

    let apply_badge_style = |lbl: &Label, is_clean: bool| {
        lbl.remove_css_class("disk-badge-ok");
        lbl.remove_css_class("disk-badge-warn");
        if is_clean {
            lbl.add_css_class("disk-badge-ok");
        } else {
            lbl.add_css_class("disk-badge-warn");
        }
    };

    let populate_disks = {
        let disks_state = disks_state.clone();
        let disk_combo = disk_combo.clone();
        let disk_warning_lbl = disk_warning_lbl.clone();
        move || {
            disk_combo.remove_all();
            let list = probe_disks();
            *disks_state.borrow_mut() = list.clone();
            for d in &list {
                let gib = (d.bytes as f64) / (1024.0 * 1024.0 * 1024.0);
                let model_disp = if d.model.trim().is_empty() {
                    "Depolama aygıtı"
                } else {
                    d.model.trim()
                };
                let status = if d.eligible {
                    "Uygun"
                } else if d.mounted {
                    "Aktif sistem diski"
                } else {
                    "Uygun değil"
                };
                let label = format!("{} · {} ({:.1} GiB) · {}", d.path, model_disp, gib, status);
                disk_combo.append(Some(&d.path), &label);
            }
            if let Some(first) = list.iter().find(|d| d.eligible).or_else(|| list.first()) {
                disk_combo.set_active_id(Some(&first.path));
                let (txt, is_clean) = format_disk_feedback(first);
                apply_badge_style(&disk_warning_lbl, is_clean);
                disk_warning_lbl.set_text(&txt);
            } else {
                apply_badge_style(&disk_warning_lbl, false);
                disk_warning_lbl.set_text("Kuruluma uygun depolama aygıtı bulunamadı");
            }
            list.len()
        }
    };
    let _ = populate_disks();

    {
        let disks_state = disks_state.clone();
        let disk_warning_lbl = disk_warning_lbl.clone();
        disk_combo.connect_changed(move |cb| {
            if let Some(id) = cb.active_id() {
                if let Some(d) = disks_state.borrow().iter().find(|x| x.path == id.as_str()) {
                    let (txt, is_clean) = format_disk_feedback(d);
                    apply_badge_style(&disk_warning_lbl, is_clean);
                    disk_warning_lbl.set_text(&txt);
                }
            }
        });
    }

    let disk_row = GtkBox::new(Orientation::Horizontal, 8);
    disk_combo.set_hexpand(true);
    disk_row.append(&disk_combo);

    let refresh_btn = Button::with_label("Diskleri yenile");
    refresh_btn.add_css_class("lf-btn-glass");
    let pop_clone = populate_disks.clone();
    let cloud_refresh = fluid_cloud.clone();
    refresh_btn.connect_clicked(move |_| {
        let count = pop_clone();
        cloud_refresh.pulse(
            &format!("{} disk tarandı", count),
            FluidCloudTone::Success,
            false,
        );
    });
    disk_row.append(&refresh_btn);
    if !Path::new("/usr/bin/solar-installer").exists()
        && (Path::new("/usr/bin/liveinst").exists() || Path::new("/usr/bin/blaze-setup").exists())
    {
        let fb_btn = Button::with_label("Anaconda kurulumunu aç");
        fb_btn.add_css_class("lf-btn-glass");
        let win_fb = window.downgrade();
        fb_btn.connect_clicked(move |_| {
            if Path::new("/usr/bin/liveinst").exists() {
                let _ = Command::new("/usr/bin/liveinst").spawn();
            } else if Path::new("/usr/bin/blaze-setup").exists() {
                let _ = Command::new("/usr/bin/blaze-setup").spawn();
            }
            if let Some(w) = win_fb.upgrade() {
                w.close();
            }
        });
        disk_row.append(&fb_btn);
    }
    disk_card.append(&disk_row);
    disk_card.append(&disk_warning_lbl);

    let chk_removable = CheckButton::with_label(
        "Çıkarılabilir USB veya harici disklere kuruluma izin ver",
    );
    let cloud_rem = fluid_cloud.clone();
    chk_removable.connect_toggled(move |cb| {
        if cb.is_active() {
            cloud_rem.pulse(
                "Çıkarılabilir disklere izin verildi",
                FluidCloudTone::Warning,
                false,
            );
        } else {
            cloud_rem.pulse(
                "Yalnızca dahili diskler etkin",
                FluidCloudTone::Info,
                false,
            );
        }
    });
    disk_card.append(&chk_removable);

    page_disk.append(&disk_card);

    let nav_disk = GtkBox::new(Orientation::Horizontal, 8);
    nav_disk.set_halign(gtk4::Align::End);
    let to_step2_btn = Button::with_label("İleri");
    to_step2_btn.add_css_class("lf-btn-flame");
    let sw_to_2 = switch_step.clone();
    to_step2_btn.connect_clicked(move |_| {
        sw_to_2(1);
    });
    nav_disk.append(&to_step2_btn);
    page_disk.append(&nav_disk);

    stack.add_named(&page_disk, Some("step1"));

    // Step 2: Account and regional configuration
    let page_user = GtkBox::new(Orientation::Vertical, 12);
    page_user.set_margin_top(4);

    let user_scroll = ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .vexpand(true)
        .build();

    let user_card = GtkBox::new(Orientation::Vertical, 12);
    user_card.add_css_class("installer-card");

    let u_head = Label::new(Some("Yönetici hesabı ve bölge"));
    u_head.add_css_class("section-heading");
    u_head.set_halign(gtk4::Align::Start);
    user_card.append(&u_head);

    let entry_fullname = Entry::builder()
        .placeholder_text("Ad ve soyad")
        .text("Blaze Kullanıcısı")
        .build();
    let entry_username = Entry::builder()
        .placeholder_text("Küçük harf, örn. blaze")
        .text("blaze")
        .build();
    let entry_hostname = Entry::builder()
        .placeholder_text("Örn. blazeos")
        .text("blazeos")
        .build();
    let entry_pass = PasswordEntry::builder()
        .placeholder_text("En az 8 karakter")
        .show_peek_icon(true)
        .build();
    let entry_pass2 = PasswordEntry::builder()
        .placeholder_text("Parolayı tekrar girin")
        .show_peek_icon(true)
        .build();

    let combo_kb = ComboBoxText::new();
    combo_kb.append(Some("tr"), "Türkçe (Q)");
    combo_kb.append(Some("us"), "İngilizce (ABD)");
    combo_kb.append(Some("de"), "Almanca");
    combo_kb.append(Some("fr"), "Fransızca");
    combo_kb.set_active_id(Some("tr"));
    let cloud_kb = fluid_cloud.clone();
    combo_kb.connect_changed(move |cb| {
        if let Some(id) = cb.active_id() {
            let idx = match id.as_str() {
                "tr" => "0",
                "us" => "1",
                _ => "0",
            };
            let _ = Command::new("niri")
                .args(["msg", "action", "switch-layout", idx])
                .status();
            cloud_kb.pulse(
                &format!("Klavye düzeni: {}", id),
                FluidCloudTone::Success,
                false,
            );
        }
    });

    let combo_tz = ComboBoxText::new();
    combo_tz.append(Some("Europe/Istanbul"), "Europe/Istanbul (GMT+3)");
    combo_tz.append(Some("UTC"), "UTC");
    combo_tz.append(Some("Europe/Berlin"), "Europe/Berlin");
    combo_tz.append(Some("Europe/London"), "Europe/London");
    combo_tz.append(Some("America/New_York"), "America/New_York");
    combo_tz.set_active_id(Some("Europe/Istanbul"));

    let combo_locale = ComboBoxText::new();
    combo_locale.append(Some("tr_TR.UTF-8"), "Türkçe (tr_TR.UTF-8)");
    combo_locale.append(Some("en_US.UTF-8"), "English (en_US.UTF-8)");
    combo_locale.set_active_id(Some("tr_TR.UTF-8"));

    for (lbl_txt, widget) in [
        ("Tam ad", entry_fullname.clone().upcast::<gtk4::Widget>()),
        ("Kullanıcı adı", entry_username.clone().upcast::<gtk4::Widget>()),
        ("Bilgisayar adı", entry_hostname.clone().upcast::<gtk4::Widget>()),
        ("Parola", entry_pass.clone().upcast::<gtk4::Widget>()),
        ("Parola tekrarı", entry_pass2.clone().upcast::<gtk4::Widget>()),
        ("Klavye düzeni", combo_kb.clone().upcast::<gtk4::Widget>()),
        ("Saat dilimi", combo_tz.clone().upcast::<gtk4::Widget>()),
        ("Sistem dili", combo_locale.clone().upcast::<gtk4::Widget>()),
    ] {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        let lbl = Label::new(Some(lbl_txt));
        lbl.add_css_class("lf-label");
        lbl.set_width_chars(18);
        lbl.set_xalign(0.0);
        widget.set_hexpand(true);
        row.append(&lbl);
        row.append(&widget);
        user_card.append(&row);
    }

    user_scroll.set_child(Some(&user_card));
    page_user.append(&user_scroll);

    let nav_user = GtkBox::new(Orientation::Horizontal, 8);
    nav_user.set_halign(gtk4::Align::End);
    let back_step1_btn = Button::with_label("Geri");
    back_step1_btn.add_css_class("lf-btn-glass");
    let sw_back_1 = switch_step.clone();
    back_step1_btn.connect_clicked(move |_| {
        sw_back_1(0);
    });
    let to_step3_btn = Button::with_label("İleri");
    to_step3_btn.add_css_class("lf-btn-flame");
    let sw_to_3 = switch_step.clone();
    to_step3_btn.connect_clicked(move |_| {
        sw_to_3(2);
    });
    nav_user.append(&back_step1_btn);
    nav_user.append(&to_step3_btn);
    page_user.append(&nav_user);

    stack.add_named(&page_user, Some("step2"));

    // Step 3: Confirmation, dry-run plan preview, and disk write (`solar-installer apply`)
    let page_apply = GtkBox::new(Orientation::Vertical, 12);
    page_apply.set_margin_top(4);

    let apply_card = GtkBox::new(Orientation::Vertical, 12);
    apply_card.add_css_class("installer-card");
    apply_card.set_vexpand(true);

    let a_head = Label::new(Some("Onay ve kurulum"));
    a_head.add_css_class("section-heading");
    a_head.set_halign(gtk4::Align::Start);
    apply_card.append(&a_head);

    let chk_confirm_erase = CheckButton::with_label(
        "Seçilen diskteki tüm verilerin silineceğini onaylıyorum",
    );
    apply_card.append(&chk_confirm_erase);

    let progress = ProgressBar::new();
    progress.set_show_text(true);
    progress.set_text(Some("Onay bekleniyor"));
    apply_card.append(&progress);

    let log_scroll = ScrolledWindow::builder()
        .min_content_height(176)
        .vexpand(true)
        .build();
    log_scroll.add_css_class("log-frame");
    let log_view = TextView::new();
    log_view.set_editable(false);
    log_view.set_monospace(true);
    log_view.add_css_class("log-view");
    log_scroll.set_child(Some(&log_view));
    apply_card.append(&log_scroll);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    let back_step2_btn = Button::with_label("Geri");
    back_step2_btn.add_css_class("lf-btn-glass");
    let sw_back_2 = switch_step.clone();
    back_step2_btn.connect_clicked(move |_| {
        sw_back_2(1);
    });
    action_row.append(&back_step2_btn);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    action_row.append(&spacer);

    let plan_btn = Button::with_label("Planı önizle");
    plan_btn.add_css_class("lf-btn-glass");

    let install_btn = Button::with_label("Diske kur");
    install_btn.add_css_class("lf-btn-danger");

    let reboot_btn = Button::with_label("Yeniden başlat");
    reboot_btn.add_css_class("lf-btn-primary");
    reboot_btn.set_visible(false);
    reboot_btn.connect_clicked(|_| {
        let _ = Command::new("sudo").args(["-n", "reboot"]).spawn();
        let _ = Command::new("systemctl").arg("reboot").spawn();
    });

    // Plan preview button logic
    {
        let disk_combo = disk_combo.clone();
        let log_view = log_view.clone();
        let cloud_plan = fluid_cloud.clone();
        plan_btn.connect_clicked(move |_| {
            let target = disk_combo
                .active_id()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "/dev/vdb".to_string());
            cloud_plan.pulse("Kurulum planı hazırlanıyor", FluidCloudTone::Info, false);
            let out = Command::new("sudo")
                .args(["-n", "solar-installer", "plan", "--target", &target])
                .output();
            let text = match out {
                Ok(o) => format!(
                    "[PLAN] Hedef: {}\n{}{}",
                    target,
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                ),
                Err(e) => format!("[HATA] solar-installer plan çalıştırılamadı: {}", e),
            };
            log_view.buffer().set_text(&text);
        });
    }

    // Install button logic
    {
        let disk_combo = disk_combo.clone();
        let chk_removable = chk_removable.clone();
        let chk_confirm_erase = chk_confirm_erase.clone();
        let entry_fullname = entry_fullname.clone();
        let entry_username = entry_username.clone();
        let entry_hostname = entry_hostname.clone();
        let entry_pass = entry_pass.clone();
        let entry_pass2 = entry_pass2.clone();
        let combo_kb = combo_kb.clone();
        let combo_tz = combo_tz.clone();
        let combo_locale = combo_locale.clone();
        let progress = progress.clone();
        let log_view = log_view.clone();
        let reboot_btn = reboot_btn.clone();
        let cloud_inst = fluid_cloud.clone();

        install_btn.connect_clicked(move |btn| {
            if !chk_confirm_erase.is_active() {
                cloud_inst.pulse("Disk silme onayı gerekli", FluidCloudTone::Warning, false);
                log_view
                    .buffer()
                    .set_text("[UYARI] Devam etmeden önce disk silme onayını işaretleyin");
                return;
            }
            let pass1 = entry_pass.text().to_string();
            let pass2 = entry_pass2.text().to_string();
            if pass1.len() < 8 {
                cloud_inst.pulse("Parola en az 8 karakter olmalı", FluidCloudTone::Warning, false);
                log_view
                    .buffer()
                    .set_text("[HATA] Parola en az 8 karakter olmalı");
                return;
            }
            if pass1 != pass2 {
                cloud_inst.pulse("Parolalar eşleşmiyor", FluidCloudTone::Danger, false);
                log_view
                    .buffer()
                    .set_text("[HATA] Girilen parolalar eşleşmiyor");
                return;
            }
            let Some(target) = disk_combo.active_id().map(|s| s.to_string()) else {
                log_view
                    .buffer()
                    .set_text("[HATA] Önce 1. adımdan bir hedef disk seçin");
                return;
            };

            let payload = InstallerAnswersPayload {
                hostname: entry_hostname.text().trim().to_string(),
                locale: combo_locale
                    .active_id()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "tr_TR.UTF-8".to_string()),
                timezone: combo_tz
                    .active_id()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "Europe/Istanbul".to_string()),
                keyboard_layout: combo_kb
                    .active_id()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "tr".to_string()),
                username: entry_username.text().trim().to_string(),
                full_name: entry_fullname.text().trim().to_string(),
            };

            let answers_path = PathBuf::from("/tmp/solar-installer-answers.json");
            if let Ok(json_bytes) = serde_json::to_vec_pretty(&payload) {
                let _ = std::fs::write(&answers_path, json_bytes);
            }

            let allow_rem = chk_removable.is_active();
            if !Path::new("/usr/bin/solar-installer").exists() {
                if Path::new("/usr/bin/liveinst").exists() {
                    let _ = Command::new("/usr/bin/liveinst").spawn();
                    return;
                } else if Path::new("/usr/bin/blaze-setup").exists() {
                    let _ = Command::new("/usr/bin/blaze-setup").spawn();
                    return;
                }
            }
            btn.set_sensitive(false);
            cloud_inst.pulse("Kurulum devam ediyor", FluidCloudTone::Info, true);
            progress.set_fraction(0.15);
            progress.set_text(Some("Disk bölümleniyor ve sistem kuruluyor"));

            let (tx, rx) = async_channel::unbounded::<(f64, String, bool)>();
            std::thread::spawn(move || {
                let mut args = vec![
                    "-n".to_string(),
                    "solar-installer".to_string(),
                    "apply".to_string(),
                    "--target".to_string(),
                    target.clone(),
                    "--confirm-erase".to_string(),
                    target.clone(),
                    "--answers".to_string(),
                    answers_path.to_string_lossy().to_string(),
                    "--password-stdin".to_string(),
                ];
                if allow_rem {
                    args.push("--allow-removable".to_string());
                }

                let _ = tx.send_blocking((
                    0.25,
                    format!("[BAŞLATILDI] Hedef disk: {}\n", target),
                    false,
                ));

                let child_res = Command::new("sudo")
                    .args(&args)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();

                match child_res {
                    Ok(mut child) => {
                        if let Some(mut stdin) = child.stdin.take() {
                            let _ = stdin.write_all(format!("{}\n", pass1).as_bytes());
                        }
                        if let Some(stdout) = child.stdout.take() {
                            let reader = BufReader::new(stdout);
                            for line in reader.lines().flatten() {
                                let _ = tx.send_blocking((0.65, format!("{}\n", line), false));
                            }
                        }
                        let out = child.wait_with_output();
                        let _ = std::fs::remove_file(&answers_path);
                        match out {
                            Ok(res) if res.status.success() => {
                                let _ = tx.send_blocking((
                                    1.0,
                                    "\n[TAMAMLANDI] BlazeOS diske kuruldu. Sistemi yeniden başlatabilirsiniz.\n".to_string(),
                                    true,
                                ));
                            }
                            Ok(res) => {
                                let err_txt = String::from_utf8_lossy(&res.stderr);
                                let _ = tx.send_blocking((
                                    0.0,
                                    format!("\n[HATA] Kurulum başarısız oldu:\n{}\n", err_txt),
                                    false,
                                ));
                            }
                            Err(e) => {
                                let _ = tx.send_blocking((
                                    0.0,
                                    format!("\n[HATA] Süreç beklenirken hata: {}\n", e),
                                    false,
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx.send_blocking((
                            0.0,
                            format!("[HATA] solar-installer başlatılamadı: {}\n", e),
                            false,
                        ));
                    }
                }
            });

            let progress_ui = progress.clone();
            let log_ui = log_view.clone();
            let reboot_ui = reboot_btn.clone();
            let btn_ui = btn.clone();
            let cloud_done = cloud_inst.clone();
            glib::MainContext::default().spawn_local(async move {
                let mut accumulated = String::new();
                while let Ok((frac, msg, done)) = rx.recv().await {
                    accumulated.push_str(&msg);
                    log_ui.buffer().set_text(&accumulated);
                    progress_ui.set_fraction(frac);
                    if done {
                        progress_ui.set_text(Some("Kurulum tamamlandı"));
                        cloud_done.pulse("Kurulum tamamlandı", FluidCloudTone::Success, true);
                        reboot_ui.set_visible(true);
                    } else if frac == 0.0 {
                        progress_ui.set_text(Some("Kurulum hata nedeniyle durduruldu"));
                        cloud_done.pulse("Kurulum başarısız oldu", FluidCloudTone::Danger, false);
                        btn_ui.set_sensitive(true);
                    }
                }
            });
        });
    }

    action_row.append(&plan_btn);
    action_row.append(&install_btn);
    action_row.append(&reboot_btn);
    apply_card.append(&action_row);
    page_apply.append(&apply_card);

    stack.add_named(&page_apply, Some("step3"));
    stack.set_visible_child_name("step1");

    body.append(&stack);
    shell_box.append(&body);
    window.set_child(Some(&shell_box));
    window.present();
    main_loop.run();
}
