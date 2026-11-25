use crate::components::{severity_css_class, severity_icon_name};
use crate::i18n::Lang;
use blaze_sentinel_core::{AkrepSeverity, AkrepSnapshot, Availability};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};

pub fn build_header_banner(
    snap: Option<&AkrepSnapshot>,
    offline_reason: Option<&str>,
    lang: Lang,
    on_refresh: impl Fn() + 'static,
    on_toggle_lang: impl Fn() + 'static,
) -> GtkBox {
    let outer = GtkBox::new(Orientation::Vertical, 10);

    let card = GtkBox::new(Orientation::Horizontal, 14);
    card.add_css_class("akrep-banner");
    card.set_valign(Align::Center);

    let (sev, avail, headline, subline) = match snap {
        Some(s) => (
            s.overall_status,
            Availability::Available,
            lang.pick(&s.headline_tr, &s.headline_en).to_string(),
            format!(
                "{}: {} • Sentinel v{} • {} {}/{}",
                lang.pick("Son denetim", "Last check"),
                &s.generated_at.chars().take(19).collect::<String>(),
                s.sentinel_version,
                lang.pick("Çekirdek Kontrol", "Core Controls"),
                s.posture.passed,
                s.posture.total
            ),
        ),
        None => (
            AkrepSeverity::Unknown,
            Availability::Unavailable,
            lang.pick(
                "Akrep Arka Plan Servisine Bağlanılamadı",
                "Akrep Background Service Offline",
            )
            .to_string(),
            lang.pick(
                "Telemetry akışı olmadan yalnızca salt okunur güvenli durum gösteriliyor.",
                "Running in degraded read-only fallback mode without telemetry daemon.",
            )
            .to_string(),
        ),
    };

    let icon = Image::from_icon_name(severity_icon_name(sev, avail));
    icon.set_pixel_size(36);
    card.append(&icon);

    let text_col = GtkBox::new(Orientation::Vertical, 4);
    text_col.set_hexpand(true);

    let top_row = GtkBox::new(Orientation::Horizontal, 8);
    let title_lbl = Label::new(Some(&headline));
    title_lbl.add_css_class("akrep-title");
    title_lbl.set_xalign(0.0);
    title_lbl.set_wrap(true);
    top_row.append(&title_lbl);

    let ro_pill = Label::new(Some(lang.pick("SALT OKUNUR v0.1", "READ-ONLY v0.1")));
    ro_pill.add_css_class("akrep-ro-pill");
    top_row.append(&ro_pill);

    let badge = Label::new(Some(match lang {
        Lang::Tr => sev.label_tr(),
        Lang::En => sev.label_en(),
    }));
    badge.add_css_class("akrep-badge");
    badge.add_css_class(severity_css_class(sev));
    top_row.append(&badge);

    text_col.append(&top_row);

    let sub_lbl = Label::new(Some(&subline));
    sub_lbl.add_css_class("akrep-subtitle");
    sub_lbl.set_xalign(0.0);
    sub_lbl.set_wrap(true);
    text_col.append(&sub_lbl);

    card.append(&text_col);

    let actions = GtkBox::new(Orientation::Horizontal, 8);
    actions.set_valign(Align::Center);

    let lang_btn = Button::with_label(match lang {
        Lang::Tr => "EN",
        Lang::En => "TR",
    });
    lang_btn.connect_clicked(move |_| on_toggle_lang());
    actions.append(&lang_btn);

    let refresh_btn = Button::with_label(lang.pick("Yenile", "Refresh"));
    refresh_btn.connect_clicked(move |_| on_refresh());
    actions.append(&refresh_btn);

    card.append(&actions);
    outer.append(&card);

    if let Some(reason) = offline_reason {
        let warn_box = GtkBox::new(Orientation::Horizontal, 10);
        warn_box.add_css_class("akrep-offline-banner");
        let w_icon = Image::from_icon_name("dialog-warning-symbolic");
        w_icon.set_pixel_size(20);
        warn_box.append(&w_icon);
        let msg = Label::new(Some(&format!(
            "{}: {}",
            lang.pick(
                "Kısıtlı / Çevrimdışı Durum (akrepd servisi yanıt vermiyor)",
                "Degraded / Offline Mode (akrepd service unreachable)",
            ),
            reason
        )));
        msg.set_xalign(0.0);
        msg.set_wrap(true);
        msg.set_hexpand(true);
        warn_box.append(&msg);
        outer.append(&warn_box);
    }

    outer
}
