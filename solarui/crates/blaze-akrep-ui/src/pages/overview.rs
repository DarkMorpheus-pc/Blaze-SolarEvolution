use crate::components::finding_row::build_finding_row;
use crate::components::nav_rail::map_card_id_to_tab;
use crate::components::status_card::build_status_card;
use crate::components::{build_severity_indicator, build_status_dot};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_sentinel_core::AkrepSeverity;
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Grid, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

fn format_timestamp_short(iso: &str) -> String {
    iso.chars().take(16).collect::<String>().replace('T', " ")
}

fn overview_headline(sev: AkrepSeverity, lang: Lang) -> &'static str {
    match sev {
        AkrepSeverity::Protected => lang.pick("Sistem korunuyor", "System protected"),
        AkrepSeverity::Attention => lang.pick(
            "Bazı kontroller inceleme bekliyor",
            "Some controls need review",
        ),
        AkrepSeverity::Critical => lang.pick("Kritik güvenlik uyarısı", "Critical security alert"),
        AkrepSeverity::Unavailable => {
            lang.pick("Temel korumalar etkin", "Core protections active")
        }
        AkrepSeverity::Unknown => lang.pick(
            "Bazı güvenlik alanları doğrulanamadı",
            "Some security areas unverified",
        ),
    }
}

