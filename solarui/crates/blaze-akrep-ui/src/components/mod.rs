pub mod finding_row;
pub mod header_banner;
pub mod nav_rail;
pub mod plan_dialog;
pub mod status_card;

use crate::i18n::Lang;
use blaze_akrep_common::{RiskLevel, TransactionState};
use blaze_sentinel_core::{AkrepSeverity, Availability};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Image, Label, Orientation};

pub fn severity_css_class(sev: AkrepSeverity) -> &'static str {
    match sev {
        AkrepSeverity::Protected => "akrep-sev-protected",
        AkrepSeverity::Attention => "akrep-sev-attention",
        AkrepSeverity::Critical => "akrep-sev-critical",
        AkrepSeverity::Unavailable => "akrep-sev-unavailable",
        AkrepSeverity::Unknown => "akrep-sev-unknown",
    }
}

pub fn availability_css_class(avail: Availability) -> &'static str {
    match avail {
        Availability::Available => "akrep-sev-protected",
        Availability::Unavailable => "akrep-sev-unavailable",
        Availability::Unknown => "akrep-sev-unknown",
    }
}

pub fn risk_css_class(risk: RiskLevel) -> &'static str {
    match risk {
        RiskLevel::Low => "akrep-risk-low",
        RiskLevel::Medium => "akrep-risk-medium",
        RiskLevel::High => "akrep-risk-high",
        RiskLevel::Critical => "akrep-risk-critical",
    }
}

pub fn risk_label(risk: RiskLevel, lang: Lang) -> &'static str {
    match risk {
        RiskLevel::Low => lang.pick("Düşük risk", "Low risk"),
        RiskLevel::Medium => lang.pick("Orta risk", "Medium risk"),
        RiskLevel::High => lang.pick("Yüksek risk", "High risk"),
        RiskLevel::Critical => lang.pick("Kritik risk", "Critical risk"),
    }
}

pub fn tx_state_css_class(state: TransactionState) -> &'static str {
    match state {
        TransactionState::Committed => "akrep-tx-committed",
        TransactionState::RolledBack => "akrep-tx-rolledback",
        TransactionState::RollbackFailed => "akrep-tx-failed",
        TransactionState::Interrupted => "akrep-tx-interrupted",
        _ => "akrep-sev-unknown",
    }
}

pub fn tx_state_label(state: TransactionState, lang: Lang) -> &'static str {
    match state {
        TransactionState::Committed => lang.pick("Uygulandı", "Applied"),
        TransactionState::RolledBack => lang.pick("Geri alındı", "Rolled back"),
        TransactionState::RollbackFailed => lang.pick("Geri alma hatası", "Rollback failed"),
        TransactionState::Interrupted => lang.pick("Yarıda kesildi", "Interrupted"),
        TransactionState::Draft => lang.pick("Taslak", "Draft"),
        TransactionState::AwaitingAuthorization => lang.pick("Onay bekliyor", "Awaiting approval"),
        TransactionState::Prepared => lang.pick("Hazırlandı", "Prepared"),
        TransactionState::Applying => lang.pick("Uygulanıyor", "Applying"),
        TransactionState::Verifying => lang.pick("Doğrulanıyor", "Verifying"),
        TransactionState::RollingBack => lang.pick("Geri alınıyor", "Rolling back"),
    }
}

pub fn domain_icon_name(id: &str) -> &'static str {
    match id {
        "threat_protection" | "threat_integrity" => "security-high-symbolic",
        "system_protection" | "selinux" => "security-medium-symbolic",
        "firewall" => "network-transmit-receive-symbolic",
        "applications" | "flatpak" => "application-x-executable-symbolic",
        "device_security" | "packages" => "computer-symbolic",
        "privacy" => "camera-web-symbolic",
        "profiles" => "preferences-system-symbolic",
        "history" => "document-open-recent-symbolic",
        "settings" => "emblem-system-symbolic",
        _ => "go-home-symbolic",
    }
}

