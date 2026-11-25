use crate::components::finding_row::build_finding_row;
use crate::components::status_card::build_status_card;
use crate::i18n::Lang;
use blaze_sentinel_core::AkrepSnapshot;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, FlowBox, Label, Orientation, SelectionMode};
use std::rc::Rc;

pub fn build_home_page(
    snap: Option<&AkrepSnapshot>,
    lang: Lang,
    on_select_tab: Rc<dyn Fn(&'static str)>,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 16);

    let intro = Label::new(Some(lang.pick(
        "Cihazınızın güvenliği ve sistem bütünlüğüyle ilgili anlık görünüm (Salt Okunur v0.1).",
        "Live diagnostic overview of your system security posture and integrity (Read-Only v0.1).",
    )));
    intro.add_css_class("akrep-subtitle");
    intro.set_xalign(0.0);
    intro.set_wrap(true);
    root.append(&intro);

    let Some(s) = snap else {
        let empty = GtkBox::new(Orientation::Vertical, 6);
        empty.add_css_class("akrep-detail-box");
        let lbl = Label::new(Some(lang.pick(
            "akrepd D-Bus servisi bekleniyor veya durduruldu. Yukarıdaki 'Yenile' düğmesini kullanabilir ya da servisi kontrol edebilirsiniz (`systemctl status akrepd`).",
            "akrepd system service is unavailable or stopped. Use Refresh above or check `systemctl status akrepd`.",
        )));
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        empty.append(&lbl);
        root.append(&empty);
        return root;
    };

    let flow = FlowBox::new();
    flow.set_valign(gtk4::Align::Start);
    flow.set_halign(gtk4::Align::Fill);
    flow.set_max_children_per_line(3);
    flow.set_min_children_per_line(2);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);

    for card in &s.cards {
        let cb = Rc::clone(&on_select_tab);
        let btn = build_status_card(card, lang, move |section_id| {
            let static_id: &'static str = match section_id.as_str() {
                "selinux" => "selinux",
                "firewall" => "firewall",
                "flatpak" => "flatpak",
                "device_security" => "device_security",
                "threat_integrity" => "threat_integrity",
                "packages" => "packages",
                "history" => "history",
                _ => "home",
            };
            cb(static_id);
        });
        flow.insert(&btn, -1);
    }
    root.append(&flow);

    let f_hdr = Label::new(Some(lang.pick("Etkin Bulgular & Öneriler", "Active Findings & Advisories")));
    f_hdr.add_css_class("akrep-card-title");
    f_hdr.set_xalign(0.0);
    f_hdr.set_margin_top(6);
    root.append(&f_hdr);

    if s.findings.is_empty() {
        let clean = GtkBox::new(Orientation::Vertical, 4);
        clean.add_css_class("akrep-detail-box");
        let lbl = Label::new(Some(lang.pick(
            "Herhangi bir kritik veya orta düzey güvenlik uyarısı tespit edilmedi.",
            "No security warnings or integrity findings detected.",
        )));
        lbl.set_xalign(0.0);
        clean.append(&lbl);
        root.append(&clean);
    } else {
        for f in &s.findings {
            root.append(&build_finding_row(f, lang));
        }
    }

    root
}