pub fn build_overview_page(
    state: &Rc<RefCell<AppUiState>>,
    on_select_tab: Rc<dyn Fn(&'static str)>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let (_, active_prof_tr, active_prof_en) = st.effective_active_profile();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 24);

    let Some(s) = snap else {
        let hero = GtkBox::new(Orientation::Vertical, 8);
        hero.add_css_class("akrep-hero-card");
        hero.append(&build_severity_indicator(AkrepSeverity::Unavailable, lang));

        let headline = Label::new(Some(lang.pick(
            "Güvenlik servisi yanıt vermiyor",
            "Security service unreachable",
        )));
        headline.add_css_class("akrep-hero-title");
        headline.set_xalign(0.0);
        headline.set_wrap(true);
        headline.set_wrap_mode(WrapMode::WordChar);
        hero.append(&headline);

        let desc = Label::new(Some(lang.pick(
            "Üstteki yenile düğmesiyle tekrar deneyin veya sol menüden bölümleri açın",
            "Try refreshing above or open a section from the sidebar",
        )));
        desc.add_css_class("akrep-subtitle");
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(WrapMode::WordChar);
        hero.append(&desc);

        root.append(&hero);
        return root;
    };

    // Overall security status block (single visual moment)
    let hero = GtkBox::new(Orientation::Horizontal, 24);
    hero.add_css_class("akrep-hero-card");
    hero.set_valign(Align::Start);

    let hero_left = GtkBox::new(Orientation::Vertical, 8);
    hero_left.set_hexpand(true);

    hero_left.append(&build_severity_indicator(s.overall_status, lang));

    let headline = Label::new(Some(overview_headline(s.overall_status, lang)));
    headline.add_css_class("akrep-hero-title");
    headline.set_xalign(0.0);
    headline.set_wrap(true);
    headline.set_wrap_mode(WrapMode::WordChar);
    hero_left.append(&headline);

    let sub_text = if s.overall_status == AkrepSeverity::Unavailable {
        lang.pick(
            "İsteğe bağlı bazı bileşenler kurulu değil; temel sistem korumaları çalışıyor",
            "Some optional components are not installed; core protections are active",
        )
        .to_string()
    } else {
        format!(
            "{}: {} · {}",
            lang.pick("Etkin profil", "Active profile"),
            lang.pick(&active_prof_tr, &active_prof_en),
            lang.pick(
                "SELinux Enforcing ve güvenlik duvarı etkin",
                "SELinux Enforcing and firewall active",
            )
        )
    };
    let sub_lbl = Label::new(Some(&sub_text));
    sub_lbl.add_css_class("akrep-subtitle");
    sub_lbl.set_xalign(0.0);
    sub_lbl.set_wrap(true);
    sub_lbl.set_wrap_mode(WrapMode::WordChar);
    hero_left.append(&sub_lbl);

    let meta_lbl = Label::new(Some(&format!(
        "{} {}/{} · Sentinel v{} · {} {}",
        lang.pick("Çekirdek kontrol", "Core controls"),
        s.posture.passed,
        s.posture.total,
        s.sentinel_version,
        lang.pick("Son denetim", "Last check"),
        format_timestamp_short(&s.generated_at),
    )));
    meta_lbl.add_css_class("akrep-meta");
    meta_lbl.set_xalign(0.0);
    meta_lbl.set_wrap(true);
    meta_lbl.set_wrap_mode(WrapMode::WordChar);
    hero_left.append(&meta_lbl);

    hero.append(&hero_left);

    let prof_btn = Button::with_label(lang.pick("Profil değiştir", "Switch profile"));
    prof_btn.add_css_class("akrep-ghost-btn");
    prof_btn.set_valign(Align::Center);
    let cb_prof = Rc::clone(&on_select_tab);
    prof_btn.connect_clicked(move |_| cb_prof("profiles"));
    hero.append(&prof_btn);

    root.append(&hero);

    // Security areas 2-column grid
    let areas_sec = GtkBox::new(Orientation::Vertical, 12);
    let grid_hdr = Label::new(Some(lang.pick("Güvenlik alanları", "Security areas")));
    grid_hdr.add_css_class("akrep-section-hdr");
    grid_hdr.set_xalign(0.0);
    areas_sec.append(&grid_hdr);

    let grid = Grid::new();
    grid.set_column_homogeneous(true);
    grid.set_column_spacing(12);
    grid.set_row_spacing(12);
    grid.set_hexpand(true);

    for (idx, card) in s.cards.iter().enumerate() {
        let mut adjusted_card = card.clone();
        if adjusted_card.id == "profiles" {
            adjusted_card.summary_tr = format!(
                "Etkin profil: {} · SELinux Enforcing ve güvenlik duvarı etkin",
                active_prof_tr
            );
            adjusted_card.summary_en = format!(
                "Active profile: {} · SELinux Enforcing and firewall active",
                active_prof_en
            );
            adjusted_card.metric = active_prof_en.to_ascii_uppercase();
        }
        let cb = Rc::clone(&on_select_tab);
        let btn = build_status_card(&adjusted_card, lang, move |section_id| {
            cb(map_card_id_to_tab(&section_id));
        });
        let col = (idx % 2) as i32;
        let row = (idx / 2) as i32;
        grid.attach(&btn, col, row, 1, 1);
    }
    areas_sec.append(&grid);
    root.append(&areas_sec);

    // Actionable findings
    let findings_sec = GtkBox::new(Orientation::Vertical, 12);
    let f_hdr = Label::new(Some(lang.pick(
        "Bulgular ve öneriler",
        "Findings and advisories",
    )));
    f_hdr.add_css_class("akrep-section-hdr");
    f_hdr.set_xalign(0.0);
    findings_sec.append(&f_hdr);

    if s.findings.is_empty() {
        let clean = GtkBox::new(Orientation::Horizontal, 8);
        clean.add_css_class("akrep-detail-box");
        clean.set_valign(Align::Center);
        clean.append(&build_status_dot("akrep-sev-protected"));
        let lbl = Label::new(Some(lang.pick(
            "Dikkat gerektiren güvenlik bulgusu yok",
            "No security findings require attention",
        )));
        lbl.add_css_class("akrep-subtitle");
        lbl.set_xalign(0.0);
        lbl.set_wrap(true);
        clean.append(&lbl);
        findings_sec.append(&clean);
    } else {
        for f in s.findings.iter().take(12) {
            findings_sec.append(&build_finding_row(f, lang, Some(Rc::clone(&on_select_tab))));
        }
    }
    root.append(&findings_sec);

    root
}
