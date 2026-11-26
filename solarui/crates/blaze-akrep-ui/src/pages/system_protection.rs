use crate::components::{
    build_admin_note, build_duration_selector, build_kv_row, build_risk_indicator,
    build_severity_indicator, build_status_indicator, duration_from_choice_idx,
    format_override_status_text, risk_css_class, OOMD_DURATION_CHOICES,
    PROTECTION_DURATION_CHOICES,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{
    ActionRequest, CompatPackageGroup, OverrideDuration, SelinuxBooleanEntry, SelinuxDesiredMode,
    SelinuxDiagnoseQuery, SelinuxRuntimeMode, SuggestedSelinuxFix,
};
use blaze_sentinel_core::{AkrepSeverity, AkrepSnapshot};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

/// Expands leading `~/` or `~` using `HOME` (or test override) for container folder actions.
pub fn expand_user_container_path(raw: &str, home_override: Option<&str>) -> String {
    let trimmed = raw.trim();
    let home = home_override
        .map(ToString::to_string)
        .or_else(|| std::env::var("HOME").ok())
        .filter(|h| !h.trim().is_empty() && h != "/");
    if let Some(h) = home {
        let h_clean = h.trim_end_matches('/');
        if trimmed == "~" {
            return h_clean.to_string();
        }
        if let Some(rest) = trimmed.strip_prefix("~/") {
            return format!("{}/{}", h_clean, rest);
        }
    }
    trimmed.to_string()
}

/// Builds a `DevLimitsSetNofile` request keeping Fedora's 524288 hard limit intact.
pub fn nofile_action_request(enabled: bool) -> ActionRequest {
    ActionRequest::DevLimitsSetNofile {
        enabled,
        soft_limit: 65_536,
        hard_limit: 524_288,
    }
}

fn detect_selinux_mode_state(
    snap: Option<&AkrepSnapshot>,
    diag: Option<&blaze_akrep_common::SelinuxDiagnoseResponse>,
) -> (bool, bool, Option<AkrepSeverity>) {
    let sel_override = snap.and_then(|sn| {
        sn.active_overrides
            .iter()
            .find(|o| o.subsystem == "selinux")
    });
    let snap_metric = snap.and_then(|sn| {
        sn.cards
            .iter()
            .find(|c| c.id == "system_protection")
            .map(|c| c.metric.as_str())
    });
    let is_permissive = sel_override.is_some()
        || snap_metric.is_some_and(|m| m.eq_ignore_ascii_case("Permissive"))
        || (snap_metric.is_none()
            && diag.is_some_and(|d| d.runtime_mode == SelinuxRuntimeMode::Permissive));
    let is_disabled = snap_metric.is_some_and(|m| m.eq_ignore_ascii_case("Disabled"))
        || (snap_metric.is_none()
            && diag.is_some_and(|d| d.runtime_mode == SelinuxRuntimeMode::Disabled));

    let sev = if is_disabled {
        Some(AkrepSeverity::Critical)
    } else if is_permissive {
        Some(AkrepSeverity::Attention)
    } else if snap_metric.is_some_and(|m| m.eq_ignore_ascii_case("Enforcing"))
        || diag.is_some_and(|d| d.runtime_mode == SelinuxRuntimeMode::Enforcing)
    {
        Some(AkrepSeverity::Protected)
    } else {
        None
    };

    (is_permissive, is_disabled, sev)
}

fn build_dev_compat_card(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    snap: Option<&AkrepSnapshot>,
    lang: Lang,
) -> GtkBox {
    let st = state.borrow();
    let oomd_duration_idx = st.oomd_duration_idx;
    let container_dir_path = st.container_dir_path.clone();
    let container_dir_recursive = st.container_dir_recursive;
    drop(st);

    let compat = snap.and_then(|s| s.system_compat.as_ref());
    let oomd_override =
        snap.and_then(|s| s.active_overrides.iter().find(|o| o.subsystem == "oomd"));

    let card = GtkBox::new(Orientation::Vertical, 12);
    card.add_css_class("akrep-detail-box");

    let hdr = Label::new(Some(lang.pick(
        "Geliştirici ve derleme uyumluluğu",
        "Developer and build compatibility",
    )));
    hdr.add_css_class("akrep-section-hdr");
    hdr.set_xalign(0.0);
    card.append(&hdr);

    let sub = Label::new(Some(lang.pick(
        "Ağır derleme iş yükleri, konteyner klasörleri ve Android geliştirme araçları",
        "Heavy build workloads, container folders and Android development tools",
    )));
    sub.add_css_class("akrep-subtitle");
    sub.set_xalign(0.0);
    sub.set_wrap(true);
    sub.set_wrap_mode(WrapMode::WordChar);
    card.append(&sub);
    card.append(&build_admin_note(lang));

    // 1. systemd-oomd pause/resume + duration selector
    let oomd_box = GtkBox::new(Orientation::Vertical, 8);
    oomd_box.add_css_class("akrep-diff-row");

    let oomd_top = GtkBox::new(Orientation::Horizontal, 12);
    oomd_top.set_valign(Align::Center);
    let oomd_title = Label::new(Some(lang.pick(
        "Bellek yöneticisi (systemd-oomd)",
        "Memory manager (systemd-oomd)",
    )));
    oomd_title.add_css_class("akrep-card-title");
    oomd_title.set_xalign(0.0);
    oomd_title.set_wrap(true);
    oomd_title.set_hexpand(true);
    oomd_top.append(&oomd_title);

    let oomd_paused = oomd_override.is_some() || compat.is_some_and(|c| !c.oomd_active);
    if oomd_paused {
        let status_lbl = if let Some(ov) = oomd_override {
            format!(
                "{} · {}",
                lang.pick("Duraklatıldı", "Paused"),
                format_override_status_text(ov, lang)
            )
        } else {
            lang.pick("Duraklatıldı", "Paused").to_string()
        };
        oomd_top.append(&build_status_indicator(
            "akrep-sev-attention",
            &status_lbl,
            false,
        ));
        let resume_btn = Button::with_label(lang.pick("Etkinleştir", "Enable"));
        resume_btn.add_css_class("akrep-ghost-btn");
        let cmd_on = cmd_tx.clone();
        resume_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_on,
                UiCommand::CreatePlan {
                    request: ActionRequest::OomdSetState {
                        enabled: true,
                        duration: OverrideDuration::UntilChanged,
                    },
                },
            );
        });
        oomd_top.append(&resume_btn);
    } else {
        let (dot_cls, dot_txt, muted) = if compat.is_some() {
            ("akrep-sev-protected", lang.pick("Etkin", "Active"), false)
        } else {
            (
                "akrep-sev-unavailable",
                lang.pick("Durum bekleniyor", "Awaiting state"),
                true,
            )
        };
        oomd_top.append(&build_status_indicator(dot_cls, dot_txt, muted));
    }
    oomd_box.append(&oomd_top);

    let oomd_desc = Label::new(Some(lang.pick(
        "AOSP, Rust veya C++ derlemelerinde yüksek bellek nedeniyle süreç sonlandırmayı önler",
        "Prevents premature process termination during heavy AOSP, Rust or C++ builds",
    )));
    oomd_desc.add_css_class("akrep-meta");
    oomd_desc.set_xalign(0.0);
    oomd_desc.set_wrap(true);
    oomd_desc.set_wrap_mode(WrapMode::WordChar);
    oomd_box.append(&oomd_desc);

    let oomd_ctrl = GtkBox::new(Orientation::Horizontal, 8);
    oomd_ctrl.set_valign(Align::Center);
    let st_od = Rc::clone(state);
    let oomd_sel = build_duration_selector(
        &OOMD_DURATION_CHOICES,
        oomd_duration_idx,
        lang,
        Rc::new(move |idx| {
            st_od.borrow_mut().oomd_duration_idx = idx;
        }),
    );
    oomd_ctrl.append(&oomd_sel);

    let oomd_pause_btn = Button::with_label(if oomd_paused {
        lang.pick("Süreyi güncelle", "Update duration")
    } else {
        lang.pick("Duraklat", "Pause")
    });
    oomd_pause_btn.add_css_class("akrep-ghost-btn");
    oomd_pause_btn.set_valign(Align::Center);
    let st_op = Rc::clone(state);
    let cmd_op = cmd_tx.clone();
    oomd_pause_btn.connect_clicked(move |_| {
        let idx = st_op.borrow().oomd_duration_idx;
        let duration = duration_from_choice_idx(&OOMD_DURATION_CHOICES, idx);
        dispatch_cmd(
            &cmd_op,
            UiCommand::CreatePlan {
                request: ActionRequest::OomdSetState {
                    enabled: false,
                    duration,
                },
            },
        );
    });
    oomd_ctrl.append(&oomd_pause_btn);
    oomd_box.append(&oomd_ctrl);
    card.append(&oomd_box);

    // 2. Open file descriptor limit (nofile 65536 / 524288)
    let nofile_box = GtkBox::new(Orientation::Horizontal, 12);
    nofile_box.add_css_class("akrep-diff-row");
    nofile_box.set_valign(Align::Center);

    let nf_col = GtkBox::new(Orientation::Vertical, 4);
    nf_col.set_hexpand(true);
    let nf_title = Label::new(Some(lang.pick(
        "Açık dosya limiti (nofile 65536 / 524288)",
        "Open file limit (nofile 65536 / 524288)",
    )));
    nf_title.add_css_class("akrep-card-title");
    nf_title.set_xalign(0.0);
    nf_title.set_wrap(true);
    nf_col.append(&nf_title);

    let nf_desc = Label::new(Some(lang.pick(
        "Yumuşak sınırı 65536 yaparken Fedora sert sınırını (524288) korur",
        "Raises soft limit to 65536 while preserving Fedora hard limit (524288)",
    )));
    nf_desc.add_css_class("akrep-meta");
    nf_desc.set_xalign(0.0);
    nf_desc.set_wrap(true);
    nf_desc.set_wrap_mode(WrapMode::WordChar);
    nf_col.append(&nf_desc);
    nofile_box.append(&nf_col);

    let nofile_active = compat.is_some_and(|c| c.nofile_enabled);
    let (nf_dot, nf_txt, nf_muted) = if let Some(c) = compat {
        if c.nofile_enabled {
            (
                "akrep-sev-protected",
                format!(
                    "{} / {} {}",
                    c.nofile_soft,
                    c.nofile_hard,
                    lang.pick("etkin", "active")
                ),
                false,
            )
        } else {
            (
                "akrep-sev-unavailable",
                format!(
                    "{} ({} / {})",
                    lang.pick("Varsayılan", "Default"),
                    c.nofile_soft,
                    c.nofile_hard
                ),
                true,
            )
        }
    } else {
        (
            "akrep-sev-unavailable",
            lang.pick("Varsayılan", "Default").to_string(),
            true,
        )
    };
    nofile_box.append(&build_status_indicator(nf_dot, &nf_txt, nf_muted));

    let nf_btn = Button::with_label(if nofile_active {
        lang.pick("Varsayılana dön", "Restore default")
    } else {
        lang.pick("Etkinleştir", "Enable")
    });
    nf_btn.add_css_class("akrep-ghost-btn");
    let cmd_nf = cmd_tx.clone();
    nf_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_nf,
            UiCommand::CreatePlan {
                request: nofile_action_request(!nofile_active),
            },
        );
    });
    nofile_box.append(&nf_btn);
    card.append(&nofile_box);

    // 3. Container / ROM workspace directory label + restorecon
    let cdir_box = GtkBox::new(Orientation::Vertical, 8);
    cdir_box.add_css_class("akrep-diff-row");

    let cdir_title = Label::new(Some(lang.pick(
        "Konteyner / ROM derleme klasörü izni",
        "Container / ROM build folder label",
    )));
    cdir_title.add_css_class("akrep-card-title");
    cdir_title.set_xalign(0.0);
    cdir_box.append(&cdir_title);

    let cdir_desc = Label::new(Some(lang.pick(
        "Çalışma klasörüne container_file_t etiketi uygular (ör. ~/android veya /mnt/aosp)",
        "Applies container_file_t label to workspace folders (e.g. ~/android or /mnt/aosp)",
    )));
    cdir_desc.add_css_class("akrep-meta");
    cdir_desc.set_xalign(0.0);
    cdir_desc.set_wrap(true);
    cdir_desc.set_wrap_mode(WrapMode::WordChar);
    cdir_box.append(&cdir_desc);

    let cdir_in_row = GtkBox::new(Orientation::Horizontal, 8);
    cdir_in_row.set_valign(Align::Center);
    let cdir_ent = Entry::new();
    cdir_ent.add_css_class("akrep-entry");
    cdir_ent.set_width_chars(14);
    cdir_ent.set_hexpand(true);
    cdir_ent.set_placeholder_text(Some("~/android"));
    cdir_ent.set_text(&container_dir_path);
    let st_cd = Rc::clone(state);
    cdir_ent.connect_changed(move |ent| {
        st_cd.borrow_mut().container_dir_path = ent.text().to_string();
    });
    cdir_in_row.append(&cdir_ent);

    let cdir_rec = CheckButton::with_label(lang.pick("Alt klasörler dahil", "Include subfolders"));
    cdir_rec.set_active(container_dir_recursive);
    let st_cr = Rc::clone(state);
    cdir_rec.connect_toggled(move |chk| {
        st_cr.borrow_mut().container_dir_recursive = chk.is_active();
    });
    cdir_in_row.append(&cdir_rec);
    cdir_box.append(&cdir_in_row);

    let cdir_btn_row = GtkBox::new(Orientation::Horizontal, 8);
    let label_btn = Button::with_label(lang.pick("Etiket uygula", "Apply label"));
    label_btn.add_css_class("akrep-ghost-btn");
    let st_lb = Rc::clone(state);
    let cmd_lb = cmd_tx.clone();
    label_btn.connect_clicked(move |_| {
        let s = st_lb.borrow();
        let path = expand_user_container_path(&s.container_dir_path, None);
        let rec = s.container_dir_recursive;
        drop(s);
        dispatch_cmd(
            &cmd_lb,
            UiCommand::CreatePlan {
                request: ActionRequest::SelinuxLabelContainerDir {
                    target_path: path,
                    recursive: rec,
                },
            },
        );
    });
    cdir_btn_row.append(&label_btn);

    let restore_btn = Button::with_label(lang.pick("Varsayılan etikete dön", "Restore default label"));
    restore_btn.add_css_class("akrep-ghost-btn");
    let st_rs = Rc::clone(state);
    let cmd_rs = cmd_tx.clone();
    restore_btn.connect_clicked(move |_| {
        let s = st_rs.borrow();
        let path = expand_user_container_path(&s.container_dir_path, None);
        let rec = s.container_dir_recursive;
        drop(s);
        dispatch_cmd(
            &cmd_rs,
            UiCommand::CreatePlan {
                request: ActionRequest::SelinuxRestorecon {
                    target_path: path,
                    recursive: rec,
                },
            },
        );
    });
    cdir_btn_row.append(&restore_btn);
    cdir_box.append(&cdir_btn_row);
    card.append(&cdir_box);

    // 4. Android compatibility packages
    let pkg_box = GtkBox::new(Orientation::Vertical, 8);
    pkg_box.add_css_class("akrep-diff-row");
    let pkg_hdr = Label::new(Some(lang.pick(
        "Android geliştirme paketleri",
        "Android development packages",
    )));
    pkg_hdr.add_css_class("akrep-card-title");
    pkg_hdr.set_xalign(0.0);
    pkg_box.append(&pkg_hdr);

    for (pkg_name, desc_tr, desc_en, installed_opt, group) in [
        (
            "android-udev-rules",
            "ADB ve Fastboot aygıt kuralları",
            "ADB and Fastboot device rules",
            compat.map(|c| c.android_udev_installed),
            CompatPackageGroup::AndroidUdevRules,
        ),
        (
            "ncurses-compat-libs",
            "Eski Android derleme araçları uyumluluğu",
            "Legacy Android build tools compatibility",
            compat.map(|c| c.ncurses_compat_installed),
            CompatPackageGroup::AndroidLegacyLibs,
        ),
    ] {
        let prow = GtkBox::new(Orientation::Horizontal, 12);
        prow.set_valign(Align::Center);

        let pcol = GtkBox::new(Orientation::Vertical, 2);
        pcol.set_hexpand(true);
        let pn = Label::new(Some(pkg_name));
        pn.add_css_class("akrep-mono");
        pn.set_xalign(0.0);
        pcol.append(&pn);

        let pd = Label::new(Some(lang.pick(desc_tr, desc_en)));
        pd.add_css_class("akrep-meta");
        pd.set_xalign(0.0);
        pd.set_wrap(true);
        pcol.append(&pd);
        prow.append(&pcol);

        let (pdot, ptxt, pmuted) = match installed_opt {
            Some(true) => ("akrep-sev-protected", lang.pick("Kurulu", "Installed"), false),
            Some(false) => (
                "akrep-sev-unavailable",
                lang.pick("Kurulu değil", "Not installed"),
                true,
            ),
            None => (
                "akrep-sev-unavailable",
                lang.pick("Durum bekleniyor", "Awaiting state"),
                true,
            ),
        };
        prow.append(&build_status_indicator(pdot, ptxt, pmuted));

        if installed_opt != Some(true) {
            let ibtn = Button::with_label(lang.pick("Kur", "Install"));
            ibtn.add_css_class("akrep-ghost-btn");
            let cmd_in = cmd_tx.clone();
            ibtn.connect_clicked(move |_| {
                dispatch_cmd(
                    &cmd_in,
                    UiCommand::CreatePlan {
                        request: ActionRequest::InstallCompatibilityPackage {
                            package_group: group,
                        },
                    },
                );
            });
            prow.append(&ibtn);
        }
        pkg_box.append(&prow);
    }
    card.append(&pkg_box);

    card
}

