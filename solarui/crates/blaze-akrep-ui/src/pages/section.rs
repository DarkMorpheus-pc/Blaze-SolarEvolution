use crate::components::finding_row::build_finding_row;
use crate::components::severity_css_class;
use crate::i18n::Lang;
use blaze_sentinel_core::AkrepSnapshot;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build_section_page(
    snap: Option<&AkrepSnapshot>,
    section_id: &str,
    lang: Lang,
) -> GtkBox {
    let root = GtkBox::new(Orientation::Vertical, 12);

    let Some(s) = snap else {
        let box_el = GtkBox::new(Orientation::Vertical, 6);
        box_el.add_css_class("akrep-detail-box");
        let lbl = Label::new(Some(lang.pick(
            "Bu bölüm için akrepd servis verisi bekleniyor (Salt Okunur mod).",
            "Waiting for akrepd telemetry for this section (Read-Only mode).",
        )));
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        box_el.append(&lbl);
        root.append(&box_el);
        return root;
    };

    let Some(sec) = s.sections.iter().find(|sec| sec.section == section_id) else {
        let lbl = Label::new(Some("Bölüm bulunamadı."));
        root.append(&lbl);
        return root;
    };

    let hdr = GtkBox::new(Orientation::Horizontal, 8);
    let title = Label::new(Some(lang.pick(&sec.title_tr, &sec.title_en)));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr.append(&title);

    let sev_pill = Label::new(Some(match lang {
        Lang::Tr => sec.status.label_tr(),
        Lang::En => sec.status.label_en(),
    }));
    sev_pill.add_css_class("akrep-badge");
    sev_pill.add_css_class(severity_css_class(sec.status));
    hdr.append(&sev_pill);
    root.append(&hdr);

    let summary = Label::new(Some(lang.pick(&sec.summary_tr, &sec.summary_en)));
    summary.add_css_class("akrep-subtitle");
    summary.set_xalign(0.0);
    summary.set_wrap(true);
    root.append(&summary);

    let notice = GtkBox::new(Orientation::Horizontal, 8);
    notice.add_css_class("akrep-banner");
    let n_lbl = Label::new(Some(lang.pick(
        "Akrep v0.1 bu görünümde yalnızca sistem durumunu okur. Hiçbir SELinux, Firewalld veya Flatpak kuralı otomatik değiştirilmez.",
        "Akrep v0.1 strictly inspects telemetry in this view. No SELinux, Firewalld, or Flatpak state is modified.",
    )));
    n_lbl.set_xalign(0.0);
    n_lbl.set_wrap(true);
    notice.append(&n_lbl);
    root.append(&notice);

    let metrics_box = GtkBox::new(Orientation::Vertical, 8);
    metrics_box.add_css_class("akrep-detail-box");
    for kv in &sec.metrics {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        let k = Label::new(Some(lang.pick(&kv.label_tr, &kv.label_en)));
        k.add_css_class("akrep-metric-key");
        k.set_xalign(0.0);
        k.set_size_request(240, -1);

        let v = Label::new(Some(&kv.value));
        v.add_css_class("akrep-metric-val");
        v.set_xalign(0.0);
        v.set_wrap(true);
        v.set_hexpand(true);

        row.append(&k);
        row.append(&v);
        metrics_box.append(&row);
    }
    root.append(&metrics_box);

    if !sec.findings.is_empty() {
        let f_hdr = Label::new(Some(lang.pick("İlgili Bulgular", "Section Findings")));
        f_hdr.add_css_class("akrep-card-title");
        f_hdr.set_xalign(0.0);
        f_hdr.set_margin_top(6);
        root.append(&f_hdr);

        for f in &sec.findings {
            root.append(&build_finding_row(f, lang));
        }
    }

    root
}
