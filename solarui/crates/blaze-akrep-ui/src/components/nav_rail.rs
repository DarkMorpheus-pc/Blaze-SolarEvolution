use crate::i18n::Lang;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use std::rc::Rc;

pub const NAV_ITEMS: &[(&str, &str, &str, &str)] = &[
    ("home", "Giriş", "Home", "go-home-symbolic"),
    ("selinux", "Sistem Koruması (SELinux)", "System Protection (SELinux)", "security-high-symbolic"),
    ("firewall", "Güvenlik Duvarı", "Firewall & Network", "network-workgroup-symbolic"),
    ("flatpak", "Uygulama İzinleri", "App Permissions (Flatpak)", "applications-system-symbolic"),
    ("device_security", "Cihaz Güvenliği", "Device Security", "computer-symbolic"),
    ("threat_integrity", "Tehdit & Bütünlük", "Threat & Integrity", "dialog-information-symbolic"),
    ("packages", "Paket & İmza Bütünlüğü", "Package Integrity", "package-x-generic-symbolic"),
    ("history", "Koruma Geçmişi", "Protection History", "document-open-recent-symbolic"),
    ("settings", "Ayarlar & Kapsam", "Settings & Scope", "emblem-system-symbolic"),
];

pub fn build_nav_rail(
    active_tab: &str,
    lang: Lang,
    on_select: Rc<dyn Fn(&'static str)>,
) -> GtkBox {
    let rail = GtkBox::new(Orientation::Vertical, 4);
    rail.add_css_class("akrep-rail");
    rail.set_size_request(248, -1);

    let brand_box = GtkBox::new(Orientation::Horizontal, 8);
    brand_box.set_margin_bottom(12);
    brand_box.set_margin_start(6);
    let logo = Image::from_icon_name("security-high-symbolic");
    logo.set_pixel_size(24);
    let brand_lbl = Label::new(Some("Akrep v0.1"));
    brand_lbl.add_css_class("akrep-card-title");
    brand_box.append(&logo);
    brand_box.append(&brand_lbl);
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
