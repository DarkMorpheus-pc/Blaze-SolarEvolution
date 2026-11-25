use crate::components::{build_kv_row, risk_css_class, risk_label};
use crate::dbus_client::UiCommand;
use crate::i18n::Lang;
use blaze_akrep_common::ActionPlan;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, HeaderBar, Image, Label, Orientation,
    ScrolledWindow, Window,
};

pub fn present_plan_preview_dialog(
    parent: &ApplicationWindow,
    plan: &ActionPlan,
    lang: Lang,
    cmd_tx: async_channel::Sender<UiCommand>,
) {
    let dialog = Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(680)
        .default_height(520)
        .title(lang.pick(
            "İşlem Planı Önizlemesi (Polkit Onayı)",
            "Action Plan Preview (Polkit Authorization)",
        ))
        .build();
    dialog.add_css_class("akrep-modal");

    let hb = HeaderBar::new();
    hb.add_css_class("akrep-headerbar");
    dialog.set_titlebar(Some(&hb));

    let root = GtkBox::new(Orientation::Vertical, 14);
    root.set_margin_top(16);
    root.set_margin_bottom(16);
    root.set_margin_start(18);
    root.set_margin_end(18);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let icon = Image::from_icon_name("security-high-symbolic");
    icon.set_pixel_size(28);
    hdr_row.append(&icon);

    let title_lbl = Label::new(Some(lang.pick(
        "Uygulanacak Değişiklikleri Doğrulayın",
        "Review Staged Security Changes Before Commit",
    )));
    title_lbl.add_css_class("akrep-section-hdr");
    title_lbl.set_xalign(0.0);
    title_lbl.set_hexpand(true);
    hdr_row.append(&title_lbl);

    let risk_badge = Label::new(Some(risk_label(plan.risk, lang)));
    risk_badge.add_css_class("akrep-badge");
    risk_badge.add_css_class(risk_css_class(plan.risk));
    hdr_row.append(&risk_badge);
    root.append(&hdr_row);

    let meta_box = GtkBox::new(Orientation::Vertical, 6);
    meta_box.add_css_class("akrep-detail-box");
    meta_box.append(&build_kv_row(
        lang.pick("Plan Kimliği (UUID)", "Plan ID (UUID)"),
        &plan.plan_id,
    ));
    meta_box.append(&build_kv_row(
        lang.pick("Polkit Eylemi", "Required Polkit Action"),
        &plan.polkit_action,
    ));
    let digest_short = plan.plan_sha256.chars().take(24).collect::<String>();
    meta_box.append(&build_kv_row(
        lang.pick("Kanonik SHA-256 Özeti", "Canonical SHA-256 Digest"),
        &format!("{}...", digest_short),
    ));
    meta_box.append(&build_kv_row(
        lang.pick("Son Geçerlilik (TTL)", "Plan Expires At (TTL)"),
        &plan.expires_at,
    ));
    root.append(&meta_box);

    let scroller = ScrolledWindow::new();
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);

    let content = GtkBox::new(Orientation::Vertical, 10);

    let diff_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick("Önce -> Sonra Fark Özeti", "Before -> After Diff Items"),
        plan.diffs.len()
    )));
    diff_hdr.add_css_class("akrep-card-title");
    diff_hdr.set_xalign(0.0);
    content.append(&diff_hdr);

    if plan.diffs.is_empty() {
        let none_lbl = Label::new(Some(lang.pick(
            "Durum zaten hedef değerde (fark yok).",
            "Current state already matches desired target (no diff).",
        )));
        none_lbl.add_css_class("akrep-subtitle");
        none_lbl.set_xalign(0.0);
        content.append(&none_lbl);
    } else {
        for item in &plan.diffs {
            let d_card = GtkBox::new(Orientation::Vertical, 4);
            d_card.add_css_class("akrep-diff-row");

            let t_lbl = Label::new(Some(&item.target));
            t_lbl.add_css_class("akrep-mono");
            t_lbl.set_xalign(0.0);
            t_lbl.set_wrap(true);
            d_card.append(&t_lbl);

            let desc_lbl = Label::new(Some(lang.pick(&item.description_tr, &item.description_en)));
            desc_lbl.set_xalign(0.0);
            desc_lbl.set_wrap(true);
            d_card.append(&desc_lbl);

            let change_lbl = Label::new(Some(&format!(
                "{}: {}   ->   {}: {}",
                lang.pick("Mevcut", "Current"),
                item.current_value,
                lang.pick("Hedef", "Desired"),
                item.desired_value
            )));
            change_lbl.add_css_class("akrep-subtitle");
            change_lbl.set_xalign(0.0);
            change_lbl.set_wrap(true);
            d_card.append(&change_lbl);

            content.append(&d_card);
        }
    }

    let warnings = match lang {
        Lang::Tr => &plan.warnings_tr,
        Lang::En => &plan.warnings_en,
    };
    if !warnings.is_empty() {
        let warn_box = GtkBox::new(Orientation::Vertical, 4);
        warn_box.add_css_class("akrep-offline-banner");
        let w_hdr = Label::new(Some(lang.pick("Uyarılar", "Warnings")));
        w_hdr.add_css_class("akrep-card-title");
        w_hdr.set_xalign(0.0);
        warn_box.append(&w_hdr);
        for w in warnings {
            let wl = Label::new(Some(&format!("- {}", w)));
            wl.set_xalign(0.0);
            wl.set_wrap(true);
            warn_box.append(&wl);
        }
        content.append(&warn_box);
    }

    scroller.set_child(Some(&content));
    root.append(&scroller);

    let footer = GtkBox::new(Orientation::Horizontal, 10);
    footer.set_halign(Align::End);

    let cancel_btn = Button::with_label(lang.pick("İptal", "Cancel"));
    cancel_btn.add_css_class("akrep-ghost-btn");
    let dlg_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dlg_cancel.close();
    });
    footer.append(&cancel_btn);

    let approve_btn = Button::with_label(lang.pick(
        "Onayla ve Doğrula (Polkit)",
        "Approve & Authenticate (Polkit)",
    ));
    approve_btn.add_css_class("akrep-primary-btn");
    let plan_id = plan.plan_id.clone();
    let plan_sha256 = plan.plan_sha256.clone();
    let dlg_ok = dialog.clone();
    approve_btn.connect_clicked(move |_| {
        let _ = cmd_tx.send_blocking(UiCommand::CommitPlan {
            plan_id: plan_id.clone(),
            plan_sha256: plan_sha256.clone(),
        });
        dlg_ok.close();
    });
    footer.append(&approve_btn);

    root.append(&footer);
    dialog.set_child(Some(&root));
    dialog.present();
}
