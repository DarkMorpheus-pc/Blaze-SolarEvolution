use crate::components::{build_kv_row, tx_state_css_class, tx_state_label};
use crate::dbus_client::UiCommand;
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{ActionRequest, TransactionState};
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
            "SELinux Boolean: {} = {} ({})",
            name,
            value,
            if *persistent { "persistent" } else { "runtime" }
        ),
        ActionRequest::SelinuxRestorecon {
            target_path,
            recursive,
        } => format!(
            "SELinux restorecon: {}{}",
            target_path,
            if *recursive { " (-R)" } else { "" }
        ),
        ActionRequest::FirewallAddPort {
            zone,
            port,
            proto,
            source_cidr,
            ..
        } => format!(
            "Firewall Add Port: {}/{} in {} (cidr={})",
            port,
            proto.as_str(),
            zone,
            source_cidr.as_deref().unwrap_or("any")
        ),
        ActionRequest::FirewallRemovePort {
            zone, port, proto, ..
        } => format!(
            "Firewall Remove Port: {}/{} in {}",
            port,
            proto.as_str(),
            zone
        ),
        ActionRequest::FirewallAddService { zone, service, .. } => {
            format!("Firewall Add Service: {} in {}", service, zone)
        }
        ActionRequest::FirewallRemoveService { zone, service, .. } => {
            format!("Firewall Remove Service: {} in {}", service, zone)
        }
        ActionRequest::FirewallBindInterface {
            zone, interface, ..
        } => format!("Firewall Bind Interface: {} -> {}", interface, zone),
        ActionRequest::FlatpakSetOverride { app_id, .. } => {
            format!("System Flatpak Override: {}", app_id)
        }
        ActionRequest::QuarantineFile { path, threat_name } => {
            format!("Quarantine File: {} [{}]", path, threat_name)
        }
        ActionRequest::RestoreQuarantine { quarantine_id } => {
            format!("Restore Quarantine: {}", quarantine_id)
        }
        ActionRequest::DeleteQuarantine { quarantine_id, .. } => {
            format!("Permanent Delete Quarantine: {}", quarantine_id)
        }
        ActionRequest::ApplyProfile { profile_id } => {
            format!(
                "{}: {}",
                lang.pick("Güvenlik Profili Uygulama", "Apply Security Profile"),
                profile_id
            )
        }
        ActionRequest::InstallOptionalScanner => lang
            .pick(
                "İsteğe Bağlı ClamAV Kurulumu",
                "Install Optional ClamAV Scanner",
            )
            .to_string(),
        #[allow(unreachable_patterns)]
        _ => "System Security Transaction".to_string(),
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

    let root = GtkBox::new(Orientation::Vertical, 16);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let title = Label::new(Some(lang.pick(
        "Koruma Geçmişi, İşlem Denetim Kaydı ve Tek Tıkla Geri Alma",
        "Protection History, Transaction Audit Log & 1-Click Rollback",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr_row.append(&title);

    let load_btn = Button::with_label(lang.pick(
        "Sistem İşlem Geçmişini Yükle (Polkit)",
        "Load System History (Polkit)",
    ));
    load_btn.add_css_class("akrep-primary-btn");
    let cmd_ld = cmd_tx.clone();
    load_btn.connect_clicked(move |_| {
        let _ = cmd_ld.send_blocking(UiCommand::LoadHistory { limit: 100 });
    });
    hdr_row.append(&load_btn);
    root.append(&hdr_row);

    // 1. Privileged System Transactions (Committed, RolledBack, RollbackFailed, Interrupted)
    let sys_card = GtkBox::new(Orientation::Vertical, 10);
    sys_card.add_css_class("akrep-detail-box");
    let sys_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick(
            "Ayrıcalıklı Sistem İşlemleri (akrep-privd)",
            "Privileged System Transactions (akrep-privd)"
        ),
        records.len()
    )));
    sys_hdr.add_css_class("akrep-section-hdr");
    sys_hdr.set_xalign(0.0);
    sys_card.append(&sys_hdr);

    if records.is_empty() {
        let hint = Label::new(Some(lang.pick(
            "Henüz işlem yüklenmedi veya kayıt yok. Kayıtlı tüm Committed / RolledBack / RollbackFailed / Interrupted işlemleri görmek için yukarıdaki düğmeye tıklayın.",
            "No system transactions loaded yet. Click 'Load System History (Polkit)' above to inspect Committed / RolledBack / RollbackFailed / Interrupted records.",
        )));
        hint.add_css_class("akrep-subtitle");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        sys_card.append(&hint);
    } else {
        for rec in &records {
            let tcard = GtkBox::new(Orientation::Vertical, 6);
            tcard.add_css_class("akrep-diff-row");

            let top = GtkBox::new(Orientation::Horizontal, 8);
            let sb = Label::new(Some(tx_state_label(rec.state, lang)));
            sb.add_css_class("akrep-badge");
            sb.add_css_class(tx_state_css_class(rec.state));
            top.append(&sb);

            let sum = Label::new(Some(&summarize_action_request(&rec.request, lang)));
            sum.add_css_class("akrep-card-title");
            sum.set_xalign(0.0);
            sum.set_wrap(true);
            sum.set_hexpand(true);
            top.append(&sum);

            let ts_lbl = Label::new(Some(&rec.created_at.chars().take(19).collect::<String>()));
            ts_lbl.add_css_class("akrep-mono");
            top.append(&ts_lbl);

            if rec.state == TransactionState::Committed && can_rollback_request(&rec.request) {
                let rb_btn = Button::with_label(lang.pick("Geri Al (Rollback)", "Rollback"));
                rb_btn.add_css_class("akrep-danger-btn");
                rb_btn.set_valign(Align::Center);
                let cmd_rb = cmd_tx.clone();
                let tid = rec.tx_id.clone();
                rb_btn.connect_clicked(move |_| {
                    let _ =
                        cmd_rb.send_blocking(UiCommand::RollbackTransaction { tx_id: tid.clone() });
                });
                top.append(&rb_btn);
            }
            tcard.append(&top);

            let exp = Expander::new(Some(lang.pick(
                "İşlem Ayrıntıları ve SHA-256 Parmak İzleri",
                "Transaction Metadata & SHA-256 State Fingerprints",
            )));
            let details = GtkBox::new(Orientation::Vertical, 4);
            details.set_margin_top(6);
            details.append(&build_kv_row("Transaction UUID", &rec.tx_id));
            details.append(&build_kv_row("Plan UUID", &rec.plan_id));
            details.append(&build_kv_row("Plan SHA-256", &rec.plan_sha256));
            details.append(&build_kv_row(
                lang.pick("Aktör UID / Veriyolu", "Actor UID / Bus Name"),
                &format!("uid={} ({})", rec.actor_uid, rec.actor_bus_name),
            ));
            details.append(&build_kv_row("Polkit Action", &rec.polkit_action));
            if let Some(ar) = &rec.apply_result {
                details.append(&build_kv_row(
                    lang.pick("Uygulama Sonucu", "Apply Summary"),
                    ar,
                ));
            }
            if let Some(rr) = &rec.rollback_result {
                details.append(&build_kv_row(
                    lang.pick("Geri Alma Sonucu", "Rollback Summary"),
                    rr,
                ));
            }
            if let Some(em) = &rec.error_message {
                details.append(&build_kv_row(lang.pick("Hata İletisi", "Error Detail"), em));
            }
            for fp in &rec.post_fingerprints {
                details.append(&build_kv_row(
                    &format!("Post SHA-256 ({})", fp.target),
                    &fp.sha256,
                ));
            }
            exp.set_child(Some(&details));
            tcard.append(&exp);

            sys_card.append(&tcard);
        }
    }
    root.append(&sys_card);

    // 2. User-Scope Flatpak Override Transactions
    let usr_card = GtkBox::new(Orientation::Vertical, 8);
    usr_card.add_css_class("akrep-detail-box");
    let usr_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick(
            "Kullanıcı Kapsamlı Flatpak İşlemleri",
            "User-Scope Flatpak Override Transactions"
        ),
        user_txs.len()
    )));
    usr_hdr.add_css_class("akrep-section-hdr");
    usr_hdr.set_xalign(0.0);
    usr_card.append(&usr_hdr);

    if user_txs.is_empty() {
        let uempty = Label::new(Some(lang.pick(
            "Kullanıcı kapsamında kayıtlı Flatpak override işlemi yok.",
            "No user-scope Flatpak override transactions recorded.",
        )));
        uempty.add_css_class("akrep-subtitle");
        uempty.set_xalign(0.0);
        usr_card.append(&uempty);
    } else {
        for utx in &user_txs {
            let urow = GtkBox::new(Orientation::Horizontal, 8);
            urow.add_css_class("akrep-diff-row");

            let sb = Label::new(Some(tx_state_label(utx.state, lang)));
            sb.add_css_class("akrep-badge");
            sb.add_css_class(tx_state_css_class(utx.state));
            urow.append(&sb);

            let ulbl = Label::new(Some(&format!(
                "User Flatpak Override: {} ({}) • {}",
                utx.app_id,
                utx.tx_id,
                utx.created_at.chars().take(19).collect::<String>()
            )));
            ulbl.add_css_class("akrep-mono");
            ulbl.set_xalign(0.0);
            ulbl.set_wrap(true);
            ulbl.set_hexpand(true);
            urow.append(&ulbl);

            if utx.state == TransactionState::Committed {
                let rb = Button::with_label(lang.pick("Geri Al", "Rollback"));
                rb.add_css_class("akrep-ghost-btn");
                let cmd_rb = cmd_tx.clone();
                let tid = utx.tx_id.clone();
                rb.connect_clicked(move |_| {
                    let _ = cmd_rb
                        .send_blocking(UiCommand::RollbackUserFlatpakTx { tx_id: tid.clone() });
                });
                urow.append(&rb);
            }
            usr_card.append(&urow);
        }
    }
    root.append(&usr_card);

    // 3. Scan & Recovery Event Summary
    let log_card = GtkBox::new(Orientation::Vertical, 8);
    log_card.add_css_class("akrep-detail-box");
    let l_hdr = Label::new(Some(lang.pick(
        "Tarama, Sentinel ve BERE Kurtarma Günlüğü Özeti",
        "Scan, Sentinel & BERE Recovery Event Summary",
    )));
    l_hdr.add_css_class("akrep-section-hdr");
    l_hdr.set_xalign(0.0);
    log_card.append(&l_hdr);

    if let Some(sc) = &scanner {
        for sh in &sc.recent_history {
            log_card.append(&build_kv_row(
                &format!("AV Scan ({})", sh.job_id),
                &format!(
                    "{} — {:?} ({} files, {} findings)",
                    sh.target_path, sh.state, sh.scanned_files, sh.findings_count
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