fn populate_boolean_rows(
    list_box: &GtkBox,
    booleans: &[SelinuxBooleanEntry],
    filter: &str,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    lang: Lang,
) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let q = filter.trim().to_ascii_lowercase();
    let filtered: Vec<&SelinuxBooleanEntry> = booleans
        .iter()
        .filter(|b| {
            q.is_empty()
                || b.name.to_ascii_lowercase().contains(&q)
                || b.category.to_ascii_lowercase().contains(&q)
                || b.summary_tr.to_lowercase().contains(&q)
                || b.summary_en.to_ascii_lowercase().contains(&q)
        })
        .take(60)
        .collect();

    if filtered.is_empty() {
        let empty = Label::new(Some(lang.pick(
            "Aramayla eşleşen SELinux anahtarı yok",
            "No matching SELinux booleans found",
        )));
        empty.add_css_class("akrep-subtitle");
        empty.set_xalign(0.0);
        list_box.append(&empty);
        return;
    }

    for b in filtered {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.add_css_class("akrep-diff-row");
        row.set_valign(Align::Center);

        let info_col = GtkBox::new(Orientation::Vertical, 4);
        info_col.set_hexpand(true);

        let top = GtkBox::new(Orientation::Horizontal, 8);
        top.set_valign(Align::Center);
        let name_lbl = Label::new(Some(&b.name));
        name_lbl.add_css_class("akrep-mono");
        name_lbl.set_xalign(0.0);
        name_lbl.set_wrap(true);
        name_lbl.set_wrap_mode(WrapMode::WordChar);
        top.append(&name_lbl);

        top.append(&build_risk_indicator(b.risk, lang));

        let cat = Label::new(Some(&b.category));
        cat.add_css_class("akrep-tag");
        top.append(&cat);
        info_col.append(&top);

        let desc = Label::new(Some(lang.pick(&b.summary_tr, &b.summary_en)));
        desc.add_css_class("akrep-subtitle");
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(WrapMode::WordChar);
        info_col.append(&desc);
        row.append(&info_col);

        let state_col = GtkBox::new(Orientation::Vertical, 4);
        state_col.set_valign(Align::Center);
        state_col.set_halign(Align::End);
        state_col.append(&build_status_indicator(
            if b.current {
                "akrep-sev-attention"
            } else {
                "akrep-sev-protected"
            },
            if b.current {
                lang.pick("Açık", "On")
            } else {
                lang.pick("Kapalı", "Off")
            },
            false,
        ));

        let pers_str = if !b.persistent_known {
            lang.pick("Varsayılan", "Default")
        } else if b.persistent {
            lang.pick("Açık", "On")
        } else {
            lang.pick("Kapalı", "Off")
        };
        let p_lbl = Label::new(Some(&format!(
            "{}: {}",
            lang.pick("Kalıcı", "Persistent"),
            pers_str
        )));
        p_lbl.add_css_class("akrep-meta");
        p_lbl.set_xalign(1.0);
        state_col.append(&p_lbl);
        row.append(&state_col);

        let target_val = !b.current;
        let btn_txt = if target_val {
            lang.pick("Aç", "Enable")
        } else {
            lang.pick("Kapat", "Disable")
        };
        let toggle_btn = Button::with_label(btn_txt);
        toggle_btn.add_css_class("akrep-ghost-btn");
        toggle_btn.set_valign(Align::Center);
        let cmd_b = cmd_tx.clone();
        let st_b = Rc::clone(state);
        let bname = b.name.clone();
        toggle_btn.connect_clicked(move |_| {
            let persistent = st_b.borrow().selinux_draft.persistent_toggle;
            dispatch_cmd(
                &cmd_b,
                UiCommand::CreatePlan {
                    request: ActionRequest::SelinuxSetBoolean {
                        name: bname.clone(),
                        value: target_val,
                        persistent,
                    },
                },
            );
        });
        row.append(&toggle_btn);

        list_box.append(&row);
    }
}

