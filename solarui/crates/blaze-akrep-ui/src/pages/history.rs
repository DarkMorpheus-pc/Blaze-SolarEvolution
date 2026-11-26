use crate::components::{build_admin_note, build_kv_row, build_tx_state_indicator};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{ActionRequest, TransactionState};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Expander, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub fn summarize_action_request(req: &ActionRequest, lang: Lang) -> String {
    match req {
        ActionRequest::SelinuxSetBoolean {
            name,
            value,
            persistent,
        } => format!(
            "SELinux: {} = {} ({})",
            name,
            if *value {
                lang.pick("açık", "on")
            } else {
                lang.pick("kapalı", "off")
            },
            if *persistent {
                lang.pick("kalıcı", "permanent")
            } else {
                lang.pick("geçici", "temporary")
            }
        ),
        ActionRequest::SelinuxRestorecon {
            target_path,
            recursive,
        } => format!(
            "{}: {}{}",
            lang.pick("Bağlam onarımı", "Context repair"),
            target_path,
            if *recursive { " (-R)" } else { "" }
        ),
        ActionRequest::FirewallAddPort {
            zone,
            port,
            proto,
            source_cidr,
            ..
        } => match source_cidr {
            Some(src) => format!(
                "{}: {}/{} ({}, {})",
                lang.pick("Port aç", "Allow port"),
                port,
                proto.as_str(),
                zone,
                src
            ),
            None => format!(
                "{}: {}/{} ({})",
                lang.pick("Port aç", "Allow port"),
                port,
                proto.as_str(),
                zone
            ),
        },
        ActionRequest::FirewallRemovePort {
            zone, port, proto, ..
        } => format!(
            "{}: {}/{} ({})",
            lang.pick("Port kaldır", "Remove port"),
            port,
            proto.as_str(),
            zone
        ),
        ActionRequest::FirewallAddService { zone, service, .. } => format!(
            "{}: {} ({})",
            lang.pick("Servis aç", "Allow service"),
            service,
            zone
        ),
        ActionRequest::FirewallRemoveService { zone, service, .. } => format!(
            "{}: {} ({})",
            lang.pick("Servis kaldır", "Remove service"),
            service,
            zone
        ),
        ActionRequest::FirewallBindInterface {
            zone, interface, ..
        } => format!(
            "{}: {} → {}",
            lang.pick("Arayüz bağla", "Bind interface"),
            interface,
            zone
        ),
        ActionRequest::FlatpakSetOverride { app_id, .. } => format!(
            "{}: {}",
            lang.pick("Sistem uygulama izni", "System app permission"),
            app_id
        ),
        ActionRequest::QuarantineFile { path, threat_name } => format!(
            "{}: {} ({})",
            lang.pick("Karantinaya al", "Quarantine file"),
            path,
            threat_name
        ),
        ActionRequest::RestoreQuarantine { quarantine_id } => format!(
            "{}: {}",
            lang.pick("Karantinadan geri yükle", "Restore from quarantine"),
            quarantine_id
        ),
        ActionRequest::DeleteQuarantine { quarantine_id, .. } => format!(
            "{}: {}",
            lang.pick("Karantinadan kalıcı sil", "Delete from quarantine"),
            quarantine_id
        ),
        ActionRequest::ApplyProfile { profile_id } => format!(
            "{}: {}",
            lang.pick("Güvenlik profili", "Security profile"),
            profile_id
        ),
        ActionRequest::InstallOptionalScanner => {
            lang.pick("ClamAV kurulumu", "Install ClamAV").to_string()
        }
        #[allow(unreachable_patterns)]
        _ => lang.pick("Güvenlik işlemi", "Security action").to_string(),
    }
}

pub fn can_rollback_request(req: &ActionRequest) -> bool {
    !matches!(
        req,
        ActionRequest::DeleteQuarantine { .. } | ActionRequest::InstallOptionalScanner
    )
}

