use crate::components::{
    build_admin_note, build_kv_row, build_severity_indicator, build_status_indicator,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::AppUiState;
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Label, Orientation};
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

    let root = GtkBox::new(Orientation::Vertical, 20);

    let intro_col = GtkBox::new(Orientation::Vertical, 4);
    let hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    hdr_row.set_valign(Align::Center);

    let page_desc = Label::new(Some(lang.pick(
        "Secure Boot, TPM 2.0, çekirdek bütünlüğü, USB aygıtları ve paket imzaları",
        "Secure Boot, TPM 2.0, kernel integrity, USB devices and package signatures",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    page_desc.set_hexpand(true);
    hdr_row.append(&page_desc);

    let deep_btn = Button::with_label(lang.pick("Derin denetim", "Run deep audit"));
    deep_btn.add_css_class("akrep-primary-btn");
    deep_btn.set_valign(Align::Center);
    let cmd_deep = cmd_tx.clone();
    deep_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_deep, UiCommand::InspectDeviceSecurity);
    });
    hdr_row.append(&deep_btn);
    intro_col.append(&hdr_row);
    intro_col.append(&build_admin_note(lang));
    root.append(&intro_col);

    if let Some(d) = &dev {
        let hw_card = GtkBox::new(Orientation::Vertical, 12);
        hw_card.add_css_class("akrep-detail-box");
        let hw_hdr = GtkBox::new(Orientation::Horizontal, 12);
        hw_hdr.set_valign(Align::Center);

        let hw_title = Label::new(Some(lang.pick("Aygıt yazılımı", "Firmware trust")));
        hw_title.add_css_class("akrep-section-hdr");
        hw_title.set_xalign(0.0);
        hw_title.set_hexpand(true);
        hw_hdr.append(&hw_title);

        let sb_text = format!(
            "Secure Boot: {}",
            match lang {
                crate::i18n::Lang::Tr => d.secure_boot.severity.label_tr(),
                crate::i18n::Lang::En => d.secure_boot.severity.label_en(),
            }
        );
        hw_hdr.append(&build_status_indicator(
            crate::components::severity_css_class(d.secure_boot.severity),
            &sb_text,
            matches!(
                d.secure_boot.severity,
                blaze_sentinel_core::AkrepSeverity::Unavailable
                    | blaze_sentinel_core::AkrepSeverity::Unknown
            ),
        ));

        let tpm_text = format!(
            "TPM: {}",
            match lang {
                crate::i18n::Lang::Tr => d.tpm.severity.label_tr(),
                crate::i18n::Lang::En => d.tpm.severity.label_en(),
            }
        );
        hw_hdr.append(&build_status_indicator(
            crate::components::severity_css_class(d.tpm.severity),
            &tpm_text,
            matches!(
                d.tpm.severity,
                blaze_sentinel_core::AkrepSeverity::Unavailable
                    | blaze_sentinel_core::AkrepSeverity::Unknown
            ),
        ));
        hw_card.append(&hw_hdr);

        hw_card.append(&build_kv_row("UEFI Secure Boot", &d.secure_boot.detail));
        hw_card.append(&build_kv_row("TPM 2.0", &d.tpm.detail));
        root.append(&hw_card);

        let krn_card = GtkBox::new(Orientation::Vertical, 12);
        krn_card.add_css_class("akrep-detail-box");
        let k_title = Label::new(Some(lang.pick(
            "Çekirdek bütünlüğü",
            "Kernel integrity",
        )));
        k_title.add_css_class("akrep-section-hdr");
        k_title.set_xalign(0.0);
        krn_card.append(&k_title);

        krn_card.append(&build_kv_row(
            lang.pick("Modül imzaları", "Module signatures"),
            &d.kernel_lockdown.detail,
        ));
        krn_card.append(&build_kv_row(
            "/etc/ld.so.preload",
            &d.runtime_integrity.ld_so_preload_detail,
        ));
        let canary_status = match (
            d.runtime_integrity.root_canary_present,
            d.runtime_integrity.root_canary_intact,
        ) {
            (true, true) => lang.pick("Etkin ve sağlam", "Present and intact"),
            (true, false) => lang.pick("Değişiklik tespit edildi", "Tampering detected"),
            (false, _) => lang.pick("Bulunamadı", "Missing"),
        };
        krn_card.append(&build_kv_row(
            lang.pick("Kök bütünlük kanarisi", "Root integrity canary"),
            canary_status,
        ));
        root.append(&krn_card);

        let usb_card = GtkBox::new(Orientation::Vertical, 12);
        usb_card.add_css_class("akrep-detail-box");
        let u_hdr = GtkBox::new(Orientation::Horizontal, 12);
        u_hdr.set_valign(Align::Center);
        let u_title = Label::new(Some(&format!(
            "{} ({})",
            lang.pick("Bağlı USB aygıtları", "Connected USB devices"),
            d.usb_audit.devices.len()
        )));
        u_title.add_css_class("akrep-section-hdr");
        u_title.set_xalign(0.0);
        u_title.set_hexpand(true);
        u_hdr.append(&u_title);

        u_hdr.append(&build_severity_indicator(d.usb_audit.severity, lang));
        usb_card.append(&u_hdr);

        if d.usb_audit.devices.is_empty() {
            let ul = Label::new(Some(lang.pick(
                "Bağlı harici USB aygıtı algılanmadı",
                "No connected USB devices detected",
            )));
            ul.add_css_class("akrep-subtitle");
            ul.set_xalign(0.0);
            usb_card.append(&ul);
        } else {
            for udev in &d.usb_audit.devices {
                let urow = GtkBox::new(Orientation::Horizontal, 12);
                urow.add_css_class("akrep-diff-row");
                urow.set_valign(Align::Center);

                let auth_txt = if udev.authorized {
                    lang.pick("Yetkili", "Authorized")
                } else {
                    lang.pick("Yetkisiz", "Unauthorized")
                };
                let desc = format!(
                    "{}:{} · {} {} ({})",
                    udev.id_vendor,
                    udev.id_product,
                    udev.manufacturer
                        .as_deref()
                        .unwrap_or(lang.pick("Bilinmeyen üretici", "Unknown vendor")),
                    udev.product
                        .as_deref()
                        .unwrap_or(lang.pick("USB aygıtı", "USB device")),
                    auth_txt
                );
                let ulbl = Label::new(Some(&desc));
                ulbl.add_css_class("akrep-mono");
                ulbl.set_xalign(0.0);
                ulbl.set_wrap(true);
                ulbl.set_wrap_mode(WrapMode::WordChar);
                ulbl.set_hexpand(true);
                urow.append(&ulbl);

                if !udev.anomalies.is_empty() {
                    urow.append(&build_status_indicator(
                        "akrep-sev-attention",
                        &udev.anomalies.join(", "),
                        false,
                    ));
                }
                usb_card.append(&urow);
            }
        }
        root.append(&usb_card);

        let rpm_card = GtkBox::new(Orientation::Vertical, 12);
        rpm_card.add_css_class("akrep-detail-box");
        let r_hdr = GtkBox::new(Orientation::Horizontal, 12);
        r_hdr.set_valign(Align::Center);
        let r_title = Label::new(Some(lang.pick(
            "Paket imza bütünlüğü",
            "Package signature integrity",
        )));
        r_title.add_css_class("akrep-section-hdr");
        r_title.set_xalign(0.0);
        r_title.set_hexpand(true);
        r_hdr.append(&r_title);

        r_hdr.append(&build_severity_indicator(d.rpm_integrity.severity, lang));
        rpm_card.append(&r_hdr);

        rpm_card.append(&build_kv_row(
            lang.pick("İmzalı RPM paketleri", "Signed RPM packages"),
            &format!(
                "{} / {}",
                d.rpm_integrity.signed_packages, d.rpm_integrity.total_packages
            ),
        ));
        rpm_card.append(&build_kv_row(
            lang.pick("Doğrulanan kritik paketler", "Verified critical packages"),
            &if d.rpm_integrity.verified_critical_packages.is_empty() {
                "-".to_string()
            } else {
                d.rpm_integrity.verified_critical_packages.join(", ")
            },
        ));
        if !d.rpm_integrity.unsigned_packages.is_empty() {
            rpm_card.append(&build_kv_row(
                lang.pick("İmzasız paketler", "Unsigned packages"),
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
                lang.pick("Doğrulama sapmaları", "Verification anomalies"),
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

    let base_card = GtkBox::new(Orientation::Vertical, 12);
    base_card.add_css_class("akrep-detail-box");
    let b_title = Label::new(Some(lang.pick(
        "Temel sistem durumu",
        "System baseline",
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
            "USB aygıtları, çekirdek bütünlüğü ve paket imzaları için üstteki Derin denetim düğmesini kullanın",
            "Select Run deep audit above to inspect USB peripherals, kernel integrity and RPM signatures",
        )));
        sub.add_css_class("akrep-subtitle");
        sub.set_xalign(0.0);
        sub.set_wrap(true);
        base_card.append(&sub);
    }
    root.append(&base_card);

    root
}
