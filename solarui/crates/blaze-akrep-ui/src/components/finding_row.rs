use crate::components::severity_css_class;
use crate::i18n::Lang;
use blaze_sentinel_core::AkrepFinding;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Label, Orientation};

pub fn build_finding_row(finding: &AkrepFinding, lang: Lang) -> GtkBox {
    let card = GtkBox::new(Orientation::Vertical, 6);
    card.add_css_class("akrep-detail-box");

    let header = GtkBox::new(Orientation::Horizontal, 8);
    let sev_lbl = Label::new(Some(match lang {
        Lang::Tr => finding.severity.label_tr(),
        Lang::En => finding.severity.label_en(),
    }));
    sev_lbl.add_css_class("akrep-badge");
    sev_lbl.add_css_class(severity_css_class(finding.severity));
    header.append(&sev_lbl);

    let title = Label::new(Some(lang.pick(&finding.title_tr, &finding.title_en)));
    title.add_css_class("akrep-card-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    title.set_hexpand(true);
    header.append(&title);

    let src = Label::new(Some(&finding.source));
    src.add_css_class("akrep-subtitle");
    header.append(&src);

    card.append(&header);

    let desc = Label::new(Some(lang.pick(&finding.description_tr, &finding.description_en)));
    desc.set_xalign(0.0);
    desc.set_wrap(true);
    card.append(&desc);

    let hint = Label::new(Some(&format!(
        "{}: {}",
        lang.pick("Salt Okunur Öneri", "Read-Only Hint"),
        lang.pick(&finding.remediation_hint_tr, &finding.remediation_hint_en)
    )));
    hint.add_css_class("akrep-subtitle");
    hint.set_xalign(0.0);
    hint.set_wrap(true);
    card.append(&hint);

    card
}
