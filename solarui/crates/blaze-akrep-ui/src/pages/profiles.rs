use crate::components::plan_dialog::present_plan_preview_dialog;
use crate::components::{build_admin_note, build_risk_indicator, build_status_indicator};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::AppUiState;
use blaze_akrep_common::{ActionRequest, TransactionState};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, ApplicationWindow, Box as GtkBox, Button, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

struct StaticProfileMeta {
    id: &'static str,
    name_tr: &'static str,
    name_en: &'static str,
    zone: &'static str,
    summary_tr: &'static str,
    summary_en: &'static str,
    policy_tr: &'static str,
    policy_en: &'static str,
}

const BUILTIN_PROFILES: &[StaticProfileMeta] = &[
    StaticProfileMeta {
        id: "balanced",
        name_tr: "Dengeli",
        name_en: "Balanced",
        zone: "FedoraWorkstation / public",
        summary_tr: "Günlük masaüstü kullanımı için SELinux Enforcing ve standart güvenlik duvarı kuralları",
        summary_en: "Default BlazeOS desktop baseline with SELinux Enforcing and clean firewall rules",
        policy_tr: "SELinux Enforcing · Özel yerel ağ kuralı yok",
        policy_en: "SELinux Enforcing · No custom LAN rules",
    },
    StaticProfileMeta {
        id: "developer",
        name_tr: "Geliştirici",
        name_en: "Developer",
        zone: "FedoraWorkstation / public",
        summary_tr: "Konteyner ve sanallaştırma araçları için gereken SELinux izinlerini açar; güvenlik duvarı etkin kalır",
        summary_en: "Enables container and virtualization SELinux booleans while keeping the firewall active",
        policy_tr: "virt_use_kvm=on, container_manage_cgroup=on · Bellek koruması korunur",
        policy_en: "virt_use_kvm=on, container_manage_cgroup=on · Memory protections intact",
    },
    StaticProfileMeta {
        id: "gaming",
        name_tr: "Oyun",
        name_en: "Gaming",
        zone: "FedoraWorkstation / home",
        summary_tr: "Yalnızca yerel ağda Steam Remote Play ve LAN keşif portlarına izin verir; dış ağa port açmaz",
        summary_en: "Allows local network discovery and Steam Remote Play on private LAN only; no public ports",
        policy_tr: "Yalnızca özel yerel ağ (10/8, 172.16/12, 192.168/16) · Geçici",
        policy_en: "Private LAN subnets only (10/8, 172.16/12, 192.168/16) · Temporary",
    },
    StaticProfileMeta {
        id: "public-network",
        name_tr: "Açık ağ",
        name_en: "Public network",
        zone: "drop / block",
        summary_tr: "Kafe veya havalimanı gibi ortak Wi-Fi ağlarında tüm gelen bağlantıları ve paylaşım servislerini engeller",
        summary_en: "Blocks incoming connections and local discovery services on untrusted public Wi-Fi",
        policy_tr: "Gelen ssh, mdns, samba ve KDE Connect erişimi kapalı",
        policy_en: "Incoming ssh, mdns, samba and KDE Connect stripped",
    },
];

