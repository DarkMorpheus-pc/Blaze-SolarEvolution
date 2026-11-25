use crate::components::plan_dialog::present_plan_preview_dialog;
use crate::components::{risk_css_class, risk_label};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::AppUiState;
use blaze_akrep_common::{ActionRequest, TransactionState};
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
        name_tr: "Dengeli (Varsayılan)",
        name_en: "Balanced (Default)",
        zone: "FedoraWorkstation / public",
        summary_tr: "Günlük masaüstü kullanımı için sıkı SELinux Enforcing ve temiz güvenlik duvarı temel çizgisi.",
        summary_en: "Default BlazeOS desktop baseline with SELinux Enforcing and clean firewall posture.",
        policy_tr: "SELinux: Enforcing • Firewalld: Aktif • Özel LAN rich-rule yok • Flatpak & Portal onayı korunur",
        policy_en: "SELinux: Enforcing • Firewalld: Active • Zero custom LAN rich-rules • Flatpak & Portal consent preserved",
    },
    StaticProfileMeta {
        id: "developer",
        name_tr: "Geliştirici",
        name_en: "Developer",
        zone: "FedoraWorkstation / public",
        summary_tr: "Konteyner ve sanallaştırma iş akışları için denetimli boolean ayarları; SELinux ve güvenlik duvarı açık kalır.",
        summary_en: "Curated virtualization/container booleans while keeping SELinux Enforcing and Firewalld active.",
        policy_tr: "SELinux: Enforcing (virt_use_kvm=on, container_manage_cgroup=on) • Bellek zayıflatan execheap/execstack yasaklı",
        policy_en: "SELinux: Enforcing (virt_use_kvm=on, container_manage_cgroup=on) • Memory-weakening booleans forbidden",
    },
    StaticProfileMeta {
        id: "gaming",
        name_tr: "Oyun",
        name_en: "Gaming",
        zone: "FedoraWorkstation / home",
        summary_tr: "Yalnızca yerel ağ (RFC1918) Steam Remote Play / LAN keşif portları; WAN'a açık port açılmaz.",
        summary_en: "RFC1918 private LAN discovery/streaming rules only; zero public WAN port exposure.",
        policy_tr: "SELinux: Enforcing • Firewalld: Aktif • Yalnızca 10/8, 172.16/12, 192.168/16 LAN kuralları (kalıcı değil)",
        policy_en: "SELinux: Enforcing • Firewalld: Active • Scoped RFC1918 LAN rich-rules only (runtime)",
    },
    StaticProfileMeta {
        id: "public-network",
        name_tr: "Açık Ağ (Kafe / Havalimanı)",
        name_en: "Public Network (Untrusted Wi-Fi)",
        zone: "drop / block",
        summary_tr: "Güvenilmeyen ortak Wi-Fi ağlarında tüm gelen bağlantıları ve yerel ağ paylaşım servislerini kilitler.",
        summary_en: "Locks interfaces to drop/block zone and strips incoming discovery services on untrusted Wi-Fi.",
        policy_tr: "SELinux: Enforcing • Arayüzler -> drop bölgesi • ssh/mdns/samba-client/kdeconnect gelen erişimi kapalı",
        policy_en: "SELinux: Enforcing • Active interfaces -> drop zone • Incoming ssh/mdns/samba/kdeconnect stripped",
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

    let root = GtkBox::new(Orientation::Vertical, 16);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let title = Label::new(Some(lang.pick(
        "Bildirimsel Güvenlik Profilleri (Dengeli, Geliştirici, Oyun, Açık Ağ)",
        "Declarative Security Profiles (Balanced, Developer, Gaming, Public Network)",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr_row.append(&title);

    let load_btn = Button::with_label(lang.pick(
        "Profil Durumunu Sorgula (Polkit)",
        "Sync Profile State (Polkit)",
    ));
    load_btn.add_css_class("akrep-ghost-btn");
    let cmd_l = cmd_tx.clone();
    load_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_l, UiCommand::ListProfiles);
    });
    hdr_row.append(&load_btn);
    root.append(&hdr_row);

    let inv_banner = GtkBox::new(Orientation::Horizontal, 10);
    inv_banner.add_css_class("akrep-detail-box");
    let inv_lbl = Label::new(Some(lang.pick(
        "Değişmez Garanti: Hiçbir profil SELinux'u Permissive/Disabled yapmaz, Firewalld'yi kapatmaz, Flatpak kum havuzunu delmez veya Wayland Portal rızasını atlamaz.",
        "Profile Invariant: Every profile keeps SELinux Enforcing, keeps Firewalld active, and preserves Flatpak sandboxes and XDG Portal user consent.",
    )));
    inv_lbl.add_css_class("akrep-subtitle");
    inv_lbl.set_xalign(0.0);
    inv_lbl.set_wrap(true);
    inv_lbl.set_hexpand(true);
    inv_banner.append(&inv_lbl);

    if let Some(last_prof_tx) = history.iter().find(|r| {
        r.state == TransactionState::Committed
            && matches!(r.request, ActionRequest::ApplyProfile { .. })
    }) {
        let rb_btn = Button::with_label(lang.pick(
            "Son Profil Geçişini Geri Al",
            "Rollback Last Profile Switch",
        ));
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
        inv_banner.append(&rb_btn);
    }
    root.append(&inv_banner);

    for meta in BUILTIN_PROFILES {
        let is_active = active_id == meta.id;
        let pcard = GtkBox::new(Orientation::Vertical, 8);
        pcard.add_css_class("akrep-detail-box");

        let top = GtkBox::new(Orientation::Horizontal, 10);
        let pname = Label::new(Some(lang.pick(meta.name_tr, meta.name_en)));
        pname.add_css_class("akrep-section-hdr");
        pname.set_xalign(0.0);
        pname.set_hexpand(true);
        top.append(&pname);

        let z_pill = Label::new(Some(&format!("Zone: {}", meta.zone)));
        z_pill.add_css_class("akrep-ro-pill");
        top.append(&z_pill);

        if is_active {
            let act_b = Label::new(Some(lang.pick("ETKİN PROFİL", "ACTIVE")));
            act_b.add_css_class("akrep-badge");
            act_b.add_css_class("akrep-sev-protected");
            top.append(&act_b);
        }

        let prev_btn =
            Button::with_label(lang.pick("Profil Geçişini Önizle", "Preview Profile Switch"));
        prev_btn.add_css_class("akrep-primary-btn");
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
        sum.set_xalign(0.0);
        sum.set_wrap(true);
        pcard.append(&sum);

        let pol = Label::new(Some(lang.pick(meta.policy_tr, meta.policy_en)));
        pol.add_css_class("akrep-mono");
        pol.set_xalign(0.0);
        pol.set_wrap(true);
        pcard.append(&pol);

        root.append(&pcard);
    }

    // Inline Profile Diff Card — re-opens PlanPreviewDialog so user always sees SHA-256 & warnings
    if let Some(plan) = &preview {
        let diff_card = GtkBox::new(Orientation::Vertical, 8);
        diff_card.add_css_class("akrep-detail-box");

        let dhdr = GtkBox::new(Orientation::Horizontal, 8);
        let dtitle = Label::new(Some(&format!(
            "{} (Plan {})",
            lang.pick(
                "Hazırlanan Profil Geçiş Özeti",
                "Staged Profile Switch Summary"
            ),
            plan.plan_id
        )));
        dtitle.add_css_class("akrep-section-hdr");
        dtitle.set_xalign(0.0);
        dtitle.set_hexpand(true);
        dhdr.append(&dtitle);

        let rbadge = Label::new(Some(risk_label(plan.risk, lang)));
        rbadge.add_css_class("akrep-badge");
        rbadge.add_css_class(risk_css_class(plan.risk));
        dhdr.append(&rbadge);

        let clear_btn = Button::with_label(lang.pick("İptal / Temizle", "Dismiss Preview"));
        clear_btn.add_css_class("akrep-ghost-btn");
        let st_clr = Rc::clone(state);
        let rr_clr = Rc::clone(&on_rerender);
        clear_btn.connect_clicked(move |_| {
            st_clr.borrow_mut().last_profile_preview = None;
            rr_clr();
        });
        dhdr.append(&clear_btn);

        let review_btn = Button::with_label(lang.pick(
            "Plan Onay Penceresini Aç (Polkit)",
            "Review & Approve Plan (Polkit)",
        ));
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
                "[{}] {} -> {} ({})",
                d.target,
                d.current_value,
                d.desired_value,
                lang.pick(&d.description_tr, &d.description_en)
            )));
            drow.add_css_class("akrep-mono");
            drow.set_xalign(0.0);
            drow.set_wrap(true);
            diff_card.append(&drow);
        }
        root.append(&diff_card);
    }

    root
}
