pub mod finding_row;
pub mod header_banner;
pub mod nav_rail;
pub mod plan_dialog;
pub mod status_card;

use crate::i18n::Lang;
use blaze_akrep_common::{RiskLevel, TransactionState};
use blaze_sentinel_core::{AkrepSeverity, Availability};
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

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

pub fn severity_icon_name(sev: AkrepSeverity, avail: Availability) -> &'static str {
    match avail {
        Availability::Unavailable => return "dialog-information-symbolic",
        Availability::Unknown => return "dialog-question-symbolic",
        Availability::Available => {}
    }
    match sev {
        AkrepSeverity::Protected => "security-high-symbolic",
        AkrepSeverity::Attention => "security-medium-symbolic",
        AkrepSeverity::Critical => "dialog-error-symbolic",
        AkrepSeverity::Unavailable => "dialog-information-symbolic",
        AkrepSeverity::Unknown => "dialog-question-symbolic",
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
        RiskLevel::Low => lang.pick("Düşük Risk", "Low Risk"),
        RiskLevel::Medium => lang.pick("Orta Risk", "Medium Risk"),
        RiskLevel::High => lang.pick("Yüksek Risk", "High Risk"),
        RiskLevel::Critical => lang.pick("Kritik Risk", "Critical Risk"),
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
        TransactionState::Committed => lang.pick("Onaylandı (Committed)", "Committed"),
        TransactionState::RolledBack => lang.pick("Geri Alındı (RolledBack)", "RolledBack"),
        TransactionState::RollbackFailed => lang.pick("Geri Alma Hatası", "RollbackFailed"),
        TransactionState::Interrupted => lang.pick("Yarıda Kesildi (Interrupted)", "Interrupted"),
        TransactionState::Draft => lang.pick("Taslak", "Draft"),
        TransactionState::AwaitingAuthorization => lang.pick("Yetki Bekliyor", "Awaiting Auth"),
        TransactionState::Prepared => lang.pick("Hazırlandı", "Prepared"),
        TransactionState::Applying => lang.pick("Uygulanıyor", "Applying"),
        TransactionState::Verifying => lang.pick("Doğrulanıyor", "Verifying"),
        TransactionState::RollingBack => lang.pick("Geri Alınıyor", "Rolling Back"),
    }
}

pub fn build_kv_row(key: &str, val: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    let k = Label::new(Some(key));
    k.add_css_class("akrep-metric-key");
    k.set_xalign(0.0);
    k.set_size_request(240, -1);

    let v = Label::new(Some(val));
    v.add_css_class("akrep-metric-val");
    v.set_xalign(0.0);
    v.set_wrap(true);
    v.set_hexpand(true);
    v.set_selectable(true);

    row.append(&k);
    row.append(&v);
    row
}
