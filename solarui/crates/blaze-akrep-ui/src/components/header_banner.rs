use crate::components::nav_rail::nav_title_for_tab;
use crate::components::{build_severity_indicator, build_status_dot};
use crate::i18n::Lang;
use crate::state::{NoticeBanner, NoticeKind};
use blaze_sentinel_core::{AkrepSeverity, AkrepSnapshot};
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};

pub struct HeaderBannerCallbacks {
    pub on_refresh: Box<dyn Fn()>,
    pub on_toggle_lang: Box<dyn Fn()>,
    pub on_open_profiles: Box<dyn Fn()>,
    pub on_dismiss_notice: Box<dyn Fn()>,
}

pub fn build_header_banner(
    active_tab: &str,
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

    let outer = GtkBox::new(Orientation::Vertical, 0);

    let topbar = GtkBox::new(Orientation::Horizontal, 12);
    topbar.add_css_class("akrep-topbar");
    topbar.set_valign(Align::Center);

    let page_title = Label::new(Some(nav_title_for_tab(active_tab, lang)));
    page_title.add_css_class("akrep-title");
    page_title.set_xalign(0.0);
    page_title.set_hexpand(true);
    page_title.set_ellipsize(EllipsizeMode::End);
    page_title.set_valign(Align::Center);
    topbar.append(&page_title);

    let right = GtkBox::new(Orientation::Horizontal, 12);
    right.set_valign(Align::Center);

    let sev = snap
        .map(|s| s.overall_status)
        .unwrap_or(AkrepSeverity::Unavailable);
    right.append(&build_severity_indicator(sev, lang));

    let profile_lbl = format!(
        "{}: {}",
        lang.pick("Profil", "Profile"),
        lang.pick(active_profile_tr_en.0, active_profile_tr_en.1)
    );
    let prof_btn = Button::with_label(&profile_lbl);
    prof_btn.add_css_class("akrep-tag-btn");
    prof_btn.set_valign(Align::Center);
    prof_btn.connect_clicked(move |_| on_open_profiles());
    right.append(&prof_btn);

    let lang_btn = Button::with_label(match lang {
        Lang::Tr => "EN",
        Lang::En => "TR",
    });
    lang_btn.add_css_class("akrep-tag-btn");
    lang_btn.set_valign(Align::Center);
    lang_btn.set_tooltip_text(Some(lang.pick("Dili değiştir", "Switch language")));
    lang_btn.connect_clicked(move |_| on_toggle_lang());
    right.append(&lang_btn);

    let refresh_btn = Button::new();
    refresh_btn.add_css_class("akrep-icon-btn");
    refresh_btn.set_valign(Align::Center);
    refresh_btn.set_tooltip_text(Some(lang.pick("Yenile", "Refresh")));
    let refresh_icon = Image::from_icon_name("view-refresh-symbolic");
    refresh_icon.set_pixel_size(16);
    refresh_btn.set_child(Some(&refresh_icon));
    refresh_btn.connect_clicked(move |_| on_refresh());
    right.append(&refresh_btn);

    topbar.append(&right);
    outer.append(&topbar);

    if offline_reason.is_some() || notice.is_some() {
        let notices_wrap = GtkBox::new(Orientation::Vertical, 8);
        notices_wrap.add_css_class("akrep-topbar-notices");

        if let Some(reason) = offline_reason {
            let warn_box = GtkBox::new(Orientation::Horizontal, 12);
            warn_box.add_css_class("akrep-offline-banner");
            warn_box.set_valign(Align::Center);
            warn_box.append(&build_status_dot("akrep-sev-attention"));

            let msg = Label::new(Some(&format!(
                "{}: {}",
                lang.pick("Telemetri servisi çevrimdışı", "Telemetry service offline"),
                reason
            )));
            msg.add_css_class("akrep-subtitle");
            msg.set_xalign(0.0);
            msg.set_wrap(true);
            msg.set_wrap_mode(WrapMode::WordChar);
            msg.set_hexpand(true);
            warn_box.append(&msg);
            notices_wrap.append(&warn_box);
        }

        if let Some(nt) = notice {
            let toast_box = GtkBox::new(Orientation::Horizontal, 12);
            toast_box.set_valign(Align::Center);
            let (cls, dot_cls) = match nt.kind {
                NoticeKind::Success => ("akrep-toast-ok", "akrep-sev-protected"),
                NoticeKind::Error => ("akrep-toast-err", "akrep-sev-critical"),
                NoticeKind::Info => ("akrep-toast-info", "akrep-sev-unavailable"),
            };
            toast_box.add_css_class(cls);
            toast_box.append(&build_status_dot(dot_cls));

            let lbl = Label::new(Some(lang.pick(&nt.message_tr, &nt.message_en)));
            lbl.add_css_class("akrep-subtitle");
            lbl.set_xalign(0.0);
            lbl.set_wrap(true);
            lbl.set_wrap_mode(WrapMode::WordChar);
            lbl.set_hexpand(true);
            toast_box.append(&lbl);

            let close_btn = Button::with_label(lang.pick("Kapat", "Dismiss"));
            close_btn.add_css_class("akrep-tag-btn");
            close_btn.set_valign(Align::Center);
            close_btn.connect_clicked(move |_| on_dismiss_notice());
            toast_box.append(&close_btn);

            notices_wrap.append(&toast_box);
        }

        outer.append(&notices_wrap);
    }

    outer
}