pub fn domain_title(id: &str, lang: Lang) -> Option<&'static str> {
    match id {
        "threat_protection" | "threat_integrity" => {
            Some(lang.pick("Tehdit koruması", "Threat protection"))
        }
        "system_protection" | "selinux" => Some(lang.pick("Sistem koruması", "System protection")),
        "firewall" => Some(lang.pick("Güvenlik duvarı", "Firewall")),
        "applications" | "flatpak" => Some(lang.pick("Uygulama güvenliği", "App security")),
        "device_security" | "packages" => Some(lang.pick("Cihaz güvenliği", "Device security")),
        "privacy" => Some(lang.pick("Gizlilik", "Privacy")),
        "profiles" => Some(lang.pick("Güvenlik profilleri", "Security profiles")),
        "history" => Some(lang.pick("Koruma geçmişi", "Protection history")),
        _ => None,
    }
}

pub fn build_status_dot(dot_css_class: &str) -> GtkBox {
    let dot = GtkBox::new(Orientation::Horizontal, 0);
    dot.add_css_class("akrep-dot");
    dot.add_css_class(dot_css_class);
    dot.set_valign(Align::Center);
    dot.set_halign(Align::Center);
    dot
}

pub fn build_status_indicator(dot_css_class: &str, text: &str, muted: bool) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_valign(Align::Center);
    row.append(&build_status_dot(dot_css_class));

    let lbl = Label::new(Some(text));
    lbl.add_css_class(if muted {
        "akrep-status-text-muted"
    } else {
        "akrep-status-text"
    });
    lbl.set_valign(Align::Center);
    row.append(&lbl);
    row
}

pub fn build_severity_indicator(sev: AkrepSeverity, lang: Lang) -> GtkBox {
    let text = match lang {
        Lang::Tr => sev.label_tr(),
        Lang::En => sev.label_en(),
    };
    let muted = matches!(sev, AkrepSeverity::Unavailable | AkrepSeverity::Unknown);
    build_status_indicator(severity_css_class(sev), text, muted)
}

pub fn build_availability_indicator(avail: Availability, lang: Lang) -> GtkBox {
    let text = match lang {
        Lang::Tr => avail.label_tr(),
        Lang::En => avail.label_en(),
    };
    let muted = !matches!(avail, Availability::Available);
    build_status_indicator(availability_css_class(avail), text, muted)
}

pub fn build_risk_indicator(risk: RiskLevel, lang: Lang) -> GtkBox {
    build_status_indicator(risk_css_class(risk), risk_label(risk, lang), false)
}

pub fn build_tx_state_indicator(state: TransactionState, lang: Lang) -> GtkBox {
    let muted = matches!(
        state,
        TransactionState::RolledBack | TransactionState::Draft | TransactionState::Prepared
    );
    build_status_indicator(
        tx_state_css_class(state),
        tx_state_label(state, lang),
        muted,
    )
}

pub fn build_admin_note(lang: Lang) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 8);
    row.set_valign(Align::Center);
    let icon = Image::from_icon_name("changes-prevent-symbolic");
    icon.set_pixel_size(16);
    icon.add_css_class("akrep-icon-muted");
    row.append(&icon);

    let lbl = Label::new(Some(lang.pick(
        "Yönetici parolası istenir",
        "Administrator password required",
    )));
    lbl.add_css_class("akrep-meta");
    lbl.set_xalign(0.0);
    lbl.set_wrap(true);
    row.append(&lbl);
    row
}

pub fn build_kv_row(key: &str, val: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let k = Label::new(Some(key));
    k.add_css_class("akrep-metric-key");
    k.set_xalign(0.0);
    k.set_valign(Align::Start);
    k.set_wrap(true);
    k.set_size_request(184, -1);

    let v = Label::new(Some(val));
    v.add_css_class("akrep-metric-val");
    v.set_xalign(0.0);
    v.set_valign(Align::Start);
    v.set_wrap(true);
    v.set_wrap_mode(WrapMode::WordChar);
    v.set_hexpand(true);
    v.set_selectable(true);

    row.append(&k);
    row.append(&v);
    row
}
