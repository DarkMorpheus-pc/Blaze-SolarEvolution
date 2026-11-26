use crate::components::{build_admin_note, build_kv_row, build_risk_indicator};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::pages::history::summarize_action_request;
use crate::state::AppUiState;
use blaze_akrep_common::{ActionPlan, ActionRequest, OverrideDuration, SelinuxDesiredMode};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{
    Align, ApplicationWindow, Box as GtkBox, Button, Expander, HeaderBar, Image, Label,
    Orientation, PolicyType, ScrolledWindow, Window,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// Returns true when a previewed plan relaxes protection persistently until manually re-enabled.
pub fn is_until_changed_relaxation(req: &ActionRequest) -> bool {
    matches!(
        req,
        ActionRequest::SelinuxSetMode {
            mode: SelinuxDesiredMode::Permissive,
            duration: OverrideDuration::UntilChanged,
        } | ActionRequest::FirewallSetState {
            enabled: false,
            duration: OverrideDuration::UntilChanged,
        } | ActionRequest::OomdSetState {
            enabled: false,
            duration: OverrideDuration::UntilChanged,
        }
    )
}

fn format_diff_target_label(target: &str, lang: Lang) -> String {
    match target {
        "selinux.mode" => format!(
            "{} ({})",
            lang.pick("SELinux çalışma modu", "SELinux runtime mode"),
            target
        ),
        "firewall.service_state" => format!(
            "{} ({})",
            lang.pick("Güvenlik duvarı servisi", "Firewall service"),
            target
        ),
        "system.oomd_state" => format!(
            "{} ({})",
            lang.pick("Bellek yöneticisi (systemd-oomd)", "Memory manager (systemd-oomd)"),
            target
        ),
        "system.limits.nofile" => format!(
            "{} ({})",
            lang.pick("Açık dosya limiti", "Open file descriptor limit"),
            target
        ),
        t if t.starts_with("packages.compat.") => format!(
            "{}: {}",
            lang.pick("Uyumluluk paketi", "Compatibility package"),
            t.trim_start_matches("packages.compat.")
        ),
        other => other.to_string(),
    }
}

pub fn present_plan_preview_dialog(
    parent: &ApplicationWindow,
    state: &Rc<RefCell<AppUiState>>,
    plan: &ActionPlan,
    lang: Lang,
    cmd_tx: async_channel::Sender<UiCommand>,
    on_cancelled: Option<Rc<dyn Fn()>>,
) {
    if let Some(prev_dlg) = state.borrow_mut().active_plan_dialog.take() {
        prev_dlg.close();
    }

    let dialog = Window::builder()
        .transient_for(parent)
        .modal(true)
        .default_width(640)
        .default_height(500)
        .title(lang.pick("İşlem önizlemesi", "Action preview"))
        .build();
    dialog.add_css_class("akrep-modal");
    state.borrow_mut().active_plan_dialog = Some(dialog.clone());

    let committed = Rc::new(Cell::new(false));

    let hb = HeaderBar::new();
    hb.add_css_class("akrep-headerbar");
    dialog.set_titlebar(Some(&hb));

    let root = GtkBox::new(Orientation::Vertical, 16);
    root.set_margin_top(20);
    root.set_margin_bottom(20);
    root.set_margin_start(24);
    root.set_margin_end(24);

    let hdr_col = GtkBox::new(Orientation::Vertical, 4);
    let hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    hdr_row.set_valign(Align::Center);

    let icon = Image::from_icon_name("security-high-symbolic");
    icon.set_pixel_size(16);
    icon.add_css_class("akrep-icon-secondary");
    hdr_row.append(&icon);

    let title_lbl = Label::new(Some(lang.pick(
        "Değişiklikleri gözden geçir",
        "Review changes",
    )));
    title_lbl.add_css_class("akrep-section-hdr");
    title_lbl.set_xalign(0.0);
    title_lbl.set_hexpand(true);
    title_lbl.set_wrap(true);
    hdr_row.append(&title_lbl);

    hdr_row.append(&build_risk_indicator(plan.risk, lang));
    hdr_col.append(&hdr_row);

    let action_sum = Label::new(Some(&summarize_action_request(&plan.request, lang)));
    action_sum.add_css_class("akrep-subtitle");
    action_sum.set_xalign(0.0);
    action_sum.set_wrap(true);
    action_sum.set_wrap_mode(WrapMode::WordChar);
    hdr_col.append(&action_sum);

    hdr_col.append(&build_admin_note(lang));
    root.append(&hdr_col);

    let scroller = ScrolledWindow::new();
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);
    scroller.set_policy(PolicyType::Never, PolicyType::Automatic);

    let content = GtkBox::new(Orientation::Vertical, 12);

    if is_until_changed_relaxation(&plan.request) {
        let callout = GtkBox::new(Orientation::Vertical, 6);
        callout.add_css_class("akrep-high-risk-banner");

        let chdr = GtkBox::new(Orientation::Horizontal, 8);
        chdr.set_valign(Align::Center);
        let cicon = Image::from_icon_name("dialog-warning-symbolic");
        cicon.set_pixel_size(16);
        cicon.add_css_class("akrep-icon-secondary");
        chdr.append(&cicon);

        let ctitle = Label::new(Some(lang.pick(
            "Kalıcı gevşetme uyarısı",
            "Persistent relaxation notice",
        )));
        ctitle.add_css_class("akrep-card-title");
        ctitle.set_xalign(0.0);
        ctitle.set_wrap(true);
        ctitle.set_hexpand(true);
        chdr.append(&ctitle);
        callout.append(&chdr);

        let cdesc = Label::new(Some(match &plan.request {
            ActionRequest::OomdSetState { .. } => lang.pick(
                "systemd-oomd siz yeniden etkinleştirene kadar yeniden başlatmalar arasında da duraklatılmış kalacaktır",
                "systemd-oomd will remain paused across reboots until you re-enable it",
            ),
            _ => lang.pick(
                "Bu gevşetme siz yeniden etkinleştirene kadar yeniden başlatmalar arasında da etkin kalacaktır",
                "This override remains active across reboots until you re-enable protection",
            ),
        }));
        cdesc.add_css_class("akrep-subtitle");
        cdesc.set_xalign(0.0);
        cdesc.set_wrap(true);
        cdesc.set_wrap_mode(WrapMode::WordChar);
        callout.append(&cdesc);
        content.append(&callout);
    }

    let diff_hdr = Label::new(Some(&format!(
        "{} ({})",
        lang.pick("Değişiklikler", "Changes"),
        plan.diffs.len()
    )));
    diff_hdr.add_css_class("akrep-card-title");
    diff_hdr.set_xalign(0.0);
    content.append(&diff_hdr);

    if plan.diffs.is_empty() {
        let none_lbl = Label::new(Some(lang.pick(
            "Mevcut durum zaten hedef değerde",
            "Current state already matches the target",
        )));
        none_lbl.add_css_class("akrep-subtitle");
        none_lbl.set_xalign(0.0);
        none_lbl.set_wrap(true);
        content.append(&none_lbl);
    } else {
        for item in &plan.diffs {
            let d_card = GtkBox::new(Orientation::Vertical, 4);
            d_card.add_css_class("akrep-diff-row");

            let t_lbl = Label::new(Some(&format_diff_target_label(&item.target, lang)));
            t_lbl.add_css_class("akrep-mono");
            t_lbl.set_xalign(0.0);
            t_lbl.set_wrap(true);
            t_lbl.set_wrap_mode(WrapMode::WordChar);
            d_card.append(&t_lbl);

            let desc_lbl = Label::new(Some(lang.pick(&item.description_tr, &item.description_en)));
            desc_lbl.add_css_class("akrep-subtitle");
            desc_lbl.set_xalign(0.0);
            desc_lbl.set_wrap(true);
            desc_lbl.set_wrap_mode(WrapMode::WordChar);
            d_card.append(&desc_lbl);

            let change_lbl = Label::new(Some(&format!(
                "{}: {}  →  {}: {}",
                lang.pick("Mevcut", "Current"),
                item.current_value,
                lang.pick("Hedef", "Target"),
                item.desired_value
            )));
            change_lbl.add_css_class("akrep-meta");
            change_lbl.set_xalign(0.0);
            change_lbl.set_wrap(true);
            change_lbl.set_wrap_mode(WrapMode::WordChar);
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
            let wl = Label::new(Some(&format!("· {}", w)));
            wl.add_css_class("akrep-subtitle");
            wl.set_xalign(0.0);
            wl.set_wrap(true);
            wl.set_wrap_mode(WrapMode::WordChar);
            warn_box.append(&wl);
        }
        content.append(&warn_box);
    }

    let tech_exp = Expander::new(Some(lang.pick(
        "Teknik doğrulama bilgileri",
        "Technical verification details",
    )));
    let meta_box = GtkBox::new(Orientation::Vertical, 8);
    meta_box.add_css_class("akrep-detail-box");
    meta_box.set_margin_top(8);
    meta_box.append(&build_kv_row(
        lang.pick("Plan kimliği", "Plan ID"),
        &plan.plan_id,
    ));
    meta_box.append(&build_kv_row(
        lang.pick("Yetki eylemi", "Auth action"),
        &plan.polkit_action,
    ));
    let digest_short = plan.plan_sha256.chars().take(24).collect::<String>();
    meta_box.append(&build_kv_row("SHA-256", &format!("{}...", digest_short)));
    meta_box.append(&build_kv_row(
        lang.pick("Geçerlilik sonu", "Expires at"),
        &plan.expires_at,
    ));
    tech_exp.set_child(Some(&meta_box));
    content.append(&tech_exp);

    scroller.set_child(Some(&content));
    root.append(&scroller);

    let footer = GtkBox::new(Orientation::Horizontal, 8);
    footer.set_halign(Align::End);

    let cancel_btn = Button::with_label(lang.pick("İptal", "Cancel"));
    cancel_btn.add_css_class("akrep-ghost-btn");
    let dlg_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dlg_cancel.close();
    });
    footer.append(&cancel_btn);

    let approve_btn = Button::with_label(lang.pick("Onayla ve uygula", "Approve and apply"));
    approve_btn.add_css_class("akrep-primary-btn");
    let plan_id = plan.plan_id.clone();
    let plan_sha256 = plan.plan_sha256.clone();
    let dlg_ok = dialog.clone();
    let committed_ok = Rc::clone(&committed);
    approve_btn.connect_clicked(move |_| {
        committed_ok.set(true);
        dispatch_cmd(
            &cmd_tx,
            UiCommand::CommitPlan {
                plan_id: plan_id.clone(),
                plan_sha256: plan_sha256.clone(),
            },
        );
        dlg_ok.close();
    });
    footer.append(&approve_btn);

    let st_close = Rc::clone(state);
    let committed_close = Rc::clone(&committed);
    let plan_id_close = plan.plan_id.clone();
    dialog.connect_close_request(move |win| {
        let should_notify = {
            let mut s = st_close.borrow_mut();
            if s.active_plan_dialog.as_ref() == Some(win) {
                s.active_plan_dialog = None;
            }
            if !committed_close.get()
                && s.last_profile_preview.as_ref().map(|p| p.plan_id.as_str())
                    == Some(plan_id_close.as_str())
            {
                s.last_profile_preview = None;
                true
            } else {
                false
            }
        };
        if should_notify {
            if let Some(cb) = &on_cancelled {
                cb();
            }
        }
        gtk4::glib::Propagation::Proceed
    });

    root.append(&footer);
    dialog.set_child(Some(&root));
    dialog.present();
}
