use crate::components::{severity_css_class, severity_icon_name};
use crate::i18n::Lang;
use blaze_sentinel_core::AkrepCardSummary;
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Image, Label, Orientation};

pub fn build_status_card(
    card: &AkrepCardSummary,
    lang: Lang,
    on_open: impl Fn(String) + 'static,
) -> Button {
    let btn = Button::new();
    btn.add_css_class("akrep-card");
    btn.set_size_request(220, 140);
    btn.set_hexpand(false);

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_hexpand(false);

    let top_row = GtkBox::new(Orientation::Horizontal, 8);
    let icon = Image::from_icon_name(severity_icon_name(card.status, card.availability));
    icon.set_pixel_size(28);
    top_row.append(&icon);

    let spacer = GtkBox::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    top_row.append(&spacer);

    let pill = Label::new(Some(match lang {
        Lang::Tr => card.status.label_tr(),
        Lang::En => card.status.label_en(),
    }));
    pill.add_css_class("akrep-badge");
    pill.add_css_class(severity_css_class(card.status));
    top_row.append(&pill);
    root.append(&top_row);

    let title = Label::new(Some(lang.pick(&card.title_tr, &card.title_en)));
    title.add_css_class("akrep-card-title");
    title.set_xalign(0.0);
    title.set_wrap(true);
    root.append(&title);

    let summary = Label::new(Some(lang.pick(&card.summary_tr, &card.summary_en)));
    summary.add_css_class("akrep-subtitle");
    summary.set_xalign(0.0);
    summary.set_wrap(true);
    summary.set_vexpand(true);
    root.append(&summary);

    let footer = Label::new(Some(lang.pick("İncele →", "Inspect →")));
    footer.add_css_class("akrep-metric-key");
    footer.set_xalign(0.0);
    root.append(&footer);

    btn.set_child(Some(&root));
    let section_id = card.id.clone();
    btn.connect_clicked(move |_| on_open(section_id.clone()));
    btn
}
