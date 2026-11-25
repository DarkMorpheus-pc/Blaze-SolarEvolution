use crate::components::{build_kv_row, severity_css_class};
use crate::dbus_client::UiCommand;
use crate::state::AppUiState;
use blaze_sentinel_core::AkrepSeverity;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_device_security_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let dev = st.device_security.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let title = Label::new(Some(lang.pick(
        "Cihaz Güvenliği (UEFI Secure Boot, TPM 2.0, Çekirdek, USB & RPM İmzaları)",
        "Device Security (UEFI Secure Boot, TPM 2.0, Kernel, USB & RPM OpenPGP)",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr_row.append(&title);

    let deep_btn = Button::with_label(lang.pick(
        "Derin Donanım & RPM Denetimi (Polkit)",
        "Deep Hardware & RPM Audit (Polkit)",
    ));
    deep_btn.add_css_class("akrep-primary-btn");
    let cmd_deep = cmd_tx.clone();
    deep_btn.connect_clicked(move |_| {
        let _ = cmd_deep.send_blocking(UiCommand::InspectDeviceSecurity);
    });
    hdr_row.append(&deep_btn);
    root.append(&hdr_row);

    if let Some(d) = &dev {
        // 1. Firmware & Hardware Security (Secure Boot + TPM 2.0)
        let hw_card = GtkBox::new(Orientation::Vertical, 8);
        hw_card.add_css_class("akrep-detail-box");
        let hw_hdr = GtkBox::new(Orientation::Horizontal, 8);
        let hw_title = Label::new(Some(lang.pick(
            "Aygıt Yazılımı: UEFI Secure Boot & TPM 2.0",
            "Firmware Trust: UEFI Secure Boot & TPM 2.0",
        )));
        hw_title.add_css_class("akrep-section-hdr");
        hw_title.set_xalign(0.0);
        hw_title.set_hexpand(true);
        hw_hdr.append(&hw_title);

        let sb_badge = Label::new(Some(&format!(
            "Secure Boot: {}",
            match lang {
                crate::i18n::Lang::Tr => d.secure_boot.severity.label_tr(),
                crate::i18n::Lang::En => d.secure_boot.severity.label_en(),
            }
        )));
        sb_badge.add_css_class("akrep-badge");
        sb_badge.add_css_class(severity_css_class(d.secure_boot.severity));
        hw_hdr.append(&sb_badge);

        let tpm_badge = Label::new(Some(&format!(
            "TPM: {}",
            match lang {
                crate::i18n::Lang::Tr => d.tpm.severity.label_tr(),
                crate::i18n::Lang::En => d.tpm.severity.label_en(),
            }
        )));
        tpm_badge.add_css_class("akrep-badge");
        tpm_badge.add_css_class(severity_css_class(d.tpm.severity));
        hw_hdr.append(&tpm_badge);
        hw_card.append(&hw_hdr);

        hw_card.append(&build_kv_row("UEFI Secure Boot", &d.secure_boot.detail));
        hw_card.append(&build_kv_row("TPM 2.0", &d.tpm.detail));
        root.append(&hw_card);

        // 2. Kernel Lockdown & Runtime Integrity
        let krn_card = GtkBox::new(Orientation::Vertical, 8);
        krn_card.add_css_class("akrep-detail-box");
        let k_title = Label::new(Some(lang.pick(
            "Çekirdek İmza Zorunluluğu, Lockdown & Root Kanarisi",
            "Kernel Signature Enforcement, Lockdown & Root Canary",
        )));
        k_title.add_css_class("akrep-section-hdr");
        k_title.set_xalign(0.0);
        krn_card.append(&k_title);

        krn_card.append(&build_kv_row(
            lang.pick("Modül İmza & Lockdown", "Module Sig & Lockdown"),
            &d.kernel_lockdown.detail,
        ));
        krn_card.append(&build_kv_row(
            "/etc/ld.so.preload",
            &d.runtime_integrity.ld_so_preload_detail,
        ));
        krn_card.append(&build_kv_row(
            lang.pick(
                "Root Kanarisi (/var/lib/blaze/.blaze_canary)",
                "Root Canary",
            ),
            &format!(
                "present={} intact={}",
                d.runtime_integrity.root_canary_present, d.runtime_integrity.root_canary_intact
            ),
        ));
        root.append(&krn_card);

        // 3. Connected USB Devices Audit
        let usb_card = GtkBox::new(Orientation::Vertical, 8);
        usb_card.add_css_class("akrep-detail-box");
        let u_hdr = GtkBox::new(Orientation::Horizontal, 8);
        let u_title = Label::new(Some(&format!(
            "{} ({})",
            lang.pick(
                "Bağlı USB Aygıt Denetimi",
                "Connected USB Peripherals Audit"
            ),
            d.usb_audit.devices.len()
        )));
        u_title.add_css_class("akrep-section-hdr");
        u_title.set_xalign(0.0);
        u_title.set_hexpand(true);
        u_hdr.append(&u_title);

        let ub = Label::new(Some(match lang {
            crate::i18n::Lang::Tr => d.usb_audit.severity.label_tr(),
            crate::i18n::Lang::En => d.usb_audit.severity.label_en(),
        }));
        ub.add_css_class("akrep-badge");
        ub.add_css_class(severity_css_class(d.usb_audit.severity));
        u_hdr.append(&ub);
        usb_card.append(&u_hdr);

        if d.usb_audit.devices.is_empty() {
            let ul = Label::new(Some(lang.pick(
                "Sysfs üzerinde harici USB aygıtı algılanmadı.",
                "No USB devices enumerated in sysfs.",
            )));
            ul.add_css_class("akrep-subtitle");
            ul.set_xalign(0.0);
            usb_card.append(&ul);
        } else {
            for udev in &d.usb_audit.devices {
                let urow = GtkBox::new(Orientation::Horizontal, 8);
                urow.add_css_class("akrep-diff-row");
                let desc = format!(
                    "{}:{} — {} {} (authorized={})",
                    udev.id_vendor,
                    udev.id_product,
                    udev.manufacturer.as_deref().unwrap_or("Unknown"),
                    udev.product.as_deref().unwrap_or("USB Device"),
                    udev.authorized
                );
                let ulbl = Label::new(Some(&desc));
                ulbl.add_css_class("akrep-mono");
                ulbl.set_xalign(0.0);
                ulbl.set_wrap(true);
                ulbl.set_hexpand(true);
                urow.append(&ulbl);
                if !udev.anomalies.is_empty() {
                    let ab = Label::new(Some(&udev.anomalies.join(", ")));
                    ab.add_css_class("akrep-badge");
                    ab.add_css_class(severity_css_class(AkrepSeverity::Attention));
                    urow.append(&ab);
                }
                usb_card.append(&urow);
            }
        }
        root.append(&usb_card);

        // 4. Critical RPM OpenPGP Signature Health
        let rpm_card = GtkBox::new(Orientation::Vertical, 8);
        rpm_card.add_css_class("akrep-detail-box");
        let r_hdr = GtkBox::new(Orientation::Horizontal, 8);
        let r_title = Label::new(Some(lang.pick(
            "Kritik RPM OpenPGP / EdDSA İmza ve Bütünlük Denetimi",
            "Critical RPM OpenPGP / EdDSA Signature & Verification Health",
        )));
        r_title.add_css_class("akrep-section-hdr");
        r_title.set_xalign(0.0);
        r_title.set_hexpand(true);
        r_hdr.append(&r_title);

        let rb = Label::new(Some(match lang {
            crate::i18n::Lang::Tr => d.rpm_integrity.severity.label_tr(),
            crate::i18n::Lang::En => d.rpm_integrity.severity.label_en(),
        }));
        rb.add_css_class("akrep-badge");
        rb.add_css_class(severity_css_class(d.rpm_integrity.severity));
        r_hdr.append(&rb);
        rpm_card.append(&r_hdr);

        rpm_card.append(&build_kv_row(
            lang.pick("İmzalı Paket Oranı", "Signed RPM Packages"),
            &format!(
                "{} / {}",
                d.rpm_integrity.signed_packages, d.rpm_integrity.total_packages
            ),
        ));
        rpm_card.append(&build_kv_row(
            lang.pick("Doğrulanan Kritik Paketler", "Verified Critical Packages"),
            &if d.rpm_integrity.verified_critical_packages.is_empty() {
                "—".to_string()
            } else {
                d.rpm_integrity.verified_critical_packages.join(", ")
            },
        ));
        if !d.rpm_integrity.unsigned_packages.is_empty() {
            rpm_card.append(&build_kv_row(
                lang.pick("İmzasız Paketler", "Unsigned Packages"),
                &d.rpm_integrity
                    .unsigned_packages
                    .iter()
                    .take(15)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
            ));
        }
        if !d.rpm_integrity.verification_anomalies.is_empty() {
            rpm_card.append(&build_kv_row(
                lang.pick("Bütünlük Sapmaları (rpm -V)", "Verification Anomalies"),
                &d.rpm_integrity
                    .verification_anomalies
                    .iter()
                    .take(10)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        root.append(&rpm_card);
    }

    // Always include read-only sysfs/sysctl/DNF5 baseline from akrepd telemetry
    let base_card = GtkBox::new(Orientation::Vertical, 8);
    base_card.add_css_class("akrep-detail-box");
    let b_title = Label::new(Some(lang.pick(
        "Çekirdek Sysctl ve Tedarik Zinciri Temel Telemetrisi (Salt Okunur)",
        "Kernel Sysctl & Supply-Chain Baseline Telemetry (Read-Only)",
    )));
    b_title.add_css_class("akrep-section-hdr");
    b_title.set_xalign(0.0);
    base_card.append(&b_title);

    if let Some(sn) = &snap {
        if let Some(sec) = sn.sections.iter().find(|x| x.section == "device_security") {
            for m in &sec.metrics {
                base_card.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    } else {
        let sub = Label::new(Some(lang.pick(
            "Ayrıntılı USB listesi, Root kanarisi ve kritik RPM imza doğrulaması için yukarıdaki 'Derin Donanım & RPM Denetimi (Polkit)' düğmesini kullanabilirsiniz.",
            "Click 'Deep Hardware & RPM Audit (Polkit)' above to inspect USB peripherals, Root canary state, and critical RPM OpenPGP signatures.",
        )));
        sub.add_css_class("akrep-subtitle");
        sub.set_xalign(0.0);
        sub.set_wrap(true);
        base_card.append(&sub);
    }
    root.append(&base_card);

    root
}
