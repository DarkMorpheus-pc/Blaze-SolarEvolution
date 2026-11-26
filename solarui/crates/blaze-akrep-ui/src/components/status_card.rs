use crate::components::{build_severity_indicator, domain_icon_name, domain_title};
use crate::i18n::Lang;
use blaze_sentinel_core::AkrepCardSummary;
use gtk4::pango::{EllipsizeMode, WrapMode};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};

pub fn build_status_card(
    card: &AkrepCardSummary,
    lang: Lang,
    on_open: impl Fn(String) + 'static,
) -> Button {
    let btn = Button::new();
    btn.add_css_class("akrep-card");
    btn.set_hexpand(true);
    btn.set_halign(Align::Fill);

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_hexpand(true);
    root.set_halign(Align::Fill);
    root.set_valign(Align::Fill);

    let top_row = GtkBox::new(Orientation::Horizontal, 8);
    top_row.set_valign(Align::Center);

    let icon = Image::from_icon_name(domain_icon_name(&card.id));
    icon.set_pixel_size(16);
    icon.add_css_class("akrep-icon-secondary");
    icon.set_valign(Align::Center);
    top_row.append(&icon);

    let title_str =
        domain_title(&card.id, lang).unwrap_or_else(|| lang.pick(&card.title_tr, &card.title_en));
    let title = Label::new(Some(title_str));
    title.add_css_class("akrep-card-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    title.set_ellipsize(EllipsizeMode::End);
    top_row.append(&title);

    top_row.append(&build_severity_indicator(card.status, lang));
    root.append(&top_row);

    let summary = Label::new(Some(lang.pick(&card.summary_tr, &card.summary_en)));
    summary.add_css_class("akrep-subtitle");
    summary.set_xalign(0.0);
    summary.set_yalign(0.0);
    summary.set_wrap(true);
    summary.set_wrap_mode(WrapMode::WordChar);
    summary.set_lines(2);
    summary.set_ellipsize(EllipsizeMode::End);
    summary.set_vexpand(true);
    root.append(&summary);

    btn.set_child(Some(&root));
    let section_id = card.id.clone();
    btn.connect_clicked(move |_| on_open(section_id.clone()));
    btn
}
