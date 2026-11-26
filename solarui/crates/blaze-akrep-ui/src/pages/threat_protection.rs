use crate::components::{
    build_admin_note, build_kv_row, build_severity_indicator, build_status_indicator,
    severity_css_class,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{
    ActionRequest, QuarantineMetadata, ScanJobState, ScanProgress, ScanRequest, ScanVerdict,
    ScannerEngineState, OPTIONAL_SCANNER_INSTALL_HINT,
};
use blaze_sentinel_core::{AkrepSeverity, SENTINEL_VERSION};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, Entry, FileDialog, HeaderBar,
    Image, Label, Orientation, ProgressBar, Window,
};
use std::cell::RefCell;
use std::rc::Rc;

fn scan_state_label(state: ScanJobState, lang: Lang) -> &'static str {
    match state {
        ScanJobState::Running => lang.pick("Taranıyor", "Scanning"),
        ScanJobState::Completed => lang.pick("Tamamlandı", "Completed"),
        ScanJobState::Cancelled => lang.pick("İptal edildi", "Cancelled"),
        ScanJobState::Failed => lang.pick("Başarısız", "Failed"),
    }
}

pub fn format_scan_progress_view(prog: &ScanProgress, lang: Lang) -> (f64, String, String) {
    let fraction = (prog.percent.min(100) as f64) / 100.0;
    let bar_text = format!(
        "{} · %{} ({}/{} {})",
        scan_state_label(prog.state, lang),
        prog.percent,
        prog.scanned_files,
        prog.total_files,
        lang.pick("dosya", "files")
    );
    let current_text = match &prog.current_path {
        Some(cur) => format!("{}: {}", lang.pick("Taranan", "Current"), cur),
        None => String::new(),
    };
    (fraction, bar_text, current_text)
}

fn present_vault_delete_confirm_dialog(
    parent: &ApplicationWindow,
    entry: &QuarantineMetadata,
    lang: Lang,
    cmd_tx: async_channel::Sender<UiCommand>,
) {
    let dlg = Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .title(lang.pick("Karantinadan kalıcı sil", "Delete from quarantine"))
        .build();
    dlg.add_css_class("akrep-modal");

    let hb = HeaderBar::new();
    hb.add_css_class("akrep-headerbar");
    dlg.set_titlebar(Some(&hb));

    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(20);
    root.set_margin_bottom(20);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let hdr = GtkBox::new(Orientation::Horizontal, 8);
    hdr.set_valign(Align::Center);
    let icon = Image::from_icon_name("dialog-warning-symbolic");
    icon.set_pixel_size(16);
    icon.add_css_class("akrep-icon-secondary");
    hdr.append(&icon);
    let title = Label::new(Some(lang.pick(
        "Dosya kalıcı olarak silinsin mi?",
        "Permanently delete this file?",
    )));
    title.add_css_class("akrep-section-hdr");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_hexpand(true);
    hdr.append(&title);
    root.append(&hdr);

    let warn = Label::new(Some(lang.pick(
        "Bu işlem geri alınamaz. Dosya ve meta verileri karantina kasasından kalıcı olarak silinir.",
        "This action cannot be undone. The file and its metadata will be permanently removed from quarantine.",
    )));
    warn.add_css_class("akrep-toast-err");
    warn.set_xalign(0.0);
    warn.set_wrap(true);
    warn.set_wrap_mode(WrapMode::WordChar);
    root.append(&warn);

    let meta = GtkBox::new(Orientation::Vertical, 8);
    meta.add_css_class("akrep-detail-box");
    meta.append(&build_kv_row(
        lang.pick("Özgün yol", "Original path"),
        &entry.original_path,
    ));
    meta.append(&build_kv_row(
        lang.pick("Tehdit adı", "Threat name"),
        &entry.threat_name,
    ));
    meta.append(&build_kv_row(
        lang.pick("Karantina kimliği", "Quarantine ID"),
        &entry.quarantine_id,
    ));
    meta.append(&build_kv_row("SHA-256", &entry.sha256));
    root.append(&meta);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);

    let cancel_btn = Button::with_label(lang.pick("İptal", "Cancel"));
    cancel_btn.add_css_class("akrep-ghost-btn");
    let d_cancel = dlg.clone();
    cancel_btn.connect_clicked(move |_| d_cancel.close());
    actions.append(&cancel_btn);

    let confirm_btn = Button::with_label(lang.pick("Kalıcı olarak sil", "Delete permanently"));
    confirm_btn.add_css_class("akrep-danger-solid-btn");
    let qid = entry.quarantine_id.clone();
    let d_ok = dlg.clone();
    confirm_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_tx,
            UiCommand::CreatePlan {
                request: ActionRequest::DeleteQuarantine {
                    quarantine_id: qid.clone(),
                    confirm_permanent_delete: true,
                },
            },
        );
        d_ok.close();
    });
    actions.append(&confirm_btn);

    root.append(&actions);
    dlg.set_child(Some(&root));
    dlg.present();
}

