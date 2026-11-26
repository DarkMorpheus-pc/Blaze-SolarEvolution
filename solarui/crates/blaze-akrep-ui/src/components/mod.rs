pub mod finding_row;
pub mod header_banner;
pub mod nav_rail;
pub mod plan_dialog;
pub mod status_card;

use crate::i18n::Lang;
use blaze_akrep_common::{OverrideDuration, RiskLevel, TransactionState};
use blaze_sentinel_core::{ActiveOverrideEntry, AkrepSeverity, Availability};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, FlowBox, Image, Label, Orientation, SelectionMode};
use std::cell::RefCell;
use std::rc::Rc;

/// Choice entry for a segmented protection override duration selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationChoice {
    pub label_tr: &'static str,
    pub label_en: &'static str,
    pub duration: OverrideDuration,
}

/// Duration choices for SELinux and Firewalld overrides.
pub const PROTECTION_DURATION_CHOICES: [DurationChoice; 4] = [
    DurationChoice {
        label_tr: "15 dk",
        label_en: "15 min",
        duration: OverrideDuration::Timed { minutes: 15 },
    },
    DurationChoice {
        label_tr: "1 saat",
        label_en: "1 hour",
        duration: OverrideDuration::Timed { minutes: 60 },
    },
    DurationChoice {
        label_tr: "Yeniden başlatmaya kadar",
        label_en: "Until reboot",
        duration: OverrideDuration::UntilReboot,
    },
    DurationChoice {
        label_tr: "Ben tekrar değiştirene kadar",
        label_en: "Until I change it",
        duration: OverrideDuration::UntilChanged,
    },
];

/// Duration choices for `systemd-oomd` build-workload pauses.
pub const OOMD_DURATION_CHOICES: [DurationChoice; 4] = [
    DurationChoice {
        label_tr: "1 saat",
        label_en: "1 hour",
        duration: OverrideDuration::Timed { minutes: 60 },
    },
    DurationChoice {
        label_tr: "4 saat",
        label_en: "4 hours",
        duration: OverrideDuration::Timed { minutes: 240 },
    },
    DurationChoice {
        label_tr: "Yeniden başlatmaya kadar",
        label_en: "Until reboot",
        duration: OverrideDuration::UntilReboot,
    },
    DurationChoice {
        label_tr: "Ben tekrar değiştirene kadar",
        label_en: "Until I change it",
        duration: OverrideDuration::UntilChanged,
    },
];

/// Maps a selected duration index safely back to `OverrideDuration`.
pub fn duration_from_choice_idx(choices: &[DurationChoice], idx: usize) -> OverrideDuration {
    choices.get(idx).unwrap_or(&choices[0]).duration
}

/// Formats an `OverrideDuration` cleanly for plan previews and transaction history.
pub fn format_duration_badge(duration: OverrideDuration, lang: Lang) -> String {
    match duration {
        OverrideDuration::Timed { minutes } if minutes >= 60 && minutes % 60 == 0 => {
            let hours = minutes / 60;
            if lang == Lang::Tr {
                format!("{} saat", hours)
            } else if hours == 1 {
                "1 hour".to_string()
            } else {
                format!("{} hours", hours)
            }
        }
        OverrideDuration::Timed { minutes } => format!("{} {}", minutes, lang.pick("dk", "min")),
        OverrideDuration::UntilReboot => {
            lang.pick("Yeniden başlatmaya kadar", "Until reboot").to_string()
        }
        OverrideDuration::UntilChanged => lang
            .pick("Ben tekrar değiştirene kadar", "Until manually enabled")
            .to_string(),
    }
}

/// Formats live remaining countdown or persistence mode for an active override entry.
pub fn format_override_status_text(entry: &ActiveOverrideEntry, lang: Lang) -> String {
    match entry.duration_kind.as_str() {
        "timed" => {
            let rem_secs = entry
                .remaining_secs
                .unwrap_or_else(|| i64::from(entry.minutes.unwrap_or(1)) * 60)
                .max(1);
            if rem_secs >= 60 {
                let mins = (rem_secs + 59) / 60;
                format!("{} {}", mins, lang.pick("dk kaldı", "min remaining"))
            } else {
                format!("{} {}", rem_secs, lang.pick("sn kaldı", "s remaining"))
            }
        }
        "until_reboot" => {
            lang.pick("Yeniden başlatmaya kadar", "Until reboot").to_string()
        }
        _ => lang
            .pick("Elle açılana kadar", "Until manually enabled")
            .to_string(),
    }
}

/// Returns a localized display name for an active override subsystem.
pub fn override_subsystem_label(subsystem: &str, lang: Lang) -> &'static str {
    match subsystem {
        "selinux" => lang.pick("SELinux (Permissive)", "SELinux (Permissive)"),
        "firewalld" => lang.pick("Güvenlik duvarı (firewalld)", "Firewall (firewalld)"),
        "oomd" => lang.pick(
            "Bellek yöneticisi (systemd-oomd)",
            "Memory manager (systemd-oomd)",
        ),
        _ => lang.pick("Sistem koruması", "System protection"),
    }
}

/// Returns a short header summary string when one or more overrides are active.
pub fn compact_override_summary(overrides: &[ActiveOverrideEntry], lang: Lang) -> String {
    if overrides.len() == 1 {
        let ov = &overrides[0];
        let short_name = match ov.subsystem.as_str() {
            "selinux" => "SELinux",
            "firewalld" => "Firewalld",
            "oomd" => "oomd",
            _ => lang.pick("Koruma", "Guard"),
        };
        format!(
            "{} · {}",
            short_name,
            format_override_status_text(ov, lang)
        )
    } else {
        format!(
            "{} {}",
            overrides.len(),
            lang.pick("etkin gevşetme", "active overrides")
        )
    }
}

/// Builds a wrapping FlowBox segmented selector so 4 duration choices never clip at 1060x720.
pub fn build_duration_selector(
    choices: &'static [DurationChoice],
    selected_idx: usize,
    lang: Lang,
    on_select: Rc<dyn Fn(usize)>,
) -> FlowBox {
    let flow = FlowBox::new();
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_column_spacing(8);
    flow.set_row_spacing(8);
    flow.set_max_children_per_line(4);
    flow.set_min_children_per_line(1);
    flow.set_hexpand(true);

    let clamped_idx = if choices.is_empty() {
        0
    } else {
        selected_idx.min(choices.len() - 1)
    };

    let buttons: Rc<RefCell<Vec<Button>>> =
        Rc::new(RefCell::new(Vec::with_capacity(choices.len())));

    for (idx, choice) in choices.iter().enumerate() {
        let btn = Button::with_label(lang.pick(choice.label_tr, choice.label_en));
        btn.add_css_class(if idx == clamped_idx {
            "akrep-seg-active"
        } else {
            "akrep-seg-btn"
        });
        buttons.borrow_mut().push(btn.clone());
        flow.insert(&btn, -1);
        if let Some(fc) = btn.parent() {
            fc.set_can_focus(false);
        }
    }

    for (idx, btn) in buttons.borrow().iter().enumerate() {
        let btns_ref = Rc::clone(&buttons);
        let cb = Rc::clone(&on_select);
        btn.connect_clicked(move |_| {
            for (i, b) in btns_ref.borrow().iter().enumerate() {
                if i == idx {
                    b.remove_css_class("akrep-seg-btn");
                    b.add_css_class("akrep-seg-active");
                } else {
                    b.remove_css_class("akrep-seg-active");
                    b.add_css_class("akrep-seg-btn");
                }
            }
            cb(idx);
        });
    }

    flow
}

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
