use crate::components::{build_kv_row, severity_css_class};
use crate::dbus_client::UiCommand;
use crate::state::AppUiState;
use blaze_akrep_common::{
    ActionRequest, ScanJobState, ScanRequest, ScanVerdict, ScannerEngineState,
    OPTIONAL_SCANNER_INSTALL_HINT,
};
use blaze_sentinel_core::{AkrepSeverity, SENTINEL_VERSION};
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, CheckButton, Entry, FileDialog, Label,
    Orientation, ProgressBar,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_threat_protection_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    window: &ApplicationWindow,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let scanner = st.scanner_report.clone();
    let active_scan = st.active_scan.clone();
    let quarantine = st.quarantine.clone();
    let draft = st.threat_draft.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let page_hdr = Label::new(Some(lang.pick(
        "Tehdit Koruması, AV Tarama ve Zırhlı Karantina",
        "Threat Protection, On-Demand AV & Armored Quarantine",
    )));
    page_hdr.add_css_class("akrep-title");
    page_hdr.set_xalign(0.0);
    root.append(&page_hdr);

    // 1. Sentinel Runtime Integrity Card
    let sentinel_box = GtkBox::new(Orientation::Vertical, 8);
    sentinel_box.add_css_class("akrep-detail-box");

    let s_hdr = GtkBox::new(Orientation::Horizontal, 8);
    let s_title = Label::new(Some(lang.pick(
        "Sentinel Çalışma Zamanı Bütünlüğü",
        "Sentinel Runtime Integrity",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    s_title.set_hexpand(true);
    s_hdr.append(&s_title);

    let integrity_sev = match &snap {
        Some(sn) if sn.posture.integrity_ok => AkrepSeverity::Protected,
        Some(_) => AkrepSeverity::Critical,
        None => AkrepSeverity::Unknown,
    };
    let s_badge = Label::new(Some(match lang {
        crate::i18n::Lang::Tr => integrity_sev.label_tr(),
        crate::i18n::Lang::En => integrity_sev.label_en(),
    }));
    s_badge.add_css_class("akrep-badge");
    s_badge.add_css_class(severity_css_class(integrity_sev));
    s_hdr.append(&s_badge);
    sentinel_box.append(&s_hdr);

    if let Some(sn) = &snap {
        sentinel_box.append(&build_kv_row(
            lang.pick(
                "Çekirdek Sürümü ve Kontroller",
                "Sentinel Engine & Controls",
            ),
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
            "Sentinel telemetrisi akrepd çevrimiçi olduğunda görüntülenir.",
            "Sentinel telemetry appears when akrepd is online.",
        )));
        info.add_css_class("akrep-subtitle");
        info.set_xalign(0.0);
        sentinel_box.append(&info);
    }
    root.append(&sentinel_box);

    // 2. ClamAV Engine & Signature DB Health Card
    let engine_box = GtkBox::new(Orientation::Vertical, 8);
    engine_box.add_css_class("akrep-detail-box");

    let e_hdr = GtkBox::new(Orientation::Horizontal, 8);
    let e_title = Label::new(Some(lang.pick(
        "İsteğe Bağlı ClamAV Tarama Motoru & İmza Veritabanı",
        "Optional ClamAV Scanner Engine & Signature Database",
    )));
    e_title.add_css_class("akrep-section-hdr");
    e_title.set_xalign(0.0);
    e_title.set_hexpand(true);
    e_hdr.append(&e_title);

    let (eng_sev, eng_badge_txt) = match &scanner {
        Some(rep) => match &rep.engine_state {
            ScannerEngineState::Unavailable { .. } => (
                AkrepSeverity::Unavailable,
                lang.pick(
                    "Kullanılamıyor (İsteğe Bağlı Motor)",
                    "Unavailable (Optional Engine)",
                )
                .to_string(),
            ),
            _ => (
                rep.severity,
                match lang {
                    crate::i18n::Lang::Tr => rep.severity.label_tr().to_string(),
                    crate::i18n::Lang::En => rep.severity.label_en().to_string(),
                },
            ),
        },
        None => (
            AkrepSeverity::Unavailable,
            lang.pick(
                "Kullanılamıyor (İsteğe Bağlı Motor)",
                "Unavailable (Optional Engine)",
            )
            .to_string(),
        ),
    };

    let e_badge = Label::new(Some(&eng_badge_txt));
    e_badge.add_css_class("akrep-badge");
    e_badge.add_css_class(severity_css_class(eng_sev));
    e_hdr.append(&e_badge);

    let inspect_sc_btn =
        Button::with_label(lang.pick("Motor Durumunu Denetle (Polkit)", "Inspect Engine (Polkit)"));
    inspect_sc_btn.add_css_class("akrep-ghost-btn");
    let cmd_sc = cmd_tx.clone();
    inspect_sc_btn.connect_clicked(move |_| {
        let _ = cmd_sc.send_blocking(UiCommand::InspectScanner);
    });
    e_hdr.append(&inspect_sc_btn);
    engine_box.append(&e_hdr);

    let show_install_btn = match &scanner {
        Some(rep) => matches!(rep.engine_state, ScannerEngineState::Unavailable { .. }),
        None => true,
    };

    if let Some(rep) = &scanner {
        let mode_desc = match &rep.engine_state {
            ScannerEngineState::ClamdSocket { socket_path } => {
                format!("clamd Unix Domain Socket ({})", socket_path)
            }
            ScannerEngineState::ClamscanCli { binary_path } => {
                format!("clamscan CLI ({})", binary_path)
            }
            ScannerEngineState::Unavailable { install_hint } => format!(
                "{} ({})",
                lang.pick(
                    "Kurulu Değil — Talep üzerine tarama için isteğe bağlı",
                    "Not Installed — Optional on-demand scanner"
                ),
                install_hint
            ),
        };
        engine_box.append(&build_kv_row(
            lang.pick("Tarama Motoru Uç Noktası", "Scanner Endpoint"),
            &mode_desc,
        ));
        engine_box.append(&build_kv_row(
            lang.pick("İmza Veritabanı Durumu", "Signature DB Health"),
            &rep.signature_db.detail,
        ));
        engine_box.append(&build_kv_row(
            lang.pick(
                "Gerçek Zamanlı Arka Plan Taraması",
                "Background Fanotify Scan",
            ),
            lang.pick(
                "Kapalı (Yalnızca kullanıcı talebiyle tarama — sıfır boşta CPU yükü)",
                "Disabled (Strictly on-demand — zero idle CPU overhead)",
            ),
        ));
    } else {
        let note = Label::new(Some(&format!(
            "{} ({}).",
            lang.pick(
                "ClamAV çekirdek ISO imajında zorunlu değildir; talep üzerine dosya/klasör taraması için tek tıkla kurulabilir",
                "ClamAV is optional to keep the Live ISO lean; you can install it with 1 click for on-demand scanning"
            ),
            OPTIONAL_SCANNER_INSTALL_HINT
        )));
        note.add_css_class("akrep-subtitle");
        note.set_xalign(0.0);
        note.set_wrap(true);
        engine_box.append(&note);
    }

    if show_install_btn {
        let inst_row = GtkBox::new(Orientation::Horizontal, 8);
        let inst_btn = Button::with_label(lang.pick(
            "ClamAV Tarama Motorunu Kur (Polkit)",
            "Install ClamAV (Polkit)",
        ));
        inst_btn.add_css_class("akrep-primary-btn");
        let cmd_inst = cmd_tx.clone();
        inst_btn.connect_clicked(move |_| {
            let _ = cmd_inst.send_blocking(UiCommand::CreatePlan {
                request: ActionRequest::InstallOptionalScanner,
            });
        });
        inst_row.append(&inst_btn);
        engine_box.append(&inst_row);
    }
    root.append(&engine_box);

    // 3. On-Demand File / Folder Scan Trigger & Progress Card
    let scan_box = GtkBox::new(Orientation::Vertical, 10);
    scan_box.add_css_class("akrep-detail-box");

    let sc_title = Label::new(Some(lang.pick(
        "Dosya / Klasör Tehdit Taraması",
        "On-Demand File / Folder Scan",
    )));
    sc_title.add_css_class("akrep-section-hdr");
    sc_title.set_xalign(0.0);
    scan_box.append(&sc_title);

    let input_row = GtkBox::new(Orientation::Horizontal, 8);
    let path_entry = Entry::new();
    path_entry.add_css_class("akrep-entry");
    path_entry.set_hexpand(true);
    path_entry.set_placeholder_text(Some("/tmp/sample.bin"));
    path_entry.set_text(&draft.scan_path);

    let st_path = Rc::clone(state);
    path_entry.connect_changed(move |ent| {
        st_path.borrow_mut().threat_draft.scan_path = ent.text().to_string();
    });
    input_row.append(&path_entry);

    let file_btn = Button::with_label(lang.pick("Dosya Seç...", "Select File..."));
    file_btn.add_css_class("akrep-ghost-btn");
    let win_file = window.clone();
    let ent_file = path_entry.clone();
    file_btn.connect_clicked(move |_| {
        let dlg = FileDialog::builder()
            .title("Select File to Scan")
            .modal(true)
            .build();
        let ent = ent_file.clone();
        dlg.open(Some(&win_file), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(file) = res {
                if let Some(p) = file.path() {
                    ent.set_text(&p.to_string_lossy());
                }
            }
        });
    });
    input_row.append(&file_btn);

    let folder_btn = Button::with_label(lang.pick("Klasör Seç...", "Select Folder..."));
    folder_btn.add_css_class("akrep-ghost-btn");
    let win_dir = window.clone();
    let ent_dir = path_entry.clone();
    folder_btn.connect_clicked(move |_| {
        let dlg = FileDialog::builder()
            .title("Select Folder to Scan")
            .modal(true)
            .build();
        let ent = ent_dir.clone();
        dlg.select_folder(Some(&win_dir), gtk4::gio::Cancellable::NONE, move |res| {
            if let Ok(folder) = res {
                if let Some(p) = folder.path() {
                    ent.set_text(&p.to_string_lossy());
                }
            }
        });
    });
    input_row.append(&folder_btn);
    scan_box.append(&input_row);

    let opt_row = GtkBox::new(Orientation::Horizontal, 10);
    let rec_chk = CheckButton::with_label(lang.pick("Alt klasörleri tara", "Scan subdirectories"));
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

    let start_btn = Button::with_label(lang.pick("Taramayı Başlat", "Start Scan"));
    start_btn.add_css_class("akrep-primary-btn");
    let cmd_start = cmd_tx.clone();
    let st_start = Rc::clone(state);
    start_btn.connect_clicked(move |_| {
        let d = st_start.borrow().threat_draft.clone();
        let _ = cmd_start.send_blocking(UiCommand::StartScan {
            request: ScanRequest {
                target_path: d.scan_path.trim().to_string(),
                recursive: d.recursive,
                socket_override: None,
                synchronous: false,
            },
        });
    });
    opt_row.append(&start_btn);
    scan_box.append(&opt_row);

    if let Some(prog) = &active_scan {
        let pbar = ProgressBar::new();
        pbar.set_show_text(true);
        pbar.set_fraction((prog.percent.min(100) as f64) / 100.0);
        pbar.set_text(Some(&format!(
            "{:?} — {}% ({}/{} {})",
            prog.state,
            prog.percent,
            prog.scanned_files,
            prog.total_files,
            lang.pick("dosya", "files")
        )));
        scan_box.append(&pbar);

        if let Some(cur) = &prog.current_path {
            let cur_lbl = Label::new(Some(&format!(
                "{}: {}",
                lang.pick("Taranan", "Current"),
                cur
            )));
            cur_lbl.add_css_class("akrep-mono");
            cur_lbl.set_xalign(0.0);
            cur_lbl.set_wrap(true);
            scan_box.append(&cur_lbl);
        }

        if prog.state == ScanJobState::Running {
            let cancel_btn = Button::with_label(lang.pick("Taramayı İptal Et", "Cancel Scan"));
            cancel_btn.add_css_class("akrep-danger-btn");
            cancel_btn.set_halign(Align::Start);
            let cmd_cancel = cmd_tx.clone();
            let jid = prog.job_id.clone();
            cancel_btn.connect_clicked(move |_| {
                let _ = cmd_cancel.send_blocking(UiCommand::CancelScan {
                    job_id: jid.clone(),
                });
            });
            scan_box.append(&cancel_btn);
        }

        if let Some(err) = &prog.error_message {
            let el = Label::new(Some(err));
            el.add_css_class("akrep-toast-err");
            el.set_xalign(0.0);
            el.set_wrap(true);
            scan_box.append(&el);
        }

        if !prog.findings.is_empty() {
            let res_hdr = Label::new(Some(&format!(
                "{} ({})",
                lang.pick("Tarama Bulguları", "Scan Detections"),
                prog.findings.len()
            )));
            res_hdr.add_css_class("akrep-card-title");
            res_hdr.set_xalign(0.0);
            scan_box.append(&res_hdr);

            for finding in &prog.findings {
                let f_row = GtkBox::new(Orientation::Horizontal, 10);
                f_row.add_css_class("akrep-diff-row");

                let (v_lbl, v_sev) = match &finding.verdict {
                    ScanVerdict::Infected { signature } => {
                        (format!("INFECTED: {}", signature), AkrepSeverity::Critical)
                    }
                    ScanVerdict::Suspicious { reason } => {
                        (format!("SUSPICIOUS: {}", reason), AkrepSeverity::Attention)
                    }
                    ScanVerdict::Unknown { reason } => {
                        (format!("UNKNOWN: {}", reason), AkrepSeverity::Unknown)
                    }
                    ScanVerdict::Clean => ("CLEAN".to_string(), AkrepSeverity::Protected),
                };

                let vb = Label::new(Some(&v_lbl));
                vb.add_css_class("akrep-badge");
                vb.add_css_class(severity_css_class(v_sev));
                f_row.append(&vb);

                let pl = Label::new(Some(&finding.path));
                pl.add_css_class("akrep-mono");
                pl.set_xalign(0.0);
                pl.set_wrap(true);
                pl.set_hexpand(true);
                f_row.append(&pl);

                let q_btn = Button::with_label(lang.pick("Karantinaya Al", "Quarantine"));
                q_btn.add_css_class("akrep-primary-btn");
                let cmd_q = cmd_tx.clone();
                let fpath = finding.path.clone();
                let tname = finding
                    .threat_name
                    .clone()
                    .unwrap_or_else(|| "Detected.Threat".to_string());
                q_btn.connect_clicked(move |_| {
                    let _ = cmd_q.send_blocking(UiCommand::CreatePlan {
                        request: ActionRequest::QuarantineFile {
                            path: fpath.clone(),
                            threat_name: tname.clone(),
                        },
                    });
                });
                f_row.append(&q_btn);

                scan_box.append(&f_row);
            }
        } else if prog.state == ScanJobState::Completed {
            let clean_lbl = Label::new(Some(lang.pick(
                "Tarama tamamlandı — herhangi bir zararlı imza tespit edilmedi.",
                "Scan completed cleanly — zero threats detected.",
            )));
            clean_lbl.add_css_class("akrep-subtitle");
            clean_lbl.set_xalign(0.0);
            scan_box.append(&clean_lbl);
        }
    }
    root.append(&scan_box);

    // 4. Quarantine Vault Card
    let vault_box = GtkBox::new(Orientation::Vertical, 10);
    vault_box.add_css_class("akrep-detail-box");

    let v_hdr = GtkBox::new(Orientation::Horizontal, 10);
    let v_title = Label::new(Some(lang.pick(
        "Zırhlı Karantina Kasası (/var/lib/akrep/quarantine — 0700 / 0000)",
        "Armored Quarantine Vault (/var/lib/akrep/quarantine — 0700 / 0000)",
    )));
    v_title.add_css_class("akrep-section-hdr");
    v_title.set_xalign(0.0);
    v_title.set_hexpand(true);
    v_hdr.append(&v_title);

    let confirm_del_chk =
        CheckButton::with_label(lang.pick("Kalıcı silmeyi onayla", "Confirm permanent delete"));
    confirm_del_chk.set_active(draft.confirm_permanent_delete);
    let st_del = Rc::clone(state);
    confirm_del_chk.connect_toggled(move |chk| {
        st_del.borrow_mut().threat_draft.confirm_permanent_delete = chk.is_active();
    });
    v_hdr.append(&confirm_del_chk);

    let load_v_btn = Button::with_label(lang.pick("Kasayı Yükle (Polkit)", "Load Vault (Polkit)"));
    load_v_btn.add_css_class("akrep-ghost-btn");
    let cmd_lv = cmd_tx.clone();
    load_v_btn.connect_clicked(move |_| {
        let _ = cmd_lv.send_blocking(UiCommand::ListQuarantine);
    });
    v_hdr.append(&load_v_btn);
    vault_box.append(&v_hdr);

    match &quarantine {
        Some(inv) if !inv.entries.is_empty() => {
            for entry in &inv.entries {
                let erow = GtkBox::new(Orientation::Horizontal, 10);
                erow.add_css_class("akrep-diff-row");

                let info_col = GtkBox::new(Orientation::Vertical, 2);
                info_col.set_hexpand(true);
                let p_lbl = Label::new(Some(&format!(
                    "{}  [{}]",
                    entry.original_path, entry.threat_name
                )));
                p_lbl.add_css_class("akrep-card-title");
                p_lbl.set_xalign(0.0);
                p_lbl.set_wrap(true);
                info_col.append(&p_lbl);

                let meta_lbl = Label::new(Some(&format!(
                    "ID: {} • SHA-256: {}... • {} bytes • {}",
                    entry.quarantine_id,
                    entry.sha256.chars().take(16).collect::<String>(),
                    entry.size_bytes,
                    if entry.restored {
                        lang.pick("Geri Yüklendi", "Restored")
                    } else {
                        lang.pick("Karantinada", "Isolated")
                    }
                )));
                meta_lbl.add_css_class("akrep-mono");
                meta_lbl.set_xalign(0.0);
                meta_lbl.set_wrap(true);
                info_col.append(&meta_lbl);
                erow.append(&info_col);

                if !entry.restored {
                    let res_btn = Button::with_label(lang.pick("Geri Yükle", "Restore"));
                    res_btn.add_css_class("akrep-ghost-btn");
                    res_btn.set_valign(Align::Center);
                    let cmd_r = cmd_tx.clone();
                    let qid_r = entry.quarantine_id.clone();
                    res_btn.connect_clicked(move |_| {
                        let _ = cmd_r.send_blocking(UiCommand::CreatePlan {
                            request: ActionRequest::RestoreQuarantine {
                                quarantine_id: qid_r.clone(),
                            },
                        });
                    });
                    erow.append(&res_btn);
                }

                let del_btn = Button::with_label(lang.pick("Kalıcı Sil", "Delete (Confirm)"));
                del_btn.add_css_class("akrep-danger-btn");
                del_btn.set_valign(Align::Center);
                let cmd_d = cmd_tx.clone();
                let st_d = Rc::clone(state);
                let qid_d = entry.quarantine_id.clone();
                del_btn.connect_clicked(move |_| {
                    let confirmed = st_d.borrow().threat_draft.confirm_permanent_delete;
                    let _ = cmd_d.send_blocking(UiCommand::CreatePlan {
                        request: ActionRequest::DeleteQuarantine {
                            quarantine_id: qid_d.clone(),
                            confirm_permanent_delete: confirmed,
                        },
                    });
                });
                erow.append(&del_btn);

                vault_box.append(&erow);
            }
        }
        Some(_) => {
            let empty_lbl = Label::new(Some(lang.pick(
                "Karantina kasası boş.",
                "Quarantine vault is currently empty.",
            )));
            empty_lbl.add_css_class("akrep-subtitle");
            empty_lbl.set_xalign(0.0);
            vault_box.append(&empty_lbl);
        }
        None => {
            let hint_lbl = Label::new(Some(lang.pick(
                "Karantina kayıtlarını görüntülemek için 'Kasayı Yükle (Polkit)' düğmesine tıklayın.",
                "Click 'Load Vault (Polkit)' to inspect isolated quarantine artifacts.",
            )));
            hint_lbl.add_css_class("akrep-subtitle");
            hint_lbl.set_xalign(0.0);
            vault_box.append(&hint_lbl);
        }
    }
    root.append(&vault_box);

    root
}