fn present_scan_detection_delete_dialog(
    parent: &ApplicationWindow,
    file_path: &str,
    threat_name: &str,
    lang: Lang,
    cmd_tx: async_channel::Sender<UiCommand>,
) {
    let dlg = Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .title(lang.pick("Karantinaya al ve sil", "Quarantine before deleting"))
        .build();
    dlg.add_css_class("akrep-modal");

    let hb = HeaderBar::new();
    hb.add_css_class("akrep-headerbar");
    dlg.set_titlebar(Some(&hb));

    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(20);
    root.set_margin_bottom(20);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let msg = Label::new(Some(lang.pick(
        "Dosyalar doğrudan silinmek yerine önce karantina kasasına alınır. Ardından karantina kasasından kalıcı olarak silebilirsiniz.",
        "Files are moved to the quarantine vault first instead of being removed directly. You can then delete them permanently from the vault.",
    )));
    msg.add_css_class("akrep-subtitle");
    msg.set_xalign(0.0);
    msg.set_wrap(true);
    msg.set_wrap_mode(WrapMode::WordChar);
    root.append(&msg);

    let meta = GtkBox::new(Orientation::Vertical, 8);
    meta.add_css_class("akrep-detail-box");
    meta.append(&build_kv_row(lang.pick("Dosya", "File path"), file_path));
    meta.append(&build_kv_row(
        lang.pick("Tehdit imzası", "Threat signature"),
        threat_name,
    ));
    root.append(&meta);

    root.append(&build_admin_note(lang));

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_halign(Align::End);

    let cancel_btn = Button::with_label(lang.pick("İptal", "Cancel"));
    cancel_btn.add_css_class("akrep-ghost-btn");
    let d_cancel = dlg.clone();
    cancel_btn.connect_clicked(move |_| d_cancel.close());
    actions.append(&cancel_btn);

    let q_btn = Button::with_label(lang.pick("Karantinaya al", "Move to quarantine"));
    q_btn.add_css_class("akrep-danger-solid-btn");
    let fp = file_path.to_string();
    let tn = threat_name.to_string();
    let d_ok = dlg.clone();
    q_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_tx,
            UiCommand::CreatePlan {
                request: ActionRequest::QuarantineFile {
                    path: fp.clone(),
                    threat_name: tn.clone(),
                },
            },
        );
        d_ok.close();
    });
    actions.append(&q_btn);

    root.append(&actions);
    dlg.set_child(Some(&root));
    dlg.present();
}