pub fn build_profiles_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    window: &ApplicationWindow,
    on_rerender: Rc<dyn Fn()>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let (active_id, _, _) = st.effective_active_profile();
    let preview = st.last_profile_preview.clone();
    let history = st.history_records.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 20);

    let intro_col = GtkBox::new(Orientation::Vertical, 4);
    let hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    hdr_row.set_valign(Align::Center);

    let page_desc = Label::new(Some(lang.pick(
        "Tüm profillerde SELinux Enforcing ve güvenlik duvarı etkin kalır",
        "SELinux Enforcing and firewall remain active across all profiles",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    page_desc.set_hexpand(true);
    hdr_row.append(&page_desc);

    if let Some(last_prof_tx) = history.iter().find(|r| {
        r.state == TransactionState::Committed
            && matches!(r.request, ActionRequest::ApplyProfile { .. })
    }) {
        let rb_btn = Button::with_label(lang.pick("Son geçişi geri al", "Rollback last switch"));
        rb_btn.add_css_class("akrep-danger-btn");
        rb_btn.set_valign(Align::Center);
        let cmd_rb = cmd_tx.clone();
        let tid = last_prof_tx.tx_id.clone();
        rb_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_rb,
                UiCommand::RollbackTransaction { tx_id: tid.clone() },
            );
        });
        hdr_row.append(&rb_btn);
    }

    let load_btn = Button::with_label(lang.pick("Durumu yenile", "Sync state"));
    load_btn.add_css_class("akrep-ghost-btn");
    load_btn.set_valign(Align::Center);
    let cmd_l = cmd_tx.clone();
    load_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_l, UiCommand::ListProfiles);
    });
    hdr_row.append(&load_btn);
    intro_col.append(&hdr_row);
    intro_col.append(&build_admin_note(lang));
    root.append(&intro_col);

    for meta in BUILTIN_PROFILES {
        let is_active = active_id == meta.id;
        let pcard = GtkBox::new(Orientation::Vertical, 8);
        pcard.add_css_class("akrep-detail-box");

        let top = GtkBox::new(Orientation::Horizontal, 12);
        top.set_valign(Align::Center);

        let name_box = GtkBox::new(Orientation::Horizontal, 8);
        name_box.set_valign(Align::Center);
        name_box.set_hexpand(true);

        let pname = Label::new(Some(lang.pick(meta.name_tr, meta.name_en)));
        pname.add_css_class("akrep-section-hdr");
        pname.set_xalign(0.0);
        name_box.append(&pname);

        if meta.id == "balanced" {
            let def_tag = Label::new(Some(lang.pick("Varsayılan", "Default")));
            def_tag.add_css_class("akrep-tag");
            name_box.append(&def_tag);
        }

        if is_active {
            name_box.append(&build_status_indicator(
                "akrep-sev-protected",
                lang.pick("Etkin", "Active"),
                false,
            ));
        }
        top.append(&name_box);

        let prev_btn = Button::with_label(lang.pick("Önizle", "Preview"));
        prev_btn.add_css_class("akrep-ghost-btn");
        prev_btn.set_valign(Align::Center);
        let cmd_pv = cmd_tx.clone();
        let pid = meta.id.to_string();
        prev_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_pv,
                UiCommand::PreviewProfile {
                    profile_id: pid.clone(),
                },
            );
        });
        top.append(&prev_btn);
        pcard.append(&top);

        let sum = Label::new(Some(lang.pick(meta.summary_tr, meta.summary_en)));
        sum.add_css_class("akrep-subtitle");
        sum.set_xalign(0.0);
        sum.set_wrap(true);
        sum.set_wrap_mode(WrapMode::WordChar);
        pcard.append(&sum);

        let pol = Label::new(Some(&format!(
            "{}: {} · {}",
            lang.pick("Bölge", "Zone"),
            meta.zone,
            lang.pick(meta.policy_tr, meta.policy_en)
        )));
        pol.add_css_class("akrep-mono");
        pol.set_xalign(0.0);
        pol.set_wrap(true);
        pol.set_wrap_mode(WrapMode::WordChar);
        pcard.append(&pol);

        root.append(&pcard);
    }

    // Inline Profile Diff Card — re-opens PlanPreviewDialog so user always reviews SHA-256 & warnings
    if let Some(plan) = &preview {
        let diff_card = GtkBox::new(Orientation::Vertical, 12);
        diff_card.add_css_class("akrep-detail-box");

        let dhdr = GtkBox::new(Orientation::Horizontal, 12);
        dhdr.set_valign(Align::Center);
        let dtitle = Label::new(Some(lang.pick(
            "Hazırlanan profil geçişi",
            "Staged profile switch",
        )));
        dtitle.add_css_class("akrep-section-hdr");
        dtitle.set_xalign(0.0);
        dtitle.set_hexpand(true);
        dhdr.append(&dtitle);

        dhdr.append(&build_risk_indicator(plan.risk, lang));

        let clear_btn = Button::with_label(lang.pick("Temizle", "Dismiss"));
        clear_btn.add_css_class("akrep-ghost-btn");
        let st_clr = Rc::clone(state);
        let rr_clr = Rc::clone(&on_rerender);
        clear_btn.connect_clicked(move |_| {
            st_clr.borrow_mut().last_profile_preview = None;
            rr_clr();
        });
        dhdr.append(&clear_btn);

        let review_btn = Button::with_label(lang.pick("Onayla", "Approve"));
        review_btn.add_css_class("akrep-primary-btn");
        let win_rev = window.clone();
        let st_rev = Rc::clone(state);
        let cmd_rev = cmd_tx.clone();
        let plan_clone = plan.clone();
        let rr_rev = Rc::clone(&on_rerender);
        review_btn.connect_clicked(move |_| {
            present_plan_preview_dialog(
                &win_rev,
                &st_rev,
                &plan_clone,
                lang,
                cmd_rev.clone(),
                Some(Rc::clone(&rr_rev)),
            );
        });
        dhdr.append(&review_btn);
        diff_card.append(&dhdr);

        for d in &plan.diffs {
            let drow = Label::new(Some(&format!(
                "{} · {} → {} ({})",
                d.target,
                d.current_value,
                d.desired_value,
                lang.pick(&d.description_tr, &d.description_en)
            )));
            drow.add_css_class("akrep-mono");
            drow.set_xalign(0.0);
            drow.set_wrap(true);
            drow.set_wrap_mode(WrapMode::WordChar);
            diff_card.append(&drow);
        }
        root.append(&diff_card);
    }

    root
}
