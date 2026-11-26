use crate::i18n::Lang;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use std::rc::Rc;

pub const NAV_ITEMS: &[(&str, &str, &str, &str)] = &[
    ("overview", "Genel bakış", "Overview", "go-home-symbolic"),
    (
        "threat_protection",
        "Tehdit koruması",
        "Threat protection",
        "security-high-symbolic",
    ),
    (
        "system_protection",
        "Sistem koruması",
        "System protection",
        "security-medium-symbolic",
    ),
    (
        "firewall",
        "Güvenlik duvarı",
        "Firewall",
        "network-transmit-receive-symbolic",
    ),
    (
        "applications",
        "Uygulama güvenliği",
        "App security",
        "application-x-executable-symbolic",
    ),
    (
        "device_security",
        "Cihaz güvenliği",
        "Device security",
        "computer-symbolic",
    ),
    ("privacy", "Gizlilik", "Privacy", "camera-web-symbolic"),
    (
        "profiles",
        "Güvenlik profilleri",
        "Security profiles",
        "preferences-system-symbolic",
    ),
    (
        "history",
        "Koruma geçmişi",
        "Protection history",
        "document-open-recent-symbolic",
    ),
    ("settings", "Ayarlar", "Settings", "emblem-system-symbolic"),
];

pub fn nav_title_for_tab(tab_id: &str, lang: Lang) -> &'static str {
    if let Some(&(_, tr, en, _)) = NAV_ITEMS.iter().find(|&&(id, _, _, _)| id == tab_id) {
        lang.pick(tr, en)
    } else {
        lang.pick("Genel bakış", "Overview")
    }
}

pub fn map_card_id_to_tab(card_id: &str) -> &'static str {
    match card_id {
        "threat_protection" | "threat_integrity" => "threat_protection",
        "system_protection" | "selinux" => "system_protection",
        "firewall" => "firewall",
        "applications" | "flatpak" => "applications",
        "device_security" | "packages" => "device_security",
        "privacy" => "privacy",
        "profiles" => "profiles",
        "history" => "history",
        "settings" => "settings",
        _ => "overview",
    }
}

pub fn build_nav_rail(active_tab: &str, lang: Lang, on_select: Rc<dyn Fn(&'static str)>) -> GtkBox {
    let rail = GtkBox::new(Orientation::Vertical, 4);
    rail.add_css_class("akrep-rail");
    rail.set_size_request(220, -1);

    let brand_box = GtkBox::new(Orientation::Horizontal, 8);
    brand_box.set_margin_bottom(16);
    brand_box.set_margin_start(8);
    brand_box.set_margin_end(8);
    brand_box.set_valign(Align::Center);

    let logo = Image::from_icon_name("security-high-symbolic");
    logo.set_pixel_size(16);
    logo.add_css_class("akrep-icon-secondary");
    logo.set_valign(Align::Center);

    let brand_col = GtkBox::new(Orientation::Vertical, 0);
    let brand_lbl = Label::new(Some("Akrep"));
    brand_lbl.add_css_class("akrep-brand-title");
    brand_lbl.set_xalign(0.0);

    let sub_lbl = Label::new(Some(lang.pick("Güvenlik merkezi", "Security center")));
    sub_lbl.add_css_class("akrep-brand-sub");
    sub_lbl.set_xalign(0.0);

    brand_col.append(&brand_lbl);
    brand_col.append(&sub_lbl);
    brand_box.append(&logo);
    brand_box.append(&brand_col);
    rail.append(&brand_box);

    for &(id, title_tr, title_en, icon_name) in NAV_ITEMS {
        let btn = Button::new();
        btn.add_css_class("akrep-nav-btn");
        if id == active_tab {
            btn.add_css_class("akrep-nav-btn-active");
        }
        if id == "settings" {
            btn.set_vexpand(true);
            btn.set_valign(Align::End);
        }

        let row = GtkBox::new(Orientation::Horizontal, 8);
        row.set_hexpand(true);
        row.set_halign(Align::Fill);
        row.set_valign(Align::Center);

        let indicator = GtkBox::new(Orientation::Vertical, 0);
        indicator.add_css_class("akrep-nav-indicator");
        indicator.set_valign(Align::Center);
        row.append(&indicator);

        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        icon.set_valign(Align::Center);
        row.append(&icon);

        let lbl = Label::new(Some(lang.pick(title_tr, title_en)));
        lbl.set_xalign(0.0);
        lbl.set_hexpand(true);
        lbl.set_ellipsize(EllipsizeMode::End);
        row.append(&lbl);

        btn.set_child(Some(&row));

        let cb = Rc::clone(&on_select);
        btn.connect_clicked(move |_| cb(id));
        rail.append(&btn);
    }

    rail
}