pub fn build_threat_protection_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    window: &ApplicationWindow,
) -> GtkBox {
    let (lang, snap, scanner, active_scan, quarantine, draft) = {
        let mut st = state.borrow_mut();
        st.live_scan_widgets = None;
        st.live_scan_path_entry = None;
        (
            st.lang,
            st.snapshot.clone(),
            st.scanner_report.clone(),
            st.active_scan.clone(),
            st.quarantine.clone(),
            st.threat_draft.clone(),
        )
    };

    let root = GtkBox::new(Orientation::Vertical, 20);

    let page_desc = Label::new(Some(lang.pick(
        "Çalışma zamanı bütünlüğü, isteğe bağlı virüs taraması ve karantina yönetimi",
        "Runtime integrity, on-demand malware scanning and quarantine management",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    root.append(&page_desc);

    // 1. Sentinel Runtime Integrity Card
    let sentinel_box = GtkBox::new(Orientation::Vertical, 12);
    sentinel_box.add_css_class("akrep-detail-box");

    let s_hdr = GtkBox::new(Orientation::Horizontal, 12);
    s_hdr.set_valign(Align::Center);
    let s_title = Label::new(Some(lang.pick(
        "Sentinel çalışma zamanı bütünlüğü",
        "Sentinel runtime integrity",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    s_title.set_wrap(true);
    s_title.set_hexpand(true);
    s_hdr.append(&s_title);

    let integrity_sev = match &snap {
        Some(sn) if sn.posture.integrity_ok => AkrepSeverity::Protected,
        Some(_) => AkrepSeverity::Critical,
        None => AkrepSeverity::Unknown,
    };
    s_hdr.append(&build_severity_indicator(integrity_sev, lang));
    sentinel_box.append(&s_hdr);

    if let Some(sn) = &snap {
        sentinel_box.append(&build_kv_row(
            lang.pick("Çekirdek sürümü", "Core engine"),
            &format!(
                "v{} ({}/{} {})",
                SENTINEL_VERSION,
                sn.posture.passed,
                sn.posture.total,
                lang.pick("doğrulandı", "verified")
            ),
        ));
        if let Some(sec) = sn
            .sections
            .iter()
            .find(|x| x.section == "threat_protection" || x.section == "threat_integrity")
        {
            for m in &sec.metrics {
                sentinel_box.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    } else {
        let info = Label::new(Some(lang.pick(
            "Sentinel bilgileri güvenlik servisi çevrimiçi olduğunda görünür",
            "Sentinel details appear when the security service is online",
        )));
        info.add_css_class("akrep-subtitle");
        info.set_xalign(0.0);
        info.set_wrap(true);
        sentinel_box.append(&info);
    }
    root.append(&sentinel_box);

    // 2. ClamAV Engine & Signature DB Health Card
    let engine_box = GtkBox::new(Orientation::Vertical, 12);
    engine_box.add_css_class("akrep-detail-box");

    let e_hdr = GtkBox::new(Orientation::Horizontal, 12);
    e_hdr.set_valign(Align::Center);
    let e_title = Label::new(Some(lang.pick(
        "ClamAV tarama motoru",
        "ClamAV scanning engine",
    )));
    e_title.add_css_class("akrep-section-hdr");
    e_title.set_xalign(0.0);
    e_title.set_wrap(true);
    e_title.set_hexpand(true);
    e_hdr.append(&e_title);

    let eng_sev = match &scanner {
        Some(rep) => match &rep.engine_state {
            ScannerEngineState::Unavailable { .. } => AkrepSeverity::Unavailable,
            _ => rep.severity,
        },
        None => AkrepSeverity::Unavailable,
    };
    e_hdr.append(&build_severity_indicator(eng_sev, lang));
    engine_box.append(&e_hdr);

    let show_install_btn = match &scanner {
        Some(rep) => matches!(rep.engine_state, ScannerEngineState::Unavailable { .. }),
        None => true,
    };

    if let Some(rep) = &scanner {
        let mode_desc = match &rep.engine_state {
            ScannerEngineState::ClamdSocket { socket_path } => {
                format!("clamd ({})", socket_path)
            }
            ScannerEngineState::ClamscanCli { binary_path } => {
                format!("clamscan ({})", binary_path)
            }
            ScannerEngineState::Unavailable { install_hint } => format!(
                "{} ({})",
                lang.pick("Kurulu değil", "Not installed"),
                install_hint
            ),
        };
        engine_box.append(&build_kv_row(
            lang.pick("Tarama motoru", "Scanner engine"),
            &mode_desc,
        ));
        engine_box.append(&build_kv_row(
            lang.pick("İmza veritabanı", "Signature database"),
            &rep.signature_db.detail,
        ));
        engine_box.append(&build_kv_row(
            lang.pick("Arka plan taraması", "Background scanning"),
            lang.pick(
                "Kapalı (yalnızca talep üzerine tarama)",
                "Off (on-demand scanning only)",
            ),
        ));
    } else {
        let note = Label::new(Some(&format!(
            "{} ({})",
            lang.pick(
                "ClamAV isteğe bağlıdır; dosya ve klasör taraması için buradan kurabilirsiniz",
                "ClamAV is optional; install it here for on-demand file and folder scans"
            ),
            OPTIONAL_SCANNER_INSTALL_HINT
        )));
        note.add_css_class("akrep-subtitle");
        note.set_xalign(0.0);
        note.set_wrap(true);
        note.set_wrap_mode(WrapMode::WordChar);
        engine_box.append(&note);
    }

    let eng_actions = GtkBox::new(Orientation::Horizontal, 8);
    eng_actions.set_valign(Align::Center);
    if show_install_btn {
        let inst_btn = Button::with_label(lang.pick("ClamAV kur", "Install ClamAV"));
        inst_btn.add_css_class("akrep-ghost-btn");
        let cmd_inst = cmd_tx.clone();
        inst_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_inst,
                UiCommand::CreatePlan {
                    request: ActionRequest::InstallOptionalScanner,
                },
            );
        });
        eng_actions.append(&inst_btn);
    }

    let inspect_sc_btn = Button::with_label(lang.pick("Durumu denetle", "Check engine"));
    inspect_sc_btn.add_css_class("akrep-ghost-btn");
    let cmd_sc = cmd_tx.clone();
    inspect_sc_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_sc, UiCommand::InspectScanner);
    });
    eng_actions.append(&inspect_sc_btn);
    engine_box.append(&eng_actions);
    engine_box.append(&build_admin_note(lang));
    root.append(&engine_box);

    // 3. On-Demand File / Folder Scan Trigger & In-Place Live Progress Card
    let scan_box = GtkBox::new(Orientation::Vertical, 12);
    scan_box.add_css_class("akrep-detail-box");

    let sc_title = Label::new(Some(lang.pick(
        "Dosya ve klasör taraması",
        "File and folder scan",
    )));
    sc_title.add_css_class("akrep-section-hdr");
    sc_title.set_xalign(0.0);
    scan_box.append(&sc_title);

    let input_row = GtkBox::new(Orientation::Horizontal, 8);
    let path_entry = Entry::new();
    path_entry.add_css_class("akrep-entry");
    path_entry.set_width_chars(12);
    path_entry.set_hexpand(true);
    path_entry.set_placeholder_text(Some("/tmp/sample.bin"));
    path_entry.set_text(&draft.scan_path);

    let st_path = Rc::clone(state);
    path_entry.connect_changed(move |ent| {
        st_path.borrow_mut().threat_draft.scan_path = ent.text().to_string();
    });
    state.borrow_mut().live_scan_path_entry = Some(path_entry.clone());
    input_row.append(&path_entry);

    let file_btn = Button::with_label(lang.pick("Dosya seç", "Select file"));
    file_btn.add_css_class("akrep-ghost-btn");
    let win_file = window.clone();
    let st_file = Rc::clone(state);
    file_btn.connect_clicked(move |_| {
        let dlg = FileDialog::builder()
            .title(lang.pick("Taranacak dosyayı seç", "Select file to scan"))
            .modal(true)
            .build();
        let st_cb = Rc::clone(&st_file);
        dlg.open(Some(&win_file), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(p) = file.path() {
                    let chosen = p.to_string_lossy().to_string();
                    let live_ent = {
                        let mut s = st_cb.borrow_mut();
                        s.threat_draft.scan_path = chosen.clone();
                        s.live_scan_path_entry.clone()
                    };
                    if let Some(ent) = live_ent {
                        ent.set_text(&chosen);
                    }
                }
            }
        });
    });
    input_row.append(&file_btn);

    let folder_btn = Button::with_label(lang.pick("Klasör seç", "Select folder"));
    folder_btn.add_css_class("akrep-ghost-btn");
    let win_dir = window.clone();
    let st_dir = Rc::clone(state);
    folder_btn.connect_clicked(move |_| {
        let dlg = FileDialog::builder()
            .title(lang.pick("Taranacak klasörü seç", "Select folder to scan"))
            .modal(true)
            .build();
        let st_cb = Rc::clone(&st_dir);
        dlg.select_folder(Some(&win_dir), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(folder) = res {
                if let Some(p) = folder.path() {
                    let chosen = p.to_string_lossy().to_string();
                    let live_ent = {
                        let mut s = st_cb.borrow_mut();
                        s.threat_draft.scan_path = chosen.clone();
                        s.live_scan_path_entry.clone()
                    };
                    if let Some(ent) = live_ent {
                        ent.set_text(&chosen);
                    }
                }
            }
        });
    });
    input_row.append(&folder_btn);
    scan_box.append(&input_row);

    let opt_row = GtkBox::new(Orientation::Horizontal, 12);
    opt_row.set_valign(Align::Center);
    let rec_chk = CheckButton::with_label(lang.pick("Alt klasörleri tara", "Scan subfolders"));
    rec_chk.set_active(draft.recursive);
    let st_rec = Rc::clone(state);
    rec_chk.connect_toggled(move |chk| {
        st_rec.borrow_mut().threat_draft.recursive = chk.is_active();
    });
    opt_row.append(&rec_chk);

    let tmp_preset = Button::with_label("/tmp");
    tmp_preset.add_css_class("akrep-ghost-btn");
    let ent_tmp = path_entry.clone();
    tmp_preset.connect_clicked(move |_| ent_tmp.set_text("/tmp"));
    opt_row.append(&tmp_preset);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    opt_row.append(&spacer);

    let start_btn = Button::with_label(lang.pick("Tara", "Scan"));
    start_btn.add_css_class("akrep-primary-btn");
    let cmd_start = cmd_tx.clone();
    let st_start = Rc::clone(state);
    start_btn.connect_clicked(move |_| {
        let d = st_start.borrow().threat_draft.clone();
        dispatch_cmd(
            &cmd_start,
            UiCommand::StartScan {
                request: ScanRequest {
                    target_path: d.scan_path.trim().to_string(),
                    recursive: d.recursive,
                    socket_override: None,
                    synchronous: false,
                },
            },
        );
    });
    opt_row.append(&start_btn);
    scan_box.append(&opt_row);

    if let Some(prog) = &active_scan {
        let (frac, bar_txt, cur_txt) = format_scan_progress_view(prog, lang);
        let pbar = ProgressBar::new();
        pbar.set_show_text(true);
        pbar.set_fraction(frac);
        pbar.set_text(Some(&bar_txt));
        scan_box.append(&pbar);

        let cur_lbl = Label::new(Some(&cur_txt));
        cur_lbl.add_css_class("akrep-mono");
        cur_lbl.set_xalign(0.0);
        cur_lbl.set_wrap(true);
        cur_lbl.set_wrap_mode(WrapMode::WordChar);
        cur_lbl.set_visible(!cur_txt.is_empty());
        scan_box.append(&cur_lbl);

        if prog.state == ScanJobState::Running {
            state.borrow_mut().live_scan_widgets = Some((pbar, cur_lbl));

            let cancel_btn = Button::with_label(lang.pick("Taramayı durdur", "Stop scan"));
            cancel_btn.add_css_class("akrep-danger-btn");
            cancel_btn.set_halign(Align::Start);
            let cmd_cancel = cmd_tx.clone();
            let jid = prog.job_id.clone();
            cancel_btn.connect_clicked(move |_| {
                dispatch_cmd(
                    &cmd_cancel,
                    UiCommand::CancelScan {
                        job_id: jid.clone(),
                    },
                );
            });
            scan_box.append(&cancel_btn);
        }

        if let Some(err) = &prog.error_message {
            let el = Label::new(Some(err));
            el.add_css_class("akrep-toast-err");
            el.set_xalign(0.0);
            el.set_wrap(true);
            el.set_wrap_mode(WrapMode::WordChar);
            scan_box.append(&el);
        }

        if !prog.findings.is_empty() {
            let res_hdr = Label::new(Some(&format!(
                "{} ({})",
                lang.pick("Tarama bulguları", "Scan findings"),
                prog.findings.len()
            )));
            res_hdr.add_css_class("akrep-card-title");
            res_hdr.set_xalign(0.0);
            scan_box.append(&res_hdr);

            for finding in &prog.findings {
                let f_row = GtkBox::new(Orientation::Horizontal, 12);
                f_row.add_css_class("akrep-diff-row");
                f_row.set_valign(Align::Center);

                let (v_lbl, v_sev, actionable) = match &finding.verdict {
                    ScanVerdict::Infected { signature } => (
                        format!("{}: {}", lang.pick("Zararlı", "Infected"), signature),
                        AkrepSeverity::Critical,
                        true,
                    ),
                    ScanVerdict::Suspicious { reason } => (
                        format!("{}: {}", lang.pick("Şüpheli", "Suspicious"), reason),
                        AkrepSeverity::Attention,
                        true,
                    ),
                    ScanVerdict::Unknown { reason } => (
                        format!("{}: {}", lang.pick("Bilinmiyor", "Unknown"), reason),
                        AkrepSeverity::Unknown,
                        false,
                    ),
                    ScanVerdict::Clean => (
                        lang.pick("Temiz", "Clean").to_string(),
                        AkrepSeverity::Protected,
                        false,
                    ),
                };

                let info_col = GtkBox::new(Orientation::Vertical, 4);
                info_col.set_hexpand(true);
                info_col.append(&build_status_indicator(
                    severity_css_class(v_sev),
                    &v_lbl,
                    matches!(v_sev, AkrepSeverity::Unknown),
                ));

                let pl = Label::new(Some(&finding.path));
                pl.add_css_class("akrep-mono");
                pl.set_xalign(0.0);
                pl.set_wrap(true);
                pl.set_wrap_mode(WrapMode::WordChar);
                info_col.append(&pl);
                f_row.append(&info_col);

                if actionable {
                    let tname = finding
                        .threat_name
                        .clone()
                        .unwrap_or_else(|| "Detected.Threat".to_string());

                    let q_btn = Button::with_label(lang.pick("Karantinaya al", "Quarantine"));
                    q_btn.add_css_class("akrep-ghost-btn");
                    q_btn.set_valign(Align::Center);
                    let cmd_q = cmd_tx.clone();
                    let fpath_q = finding.path.clone();
                    let tname_q = tname.clone();
                    q_btn.connect_clicked(move |_| {
                        dispatch_cmd(
                            &cmd_q,
                            UiCommand::CreatePlan {
                                request: ActionRequest::QuarantineFile {
                                    path: fpath_q.clone(),
                                    threat_name: tname_q.clone(),
                                },
                            },
                        );
                    });
                    f_row.append(&q_btn);

                    let del_det_btn = Button::with_label(lang.pick("Sil", "Delete"));
                    del_det_btn.add_css_class("akrep-danger-btn");
                    del_det_btn.set_valign(Align::Center);
                    let win_det = window.clone();
                    let cmd_det = cmd_tx.clone();
                    let fpath_d = finding.path.clone();
                    let tname_d = tname;
                    del_det_btn.connect_clicked(move |_| {
                        present_scan_detection_delete_dialog(
                            &win_det,
                            &fpath_d,
                            &tname_d,
                            lang,
                            cmd_det.clone(),
                        );
                    });
                    f_row.append(&del_det_btn);
                }

                scan_box.append(&f_row);
            }
        } else if prog.state == ScanJobState::Completed {
            let clean_lbl = Label::new(Some(lang.pick(
                "Tarama tamamlandı; tehdit bulunmadı",
                "Scan completed; no threats detected",
            )));
            clean_lbl.add_css_class("akrep-subtitle");
            clean_lbl.set_xalign(0.0);
            clean_lbl.set_wrap(true);
            scan_box.append(&clean_lbl);
        }
    }
    root.append(&scan_box);

    // 4. Quarantine Vault Card
    let vault_box = GtkBox::new(Orientation::Vertical, 12);
    vault_box.add_css_class("akrep-detail-box");

    let v_hdr = GtkBox::new(Orientation::Horizontal, 12);
    v_hdr.set_valign(Align::Center);
    let v_title = Label::new(Some(lang.pick("Karantina kasası", "Quarantine vault")));
    v_title.add_css_class("akrep-section-hdr");
    v_title.set_xalign(0.0);
    v_title.set_wrap(true);
    v_title.set_hexpand(true);
    v_hdr.append(&v_title);

    let load_v_btn = Button::with_label(lang.pick("Kasayı yükle", "Load vault"));
    load_v_btn.add_css_class("akrep-ghost-btn");
    load_v_btn.set_valign(Align::Center);
    let cmd_lv = cmd_tx.clone();
    load_v_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_lv, UiCommand::ListQuarantine);
    });
    v_hdr.append(&load_v_btn);
    vault_box.append(&v_hdr);

    match &quarantine {
        Some(inv) if !inv.entries.is_empty() => {
            for entry in &inv.entries {
                let erow = GtkBox::new(Orientation::Horizontal, 12);
                erow.add_css_class("akrep-diff-row");
                erow.set_valign(Align::Center);

                let info_col = GtkBox::new(Orientation::Vertical, 4);
                info_col.set_hexpand(true);
                let p_lbl = Label::new(Some(&format!(
                    "{} · {}",
                    entry.original_path, entry.threat_name
                )));
                p_lbl.add_css_class("akrep-card-title");
                p_lbl.set_xalign(0.0);
                p_lbl.set_wrap(true);
                p_lbl.set_wrap_mode(WrapMode::WordChar);
                info_col.append(&p_lbl);

                let meta_lbl = Label::new(Some(&format!(
                    "SHA-256: {}... · {} B · {}",
                    entry.sha256.chars().take(16).collect::<String>(),
                    entry.size_bytes,
                    if entry.restored {
                        lang.pick("Geri yüklendi", "Restored")
                    } else {
                        lang.pick("Karantinada", "Isolated")
                    }
                )));
                meta_lbl.add_css_class("akrep-mono");
                meta_lbl.set_xalign(0.0);
                meta_lbl.set_wrap(true);
                meta_lbl.set_wrap_mode(WrapMode::WordChar);
                info_col.append(&meta_lbl);
                erow.append(&info_col);

                if !entry.restored {
                    let res_btn = Button::with_label(lang.pick("Geri yükle", "Restore"));
                    res_btn.add_css_class("akrep-ghost-btn");
                    res_btn.set_valign(Align::Center);
                    let cmd_r = cmd_tx.clone();
                    let qid_r = entry.quarantine_id.clone();
                    res_btn.connect_clicked(move |_| {
                        dispatch_cmd(
                            &cmd_r,
                            UiCommand::CreatePlan {
                                request: ActionRequest::RestoreQuarantine {
                                    quarantine_id: qid_r.clone(),
                                },
                            },
                        );
                    });
                    erow.append(&res_btn);
                }

                let del_btn = Button::with_label(lang.pick("Sil", "Delete"));
                del_btn.add_css_class("akrep-danger-btn");
                del_btn.set_valign(Align::Center);
                let win_del = window.clone();
                let cmd_d = cmd_tx.clone();
                let entry_clone = entry.clone();
                del_btn.connect_clicked(move |_| {
                    present_vault_delete_confirm_dialog(
                        &win_del,
                        &entry_clone,
                        lang,
                        cmd_d.clone(),
                    );
                });
                erow.append(&del_btn);

                vault_box.append(&erow);
            }
        }
        Some(_) => {
            let empty_lbl = Label::new(Some(lang.pick(
                "Karantina kasası boş",
                "Quarantine vault is empty",
            )));
            empty_lbl.add_css_class("akrep-subtitle");
            empty_lbl.set_xalign(0.0);
            vault_box.append(&empty_lbl);
        }
        None => {
            let hint_lbl = Label::new(Some(lang.pick(
                "İzole edilen dosyaları görmek için kasayı yükleyin",
                "Load the vault to inspect isolated files",
            )));
            hint_lbl.add_css_class("akrep-subtitle");
            hint_lbl.set_xalign(0.0);
            hint_lbl.set_wrap(true);
            vault_box.append(&hint_lbl);
        }
    }
    vault_box.append(&build_admin_note(lang));
    root.append(&vault_box);

    root
}
