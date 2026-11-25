use crate::components::{severity_css_class, severity_icon_name};
use crate::i18n::Lang;
use crate::state::{NoticeBanner, NoticeKind};
use blaze_sentinel_core::{AkrepSeverity, AkrepSnapshot, Availability};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};

pub struct HeaderBannerCallbacks {
    pub on_refresh: Box<dyn Fn()>,
    pub on_toggle_lang: Box<dyn Fn()>,
    pub on_open_profiles: Box<dyn Fn()>,
    pub on_dismiss_notice: Box<dyn Fn()>,
}

pub fn build_header_banner(
    snap: Option<&AkrepSnapshot>,
    active_profile_tr_en: (&str, &str),
    offline_reason: Option<&str>,
    notice: Option<&NoticeBanner>,
    lang: Lang,
    callbacks: HeaderBannerCallbacks,
) -> GtkBox {
    let HeaderBannerCallbacks {
        on_refresh,
        on_toggle_lang,
        on_open_profiles,
        on_dismiss_notice,
    } = callbacks;
    let outer = GtkBox::new(Orientation::Vertical, 8);

    let card = GtkBox::new(Orientation::Horizontal, 14);
    card.add_css_class("akrep-banner");
    card.set_valign(Align::Center);

    let profile_lbl = format!(
        "{}: {}",
        lang.pick("Profil", "Profile"),
        lang.pick(active_profile_tr_en.0, active_profile_tr_en.1)
    );

    let (sev, avail, headline, subline) = match snap {
        Some(s) => (
            s.overall_status,
            Availability::Available,
            lang.pick(&s.headline_tr, &s.headline_en).to_string(),
            format!(
                "{}: {} • Sentinel v{} • {} {}/{}",
                lang.pick("Son denetim", "Last check"),
                s.generated_at.chars().take(19).collect::<String>(),
                s.sentinel_version,
                lang.pick("Çekirdek Kontrol", "Core Controls"),
                s.posture.passed,
                s.posture.total
            ),
        ),
        None => (
            AkrepSeverity::Unavailable,
            Availability::Unavailable,
            lang.pick(
                "Akrep Telemetri Servisi Çevrimdışı",
                "Akrep Telemetry Service Offline",
            )
            .to_string(),
            lang.pick(
                "akrepd yanıt vermiyor; yerel oturum ve Polkit işlem kanalları bağımsız çalışmaya devam eder.",
                "akrepd is unreachable; local user-session and Polkit transaction channels remain independently available.",
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

    let badge = Label::new(Some(match lang {
        Lang::Tr => sev.label_tr(),
        Lang::En => sev.label_en(),
    }));
    badge.add_css_class("akrep-badge");
    badge.add_css_class(severity_css_class(sev));
    top_row.append(&badge);

    let prof_btn = Button::with_label(&profile_lbl);
    prof_btn.add_css_class("akrep-profile-pill");
    prof_btn.connect_clicked(move |_| on_open_profiles());
    top_row.append(&prof_btn);

    let arch_pill = Label::new(Some(
        lang.pick("ÇİFT KANAL (RO + POLKIT)", "DUAL-BUS (RO + POLKIT)"),
    ));
    arch_pill.add_css_class("akrep-ro-pill");
    top_row.append(&arch_pill);

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
    lang_btn.add_css_class("akrep-ghost-btn");
    lang_btn.connect_clicked(move |_| on_toggle_lang());
    actions.append(&lang_btn);

    let refresh_btn = Button::with_label(lang.pick("Yenile", "Refresh"));
    refresh_btn.add_css_class("akrep-ghost-btn");
    refresh_btn.connect_clicked(move |_| on_refresh());
    actions.append(&refresh_btn);

    card.append(&actions);
    outer.append(&card);

    if let Some(reason) = offline_reason {
        let warn_box = GtkBox::new(Orientation::Horizontal, 10);
        warn_box.add_css_class("akrep-offline-banner");
        let w_icon = Image::from_icon_name("dialog-warning-symbolic");
        w_icon.set_pixel_size(18);
        warn_box.append(&w_icon);
        let msg = Label::new(Some(&format!(
            "{}: {}",
            lang.pick(
                "Kısıtlı Telemetri Durumu (akrepd)",
                "Degraded Telemetry State (akrepd)",
            ),
            reason
        )));
        msg.set_xalign(0.0);
        msg.set_wrap(true);
        msg.set_hexpand(true);
        warn_box.append(&msg);
        outer.append(&warn_box);
    }

    if let Some(nt) = notice {
        let toast_box = GtkBox::new(Orientation::Horizontal, 10);
        let (cls, ico) = match nt.kind {
            NoticeKind::Success => ("akrep-toast-ok", "security-high-symbolic"),
            NoticeKind::Error => ("akrep-toast-err", "dialog-error-symbolic"),
            NoticeKind::Info => ("akrep-toast-info", "dialog-information-symbolic"),
        };
        toast_box.add_css_class(cls);
        let icon_w = Image::from_icon_name(ico);
        icon_w.set_pixel_size(18);
        toast_box.append(&icon_w);

        let lbl = Label::new(Some(lang.pick(&nt.message_tr, &nt.message_en)));
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        lbl.set_hexpand(true);
        toast_box.append(&lbl);

        let close_btn = Button::with_label(lang.pick("Kapat", "Dismiss"));
        close_btn.add_css_class("akrep-ghost-btn");
        close_btn.connect_clicked(move |_| on_dismiss_notice());
        toast_box.append(&close_btn);

        outer.append(&toast_box);
    }

    outer
}
