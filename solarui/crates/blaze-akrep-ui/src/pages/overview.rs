use crate::components::finding_row::build_finding_row;
use crate::components::nav_rail::map_card_id_to_tab;
use crate::components::severity_css_class;
use crate::components::status_card::build_status_card;
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_sentinel_core::AkrepSeverity;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, FlowBox, Label, Orientation, SelectionMode};
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_overview_page(
    state: &Rc<RefCell<AppUiState>>,
    on_select_tab: Rc<dyn Fn(&'static str)>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let legend_row = GtkBox::new(Orientation::Horizontal, 8);
    legend_row.add_css_class("akrep-detail-box");
    let legend_lbl = Label::new(Some(lang.pick(
        "Durum Modeli (5 Durum — Kullanılamıyor/Bilinmiyor asla Korunuyor sayılmaz):",
        "Posture Model (5 States — Unavailable/Unknown are never conflated with Protected):",
    )));
    legend_lbl.add_css_class("akrep-subtitle");
    legend_lbl.set_xalign(0.0);
    legend_lbl.set_hexpand(true);
    legend_row.append(&legend_lbl);

    for sev in [
        AkrepSeverity::Protected,
        AkrepSeverity::Attention,
        AkrepSeverity::Critical,
        AkrepSeverity::Unavailable,
        AkrepSeverity::Unknown,
    ] {
        let b = Label::new(Some(match lang {
            Lang::Tr => sev.label_tr(),
            Lang::En => sev.label_en(),
        }));
        b.add_css_class("akrep-badge");
        b.add_css_class(severity_css_class(sev));
        legend_row.append(&b);
    }
    root.append(&legend_row);

    let Some(s) = snap else {
        let empty = GtkBox::new(Orientation::Vertical, 8);
        empty.add_css_class("akrep-detail-box");
        let lbl = Label::new(Some(lang.pick(
            "akrepd telemetri servisine henüz ulaşılamadı. Üstteki 'Yenile' düğmesini kullanabilir veya sol menüden Uygulama Güvenliği, Gizlilik ya da Polkit tabanlı yönetim sayfalarını doğrudan açabilirsiniz.",
            "akrepd telemetry service is currently unreachable. Click 'Refresh' above, or navigate directly to Application Security, Privacy, or Polkit-managed pages from the left rail.",
        )));
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        empty.append(&lbl);
        root.append(&empty);
        return root;
    };

    let prof_bar = GtkBox::new(Orientation::Horizontal, 12);
    prof_bar.add_css_class("akrep-detail-box");
    let prof_txt = Label::new(Some(&format!(
        "{}: {} ({}) — {}",
        lang.pick("Etkin Güvenlik Profili", "Active Security Profile"),
        lang.pick(&s.active_profile_tr, &s.active_profile_en),
        s.active_profile_id,
        lang.pick(
            "SELinux Enforcing ve Firewalld tüm profillerde zorunlu tutulur.",
            "SELinux Enforcing and Firewalld remain mandatory across all profiles.",
        )
    )));
    prof_txt.set_xalign(0.0);
    prof_txt.set_wrap(true);
    prof_txt.set_hexpand(true);
    prof_bar.append(&prof_txt);

    let prof_btn = Button::with_label(lang.pick("Profil Değiştir", "Switch Profile"));
    prof_btn.add_css_class("akrep-ghost-btn");
    prof_btn.set_valign(Align::Center);
    let cb_prof = Rc::clone(&on_select_tab);
    prof_btn.connect_clicked(move |_| cb_prof("profiles"));
    prof_bar.append(&prof_btn);
    root.append(&prof_bar);

    let grid_hdr = Label::new(Some(lang.pick("Güvenlik Alanları", "Security Domains")));
    grid_hdr.add_css_class("akrep-section-hdr");
    grid_hdr.set_xalign(0.0);
    root.append(&grid_hdr);

    let flow = FlowBox::new();
    flow.set_valign(Align::Start);
    flow.set_halign(Align::Fill);
    flow.set_max_children_per_line(4);
    flow.set_min_children_per_line(2);
    flow.set_column_spacing(12);
    flow.set_row_spacing(12);
    flow.set_selection_mode(SelectionMode::None);
    flow.set_homogeneous(true);

    for card in &s.cards {
        let cb = Rc::clone(&on_select_tab);
        let btn = build_status_card(card, lang, move |section_id| {
            cb(map_card_id_to_tab(&section_id));
        });
        flow.insert(&btn, -1);
    }
    root.append(&flow);

    let f_hdr = Label::new(Some(lang.pick(
        "Öncelikli Bulgular ve Öneriler",
        "Actionable Findings & Advisories",
    )));
    f_hdr.add_css_class("akrep-section-hdr");
    f_hdr.set_xalign(0.0);
    f_hdr.set_margin_top(6);
    root.append(&f_hdr);

    if s.findings.is_empty() {
        let clean = GtkBox::new(Orientation::Vertical, 4);
        clean.add_css_class("akrep-detail-box");
        let lbl = Label::new(Some(lang.pick(
            "Herhangi bir kritik veya dikkat gerektiren güvenlik bulgusu tespit edilmedi.",
            "No critical or attention-level security findings detected.",
        )));
        lbl.set_xalign(0.0);
        clean.append(&lbl);
        root.append(&clean);
    } else {
        for f in s.findings.iter().take(12) {
            root.append(&build_finding_row(f, lang, Some(Rc::clone(&on_select_tab))));
        }
    }

    root
}