pub fn build_history_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let records = st.history_records.clone();
    let user_txs = st.user_flatpak_txs.clone();
    let scanner = st.scanner_report.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 20);

    let intro_col = GtkBox::new(Orientation::Vertical, 4);
    let hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    hdr_row.set_valign(Align::Center);

    let page_desc = Label::new(Some(lang.pick(
        "Yapılan güvenlik değişiklikleri, tarama kayıtları ve geri alma seçenekleri",
        "Security changes, scan records and rollback options",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    page_desc.set_hexpand(true);
    hdr_row.append(&page_desc);

    let load_btn = Button::with_label(lang.pick("Geçmişi yükle", "Load history"));
    load_btn.add_css_class("akrep-primary-btn");
    load_btn.set_valign(Align::Center);
    let cmd_ld = cmd_tx.clone();
    load_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_ld, UiCommand::LoadHistory { limit: 100 });
    });
    hdr_row.append(&load_btn);
    intro_col.append(&hdr_row);
    intro_col.append(&build_admin_note(lang));
    root.append(&intro_col);

    // 1. System actions
    let sys_card = GtkBox::new(Orientation::Vertical, 12);
    sys_card.add_css_class("akrep-detail-box");
    let sys_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick("Sistem işlemleri", "System actions"),
        records.len()
    )));
    sys_hdr.add_css_class("akrep-section-hdr");
    sys_hdr.set_xalign(0.0);
    sys_card.append(&sys_hdr);

    if records.is_empty() {
        let hint = Label::new(Some(lang.pick(
            "Kayıtlı sistem değişikliklerini görmek için üstteki Geçmişi yükle düğmesine tıklayın",
            "Select Load history above to view recorded system changes",
        )));
        hint.add_css_class("akrep-subtitle");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        sys_card.append(&hint);
    } else {
        for rec in &records {
            let tcard = GtkBox::new(Orientation::Vertical, 8);
            tcard.add_css_class("akrep-diff-row");

            let top = GtkBox::new(Orientation::Horizontal, 12);
            top.set_valign(Align::Center);
            top.append(&build_tx_state_indicator(rec.state, lang));

            let sum = Label::new(Some(&summarize_action_request(&rec.request, lang)));
            sum.add_css_class("akrep-card-title");
            sum.set_xalign(0.0);
            sum.set_wrap(true);
            sum.set_wrap_mode(WrapMode::WordChar);
            sum.set_hexpand(true);
            top.append(&sum);

            let ts_lbl = Label::new(Some(
                &rec.created_at
                    .chars()
                    .take(16)
                    .collect::<String>()
                    .replace('T', " "),
            ));
            ts_lbl.add_css_class("akrep-meta");
            top.append(&ts_lbl);

            if rec.state == TransactionState::Committed && can_rollback_request(&rec.request) {
                let rb_btn = Button::with_label(lang.pick("Geri al", "Rollback"));
                rb_btn.add_css_class("akrep-danger-btn");
                rb_btn.set_valign(Align::Center);
                let cmd_rb = cmd_tx.clone();
                let tid = rec.tx_id.clone();
                rb_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rb,
                        UiCommand::RollbackTransaction { tx_id: tid.clone() },
                    );
                });
                top.append(&rb_btn);
            }
            tcard.append(&top);

            let exp = Expander::new(Some(lang.pick("Teknik ayrıntılar", "Technical details")));
            let details = GtkBox::new(Orientation::Vertical, 4);
            details.set_margin_top(8);
            details.append(&build_kv_row(lang.pick("İşlem kimliği", "Action ID"), &rec.tx_id));
            details.append(&build_kv_row(lang.pick("Plan kimliği", "Plan ID"), &rec.plan_id));
            details.append(&build_kv_row("Plan SHA-256", &rec.plan_sha256));
            details.append(&build_kv_row(
                lang.pick("Kullanıcı", "Actor"),
                &format!("uid={} ({})", rec.actor_uid, rec.actor_bus_name),
            ));
            details.append(&build_kv_row(
                lang.pick("Yetki eylemi", "Auth action"),
                &rec.polkit_action,
            ));
            if let Some(ar) = &rec.apply_result {
                details.append(&build_kv_row(
                    lang.pick("Uygulama sonucu", "Apply summary"),
                    ar,
                ));
            }
            if let Some(rr) = &rec.rollback_result {
                details.append(&build_kv_row(
                    lang.pick("Geri alma sonucu", "Rollback summary"),
                    rr,
                ));
            }
            if let Some(em) = &rec.error_message {
                details.append(&build_kv_row(lang.pick("Hata ayrıntısı", "Error detail"), em));
            }
            for fp in &rec.post_fingerprints {
                details.append(&build_kv_row(
                    &format!("SHA-256 ({})", fp.target),
                    &fp.sha256,
                ));
            }
            exp.set_child(Some(&details));
            tcard.append(&exp);

            sys_card.append(&tcard);
        }
    }
    root.append(&sys_card);

    // 2. User app changes
    let usr_card = GtkBox::new(Orientation::Vertical, 12);
    usr_card.add_css_class("akrep-detail-box");
    let usr_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick("Kullanıcı uygulama izinleri", "User app permissions"),
        user_txs.len()
    )));
    usr_hdr.add_css_class("akrep-section-hdr");
    usr_hdr.set_xalign(0.0);
    usr_card.append(&usr_hdr);

    if user_txs.is_empty() {
        let uempty = Label::new(Some(lang.pick(
            "Kayıtlı kullanıcı izin değişikliği yok",
            "No recorded user app permission changes",
        )));
        uempty.add_css_class("akrep-subtitle");
        uempty.set_xalign(0.0);
        usr_card.append(&uempty);
    } else {
        for utx in &user_txs {
            let urow = GtkBox::new(Orientation::Horizontal, 12);
            urow.add_css_class("akrep-diff-row");
            urow.set_valign(Align::Center);

            urow.append(&build_tx_state_indicator(utx.state, lang));

            let ulbl = Label::new(Some(&format!(
                "{} · {}",
                utx.app_id,
                utx.created_at
                    .chars()
                    .take(16)
                    .collect::<String>()
                    .replace('T', " ")
            )));
            ulbl.add_css_class("akrep-mono");
            ulbl.set_xalign(0.0);
            ulbl.set_wrap(true);
            ulbl.set_wrap_mode(WrapMode::WordChar);
            ulbl.set_hexpand(true);
            urow.append(&ulbl);

            if utx.state == TransactionState::Committed {
                let rb = Button::with_label(lang.pick("Geri al", "Rollback"));
                rb.add_css_class("akrep-ghost-btn");
                let cmd_rb = cmd_tx.clone();
                let tid = utx.tx_id.clone();
                rb.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rb,
                        UiCommand::RollbackUserFlatpakTx { tx_id: tid.clone() },
                    );
                });
                urow.append(&rb);
            }
            usr_card.append(&urow);
        }
    }
    root.append(&usr_card);

    // 3. Scan & recovery summary
    let log_card = GtkBox::new(Orientation::Vertical, 12);
    log_card.add_css_class("akrep-detail-box");
    let l_hdr = Label::new(Some(lang.pick(
        "Tarama ve kurtarma kayıtları",
        "Scan and recovery records",
    )));
    l_hdr.add_css_class("akrep-section-hdr");
    l_hdr.set_xalign(0.0);
    log_card.append(&l_hdr);

    if let Some(sc) = &scanner {
        for sh in &sc.recent_history {
            log_card.append(&build_kv_row(
                &format!("{} ({})", lang.pick("Tarama", "Scan"), sh.job_id),
                &format!(
                    "{} · {} {}, {} {}",
                    sh.target_path,
                    sh.scanned_files,
                    lang.pick("dosya", "files"),
                    sh.findings_count,
                    lang.pick("bulgu", "findings")
                ),
            ));
        }
    }

    if let Some(sn) = &snap {
        if let Some(sec) = sn.sections.iter().find(|s| s.section == "history") {
            for m in &sec.metrics {
                log_card.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    }
    root.append(&log_card);

    root
}
