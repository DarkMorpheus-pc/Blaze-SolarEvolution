use crate::i18n::Lang;
use blaze_sentinel_core::AKREP_VERSION;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use std::rc::Rc;

pub const NAV_ITEMS: &[(&str, &str, &str, &str)] = &[
    ("overview", "Genel Bakış", "Overview", "go-home-symbolic"),
    (
        "threat_protection",
        "Tehdit Koruması",
        "Threat Protection",
        "security-high-symbolic",
    ),
    (
        "system_protection",
        "Sistem Koruması",
        "System Protection",
        "security-medium-symbolic",
    ),
    (
        "firewall",
        "Güvenlik Duvarı",
        "Firewall",
        "network-transmit-receive-symbolic",
    ),
    (
        "applications",
        "Uygulama Güvenliği",
        "Application Security",
        "application-x-executable-symbolic",
    ),
    (
        "device_security",
        "Cihaz Güvenliği",
        "Device Security",
        "computer-symbolic",
    ),
    ("privacy", "Gizlilik", "Privacy", "camera-web-symbolic"),
    (
        "profiles",
        "Güvenlik Profilleri",
        "Security Profiles",
        "preferences-system-symbolic",
    ),
    (
        "history",
        "Koruma Geçmişi",
        "Protection History",
        "document-open-recent-symbolic",
    ),
    ("settings", "Ayarlar", "Settings", "emblem-system-symbolic"),
];

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
    rail.set_size_request(248, -1);

    let brand_box = GtkBox::new(Orientation::Horizontal, 8);
    brand_box.set_margin_bottom(12);
    brand_box.set_margin_start(6);
    let logo = Image::from_icon_name("security-high-symbolic");
    logo.set_pixel_size(24);
    let brand_col = GtkBox::new(Orientation::Vertical, 1);
    let brand_lbl = Label::new(Some(&format!("Akrep v{}", AKREP_VERSION)));
    brand_lbl.add_css_class("akrep-card-title");
    brand_lbl.set_xalign(0.0);
    let sub_lbl = Label::new(Some(
        lang.pick("BlazeOS Güvenlik Merkezi", "BlazeOS Security Center"),
    ));
    sub_lbl.add_css_class("akrep-subtitle");
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

        let row = GtkBox::new(Orientation::Horizontal, 10);
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(16);
        let lbl = Label::new(Some(lang.pick(title_tr, title_en)));
        lbl.set_xalign(0.0);
        row.append(&icon);
        row.append(&lbl);
        btn.set_child(Some(&row));

        let cb = Rc::clone(&on_select);
        btn.connect_clicked(move |_| cb(id));
        rail.append(&btn);
    }

    rail
}