pub fn build_system_protection_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let diag = st.selinux_diag.clone();
    let draft = st.selinux_draft.clone();
    let selinux_duration_idx = st.selinux_duration_idx;
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 20);

    let page_desc = Label::new(Some(lang.pick(
        "SELinux erişim kontrolü, geliştirici uyumluluğu, politika anahtarları ve dosya bağlamı onarımı",
        "SELinux access control, developer compatibility, policy booleans and file context repair",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    root.append(&page_desc);

    let (is_permissive, is_disabled, m_sev_opt) =
        detect_selinux_mode_state(snap.as_ref(), diag.as_ref());
    let sel_override = snap.as_ref().and_then(|sn| {
        sn.active_overrides
            .iter()
            .find(|o| o.subsystem == "selinux")
    });

    // 1. SELinux Mode & Guardrail Status Card (NO SELINUX=disabled option)
    let status_box = GtkBox::new(Orientation::Vertical, 12);
    status_box.add_css_class("akrep-detail-box");

    let top_row = GtkBox::new(Orientation::Horizontal, 12);
    top_row.set_valign(Align::Center);
    let st_hdr = Label::new(Some(lang.pick("SELinux durumu", "SELinux status")));
    st_hdr.add_css_class("akrep-section-hdr");
    st_hdr.set_xalign(0.0);
    st_hdr.set_hexpand(true);
    top_row.append(&st_hdr);

    if let Some(m_sev) = m_sev_opt {
        top_row.append(&build_severity_indicator(m_sev, lang));
    }
    status_box.append(&top_row);

    if let Some(sn) = &snap {
        if let Some(sec) = sn
            .sections
            .iter()
            .find(|x| x.section == "system_protection" || x.section == "selinux")
        {
            for m in &sec.metrics {
                status_box.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    }

    let mode_box = GtkBox::new(Orientation::Vertical, 8);
    mode_box.add_css_class("akrep-diff-row");

    if is_permissive {
        let mode_top = GtkBox::new(Orientation::Horizontal, 12);
        mode_top.set_valign(Align::Center);
        let status_txt = if let Some(ov) = sel_override {
            format!("Permissive · {}", format_override_status_text(ov, lang))
        } else {
            lang.pick("Permissive etkin", "Permissive active")
                .to_string()
        };
        let ind = build_status_indicator("akrep-sev-attention", &status_txt, false);
        ind.set_hexpand(true);
        mode_top.append(&ind);

        let enforce_btn = Button::with_label(lang.pick(
            "Enforcing moduna dön",
            "Return to Enforcing",
        ));
        enforce_btn.add_css_class("akrep-primary-btn");
        let cmd_enf = cmd_tx.clone();
        enforce_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_enf,
                UiCommand::CreatePlan {
                    request: ActionRequest::SelinuxSetMode {
                        mode: SelinuxDesiredMode::Enforcing,
                        duration: OverrideDuration::UntilChanged,
                    },
                },
            );
        });
        mode_top.append(&enforce_btn);
        mode_box.append(&mode_top);

        let dur_row = GtkBox::new(Orientation::Horizontal, 8);
        dur_row.set_valign(Align::Center);
        let st_sd = Rc::clone(state);
        dur_row.append(&build_duration_selector(
            &PROTECTION_DURATION_CHOICES,
            selinux_duration_idx,
            lang,
            Rc::new(move |idx| {
                st_sd.borrow_mut().selinux_duration_idx = idx;
            }),
        ));

        let upd_btn = Button::with_label(lang.pick("Süreyi güncelle", "Update duration"));
        upd_btn.add_css_class("akrep-ghost-btn");
        upd_btn.set_valign(Align::Center);
        let st_upd = Rc::clone(state);
        let cmd_upd = cmd_tx.clone();
        upd_btn.connect_clicked(move |_| {
            let idx = st_upd.borrow().selinux_duration_idx;
            let duration = duration_from_choice_idx(&PROTECTION_DURATION_CHOICES, idx);
            dispatch_cmd(
                &cmd_upd,
                UiCommand::CreatePlan {
                    request: ActionRequest::SelinuxSetMode {
                        mode: SelinuxDesiredMode::Permissive,
                        duration,
                    },
                },
            );
        });
        dur_row.append(&upd_btn);
        mode_box.append(&dur_row);
    } else {
        let (dot_cls, dot_txt, dot_muted) = if is_disabled {
            ("akrep-sev-critical", lang.pick("Devre dışı", "Disabled"), false)
        } else if m_sev_opt.is_none() {
            (
                "akrep-sev-unavailable",
                lang.pick("Durum bekleniyor", "Loading status"),
                true,
            )
        } else {
            (
                "akrep-sev-protected",
                lang.pick("Enforcing etkin", "Enforcing active"),
                false,
            )
        };
        mode_box.append(&build_status_indicator(dot_cls, dot_txt, dot_muted));

        let dur_row = GtkBox::new(Orientation::Horizontal, 8);
        dur_row.set_valign(Align::Center);
        let st_sd = Rc::clone(state);
        dur_row.append(&build_duration_selector(
            &PROTECTION_DURATION_CHOICES,
            selinux_duration_idx,
            lang,
            Rc::new(move |idx| {
                st_sd.borrow_mut().selinux_duration_idx = idx;
            }),
        ));

        let perm_btn = Button::with_label(lang.pick(
            "Permissive moduna geç",
            "Switch to Permissive",
        ));
        perm_btn.add_css_class("akrep-ghost-btn");
        perm_btn.set_valign(Align::Center);
        let st_perm = Rc::clone(state);
        let cmd_perm = cmd_tx.clone();
        perm_btn.connect_clicked(move |_| {
            let idx = st_perm.borrow().selinux_duration_idx;
            let duration = duration_from_choice_idx(&PROTECTION_DURATION_CHOICES, idx);
            dispatch_cmd(
                &cmd_perm,
                UiCommand::CreatePlan {
                    request: ActionRequest::SelinuxSetMode {
                        mode: SelinuxDesiredMode::Permissive,
                        duration,
                    },
                },
            );
        });
        dur_row.append(&perm_btn);
        mode_box.append(&dur_row);
    }

    mode_box.append(&build_admin_note(lang));
    status_box.append(&mode_box);

    let guardrail_note = Label::new(Some(lang.pick(
        "SELinux tamamen kapatılamaz; geçici veya kalıcı Permissive modda dosya etiketleri korunur",
        "SELinux cannot be disabled; file labels remain intact in temporary or persistent Permissive mode",
    )));
    guardrail_note.add_css_class("akrep-meta");
    guardrail_note.set_xalign(0.0);
    guardrail_note.set_wrap(true);
    status_box.append(&guardrail_note);
    root.append(&status_box);

    // 2. Developer and Build Compatibility Card
    root.append(&build_dev_compat_card(state, cmd_tx, snap.as_ref(), lang));

    // 3. Boolean Table & AVC Diagnosis Trigger
    let bool_card = GtkBox::new(Orientation::Vertical, 12);
    bool_card.add_css_class("akrep-detail-box");

    let b_hdr = GtkBox::new(Orientation::Horizontal, 12);
    b_hdr.set_valign(Align::Center);
    let b_title = Label::new(Some(lang.pick("Politika anahtarları", "Policy booleans")));
    b_title.add_css_class("akrep-section-hdr");
    b_title.set_xalign(0.0);
    b_title.set_hexpand(true);
    b_hdr.append(&b_title);

    let diag_btn = Button::with_label(lang.pick("Tanıla", "Diagnose"));
    diag_btn.add_css_class(if is_permissive {
        "akrep-ghost-btn"
    } else {
        "akrep-primary-btn"
    });
    diag_btn.set_valign(Align::Center);
    let cmd_diag = cmd_tx.clone();
    diag_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_diag,
            UiCommand::DiagnoseSelinux {
                query: SelinuxDiagnoseQuery::default(),
            },
        );
    });
    b_hdr.append(&diag_btn);
    bool_card.append(&b_hdr);

    let filter_row = GtkBox::new(Orientation::Horizontal, 12);
    filter_row.set_valign(Align::Center);

    let search_entry = Entry::new();
    search_entry.add_css_class("akrep-entry");
    search_entry.set_width_chars(12);
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some(lang.pick(
        "Anahtar ara (virt_use_kvm, httpd, samba)",
        "Search booleans (virt_use_kvm, httpd, samba)",
    )));
    search_entry.set_text(&draft.boolean_search);
    filter_row.append(&search_entry);

    let pers_chk = CheckButton::with_label(lang.pick("Kalıcı uygula", "Apply permanently"));
    pers_chk.set_active(draft.persistent_toggle);
    let st_p = Rc::clone(state);
    pers_chk.connect_toggled(move |chk| {
        st_p.borrow_mut().selinux_draft.persistent_toggle = chk.is_active();
    });
    filter_row.append(&pers_chk);
    bool_card.append(&filter_row);

    bool_card.append(&build_admin_note(lang));

    let bool_list_box = GtkBox::new(Orientation::Vertical, 8);
    if let Some(d) = &diag {
        populate_boolean_rows(
            &bool_list_box,
            &d.booleans,
            &draft.boolean_search,
            state,
            cmd_tx,
            lang,
        );
        let booleans_clone = d.booleans.clone();
        let list_for_search = bool_list_box.clone();
        let st_search = Rc::clone(state);
        let cmd_search = cmd_tx.clone();
        search_entry.connect_changed(move |ent| {
            let txt = ent.text().to_string();
            st_search.borrow_mut().selinux_draft.boolean_search = txt.clone();
            populate_boolean_rows(
                &list_for_search,
                &booleans_clone,
                &txt,
                &st_search,
                &cmd_search,
                lang,
            );
        });
    } else {
        let st_search = Rc::clone(state);
        search_entry.connect_changed(move |ent| {
            st_search.borrow_mut().selinux_draft.boolean_search = ent.text().to_string();
        });
        let prompt = Label::new(Some(lang.pick(
            "SELinux anahtarlarını ve engelleme kayıtlarını yüklemek için Tanıla düğmesini kullanın",
            "Select Diagnose above to load SELinux booleans and grouped access denials",
        )));
        prompt.add_css_class("akrep-subtitle");
        prompt.set_xalign(0.0);
        prompt.set_wrap(true);
        bool_list_box.append(&prompt);
    }
    bool_card.append(&bool_list_box);
    root.append(&bool_card);

    // 3. Grouped AVC Denials Section
    let avc_card = GtkBox::new(Orientation::Vertical, 12);
    avc_card.add_css_class("akrep-detail-box");
    let avc_title = Label::new(Some(lang.pick("Engelleme kayıtları", "Access denials")));
    avc_title.add_css_class("akrep-section-hdr");
    avc_title.set_xalign(0.0);
    avc_card.append(&avc_title);

    match &diag {
        Some(d) if !d.avc_groups.is_empty() => {
            for grp in &d.avc_groups {
                let grow = GtkBox::new(Orientation::Vertical, 8);
                grow.add_css_class("akrep-diff-row");

                let gtop = GtkBox::new(Orientation::Horizontal, 12);
                gtop.set_valign(Align::Center);
                let app_lbl = Label::new(Some(&format!(
                    "{} ({} → {}:{})",
                    grp.app_comm, grp.source_domain, grp.target_type, grp.target_class
                )));
                app_lbl.add_css_class("akrep-card-title");
                app_lbl.set_xalign(0.0);
                app_lbl.set_wrap(true);
                app_lbl.set_wrap_mode(WrapMode::WordChar);
                app_lbl.set_hexpand(true);
                gtop.append(&app_lbl);

                gtop.append(&build_status_indicator(
                    risk_css_class(grp.severity),
                    &format!("{}×", grp.count),
                    false,
                ));
                grow.append(&gtop);

                let exp = Label::new(Some(lang.pick(&grp.explanation_tr, &grp.explanation_en)));
                exp.add_css_class("akrep-subtitle");
                exp.set_xalign(0.0);
                exp.set_wrap(true);
                exp.set_wrap_mode(WrapMode::WordChar);
                grow.append(&exp);

                match &grp.suggested_remediation_kind {
                    SuggestedSelinuxFix::Restorecon {
                        target_path,
                        recursive,
                        ..
                    } => {
                        let act_row = GtkBox::new(Orientation::Horizontal, 12);
                        act_row.set_valign(Align::Center);
                        let p_lbl = Label::new(Some(&format!("restorecon {}", target_path)));
                        p_lbl.add_css_class("akrep-mono");
                        p_lbl.set_xalign(0.0);
                        p_lbl.set_wrap(true);
                        p_lbl.set_wrap_mode(WrapMode::WordChar);
                        p_lbl.set_hexpand(true);
                        act_row.append(&p_lbl);

                        let fix_btn = Button::with_label(lang.pick("Bağlamı onar", "Repair context"));
                        fix_btn.add_css_class("akrep-ghost-btn");
                        let cmd_f = cmd_tx.clone();
                        let tp = target_path.clone();
                        let rec = *recursive;
                        fix_btn.connect_clicked(move |_| {
                            dispatch_cmd(
                                &cmd_f,
                                UiCommand::CreatePlan {
                                    request: ActionRequest::SelinuxRestorecon {
                                        target_path: tp.clone(),
                                        recursive: rec,
                                    },
                                },
                            );
                        });
                        act_row.append(&fix_btn);
                        grow.append(&act_row);
                    }
                    SuggestedSelinuxFix::SetBoolean {
                        boolean_name,
                        recommended_value,
                        ..
                    } => {
                        let act_row = GtkBox::new(Orientation::Horizontal, 12);
                        act_row.set_valign(Align::Center);
                        let b_lbl =
                            Label::new(Some(&format!("{} = {}", boolean_name, recommended_value)));
                        b_lbl.add_css_class("akrep-mono");
                        b_lbl.set_xalign(0.0);
                        b_lbl.set_wrap(true);
                        b_lbl.set_hexpand(true);
                        act_row.append(&b_lbl);

                        let fix_btn =
                            Button::with_label(lang.pick("Anahtarı uygula", "Apply boolean"));
                        fix_btn.add_css_class("akrep-ghost-btn");
                        let cmd_f = cmd_tx.clone();
                        let st_f = Rc::clone(state);
                        let bn = boolean_name.clone();
                        let rv = *recommended_value;
                        fix_btn.connect_clicked(move |_| {
                            let persistent = st_f.borrow().selinux_draft.persistent_toggle;
                            dispatch_cmd(
                                &cmd_f,
                                UiCommand::CreatePlan {
                                    request: ActionRequest::SelinuxSetBoolean {
                                        name: bn.clone(),
                                        value: rv,
                                        persistent,
                                    },
                                },
                            );
                        });
                        act_row.append(&fix_btn);
                        grow.append(&act_row);
                    }
                    SuggestedSelinuxFix::ManualPolicyReview {
                        guidance_tr,
                        guidance_en,
                    } => {
                        let gl = Label::new(Some(lang.pick(guidance_tr, guidance_en)));
                        gl.add_css_class("akrep-meta");
                        gl.set_xalign(0.0);
                        gl.set_wrap(true);
                        gl.set_wrap_mode(WrapMode::WordChar);
                        grow.append(&gl);
                    }
                }

                avc_card.append(&grow);
            }
        }
        Some(_) => {
            let clean = Label::new(Some(lang.pick(
                "Son denetim kayıtlarında engelleme bulunmadı",
                "No grouped access denials found in recent audit logs",
            )));
            clean.add_css_class("akrep-subtitle");
            clean.set_xalign(0.0);
            avc_card.append(&clean);
        }
        None => {
            let unl = Label::new(Some(lang.pick(
                "Engelleme kayıtları tanılama sonrasında burada listelenir",
                "Grouped access denials appear here after diagnosis",
            )));
            unl.add_css_class("akrep-subtitle");
            unl.set_xalign(0.0);
            unl.set_wrap(true);
            avc_card.append(&unl);
        }
    }
    root.append(&avc_card);

    // 4. File Context (restorecon) Inspector & Fixer
    let ctx_card = GtkBox::new(Orientation::Vertical, 12);
    ctx_card.add_css_class("akrep-detail-box");

    let ctx_title = Label::new(Some(lang.pick(
        "Dosya bağlamı onarımı",
        "File context repair",
    )));
    ctx_title.add_css_class("akrep-section-hdr");
    ctx_title.set_xalign(0.0);
    ctx_card.append(&ctx_title);

    let ctx_input_row = GtkBox::new(Orientation::Horizontal, 12);
    ctx_input_row.set_valign(Align::Center);
    let ctx_entry = Entry::new();
    ctx_entry.add_css_class("akrep-entry");
    ctx_entry.set_width_chars(12);
    ctx_entry.set_hexpand(true);
    ctx_entry.set_text(&draft.restorecon_path);
    let st_cp = Rc::clone(state);
    ctx_entry.connect_changed(move |ent| {
        st_cp.borrow_mut().selinux_draft.restorecon_path = ent.text().to_string();
    });
    ctx_input_row.append(&ctx_entry);

    let rec_ctx = CheckButton::with_label(lang.pick("Alt klasörler dahil", "Include subfolders"));
    rec_ctx.set_active(draft.restorecon_recursive);
    let st_cr = Rc::clone(state);
    rec_ctx.connect_toggled(move |chk| {
        st_cr.borrow_mut().selinux_draft.restorecon_recursive = chk.is_active();
    });
    ctx_input_row.append(&rec_ctx);
    ctx_card.append(&ctx_input_row);

    let ctx_btn_row = GtkBox::new(Orientation::Horizontal, 8);
    let inspect_ctx_btn = Button::with_label(lang.pick("Bağlamı incele", "Inspect context"));
    inspect_ctx_btn.add_css_class("akrep-ghost-btn");
    let cmd_ic = cmd_tx.clone();
    let st_ic = Rc::clone(state);
    inspect_ctx_btn.connect_clicked(move |_| {
        let d = st_ic.borrow().selinux_draft.clone();
        dispatch_cmd(
            &cmd_ic,
            UiCommand::DiagnoseSelinux {
                query: SelinuxDiagnoseQuery {
                    include_booleans: true,
                    include_avc: false,
                    boolean_filter: None,
                    inspect_path: Some(d.restorecon_path.trim().to_string()),
                    recursive: d.restorecon_recursive,
                    action: Some("inspect_path".to_string()),
                },
            },
        );
    });
    ctx_btn_row.append(&inspect_ctx_btn);

    let fix_ctx_btn = Button::with_label(lang.pick("Onarımı önizle", "Preview repair"));
    fix_ctx_btn.add_css_class("akrep-ghost-btn");
    let cmd_fc = cmd_tx.clone();
    let st_fc = Rc::clone(state);
    fix_ctx_btn.connect_clicked(move |_| {
        let d = st_fc.borrow().selinux_draft.clone();
        dispatch_cmd(
            &cmd_fc,
            UiCommand::CreatePlan {
                request: ActionRequest::SelinuxRestorecon {
                    target_path: d.restorecon_path.trim().to_string(),
                    recursive: d.restorecon_recursive,
                },
            },
        );
    });
    ctx_btn_row.append(&fix_ctx_btn);
    ctx_card.append(&ctx_btn_row);

    if let Some(d) = &diag {
        for c in d.current_contexts.iter().take(15) {
            ctx_card.append(&build_kv_row(&c.path, &c.context));
        }
        for p in d.restorecon_preview.iter().take(15) {
            ctx_card.append(&build_kv_row(
                &p.path,
                &format!("{} → {}", p.from_context, p.to_context),
            ));
        }
    }

    root.append(&ctx_card);
    root
}
