use crate::dbus_client::{AKREP_BUS_NAME, AKREP_INTERFACE_NAME, AKREP_OBJECT_PATH};
use crate::i18n::Lang;
use blaze_sentinel_core::{AkrepSnapshot, AKREP_SCHEMA_VERSION, AKREP_VERSION, SENTINEL_VERSION};
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build_settings_page(snap: Option<&AkrepSnapshot>, lang: Lang) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);

    let title = Label::new(Some(lang.pick(
        "Akrep v0.1 Mimari & Salt Okunur Ayarlar",
        "Akrep v0.1 Architecture & Read-Only Scope",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    root.append(&title);

    let info_box = GtkBox::new(Orientation::Vertical, 8);
    info_box.add_css_class("akrep-detail-box");

    let rows: Vec<(&str, &str, String)> = vec![
        ("Akrep Sürümü", "Akrep Version", format!("v{} (Schema {})", AKREP_VERSION, AKREP_SCHEMA_VERSION)),
        ("Paylaşılan Çekirdek", "Shared Core Library", format!("blaze-sentinel-core / Sentinel v{}", SENTINEL_VERSION)),
        ("Çalışma Kipi", "Operating Mode", lang.pick("Salt Okunur (Mutatörler Kapalı)", "Strictly Read-Only (Mutators Disabled)").to_string()),
        ("D-Bus Servisi", "System D-Bus Endpoint", format!("{} ({} / {})", AKREP_BUS_NAME, AKREP_OBJECT_PATH, AKREP_INTERFACE_NAME)),
        ("Polkit Eylem İskeleti", "Polkit Policy Skeleton", lang.pick("tr.com.blazeos.Akrep1.manage (v0.1'de devre dışı)", "tr.com.blazeos.Akrep1.manage (inactive in v0.1)").to_string()),
        (
            "Daemon Bağlantı Durumu",
            "Daemon Connection State",
            match snap {
                Some(s) => format!("Bağlı (mutators_enabled={})", s.mutators_enabled),
                None => lang.pick("Çevrimdışı / Kısıtlı görünüm", "Offline / Degraded view").to_string(),
            },
        ),
    ];

    for (l_tr, l_en, val) in rows {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        let k = Label::new(Some(lang.pick(l_tr, l_en)));
        k.add_css_class("akrep-metric-key");
        k.set_xalign(0.0);
        k.set_size_request(240, -1);

        let v = Label::new(Some(&val));
        v.add_css_class("akrep-metric-val");
        v.set_xalign(0.0);
        v.set_wrap(true);
        v.set_hexpand(true);

        row.append(&k);
        row.append(&v);
        info_box.append(&row);
    }

    root.append(&info_box);
    root
}
