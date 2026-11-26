use crate::components::{
    build_admin_note, build_duration_selector, build_kv_row, build_status_indicator,
    duration_from_choice_idx, format_override_status_text, PROTECTION_DURATION_CHOICES,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::{AppUiState, FirewallRuleInputKind};
use blaze_akrep_common::{ActionRequest, FirewallProto, OverrideDuration, RulePersistence};
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CheckButton, Entry, FlowBox, Label, Orientation, SelectionMode,
};
use std::cell::RefCell;
use std::rc::Rc;

pub fn is_accept_rich_rule(rule: &str) -> bool {
    rule.trim().ends_with(" accept")
}

pub fn extract_source_cidr_from_rich_rule(rule: &str) -> Option<String> {
    let marker = "source address=\"";
    let idx = rule.find(marker)?;
    let rest = &rule[idx + marker.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn extract_port_proto_from_rich_rule(rule: &str) -> Option<(String, FirewallProto)> {
    if !is_accept_rich_rule(rule) {
        return None;
    }
    let p_marker = "port port=\"";
    let p_idx = rule.find(p_marker)?;
    let p_rest = &rule[p_idx + p_marker.len()..];
    let p_end = p_rest.find('"')?;
    let port = p_rest[..p_end].to_string();

    let pr_marker = "protocol=\"";
    let pr_idx = rule.find(pr_marker)?;
    let pr_rest = &rule[pr_idx + pr_marker.len()..];
    let pr_end = pr_rest.find('"')?;
    let proto = match &pr_rest[..pr_end] {
        "udp" => FirewallProto::Udp,
        _ => FirewallProto::Tcp,
    };
    Some((port, proto))
}

pub fn extract_service_from_rich_rule(rule: &str) -> Option<String> {
    if !is_accept_rich_rule(rule) {
        return None;
    }
    let s_marker = "service name=\"";
    let s_idx = rule.find(s_marker)?;
    let s_rest = &rule[s_idx + s_marker.len()..];
    let s_end = s_rest.find('"')?;
    Some(s_rest[..s_end].to_string())
}

pub fn rich_rule_remove_request(zone: &str, rule: &str, permanent: bool) -> Option<ActionRequest> {
    let cidr = extract_source_cidr_from_rich_rule(rule)?;
    if let Some((port, proto)) = extract_port_proto_from_rich_rule(rule) {
        return Some(ActionRequest::FirewallRemovePort {
            zone: zone.to_string(),
            port,
            proto,
            permanent,
            source_cidr: Some(cidr),
        });
    }
    if let Some(service) = extract_service_from_rich_rule(rule) {
        return Some(ActionRequest::FirewallRemoveService {
            zone: zone.to_string(),
            service,
            permanent,
            source_cidr: Some(cidr),
        });
    }
    None
}

pub fn build_firewall_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    on_rerender: Rc<dyn Fn()>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let fw_state = st.firewall_state.clone();
    let draft = st.firewall_draft.clone();
    let firewall_duration_idx = st.firewall_duration_idx;
    drop(st);

    let fw_override = snap.as_ref().and_then(|sn| {
        sn.active_overrides
            .iter()
            .find(|o| o.subsystem == "firewalld")
    });
    let snap_fw_off = snap
        .as_ref()
        .and_then(|sn| sn.cards.iter().find(|c| c.id == "firewall"))
        .is_some_and(|c| c.metric.eq_ignore_ascii_case("OFF"));
    let is_paused =
        fw_override.is_some() || fw_state.as_ref().is_some_and(|fw| !fw.running) || snap_fw_off;

    let root = GtkBox::new(Orientation::Vertical, 20);

    let page_desc = Label::new(Some(lang.pick(
        "Firewalld durumu, bölgeler, ağ arayüzleri ve süreli port kuralları",
        "Firewalld state, zones, network interfaces and timed port rules",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    root.append(&page_desc);

    // 1. Firewalld State, Pause/Resume & Active Zones Card
    let status_card = GtkBox::new(Orientation::Vertical, 12);
    status_card.add_css_class("akrep-detail-box");

    let s_hdr = GtkBox::new(Orientation::Horizontal, 12);
    s_hdr.set_valign(Align::Center);
    let s_title = Label::new(Some(lang.pick(
        "Güvenlik duvarı durumu ve bölgeler",
        "Firewall state and zones",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    s_title.set_wrap(true);
    s_title.set_hexpand(true);
    s_hdr.append(&s_title);

    let inspect_btn = Button::with_label(lang.pick("Kuralları yükle", "Load rules"));
    inspect_btn.add_css_class("akrep-ghost-btn");
    inspect_btn.set_valign(Align::Center);
    let cmd_insp = cmd_tx.clone();
    inspect_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_insp,
            UiCommand::InspectFirewall {
                zone_filter: String::new(),
            },
        );
    });
    s_hdr.append(&inspect_btn);
    status_card.append(&s_hdr);

    let state_box = GtkBox::new(Orientation::Vertical, 8);
    state_box.add_css_class("akrep-diff-row");

    if is_paused {
        let state_top = GtkBox::new(Orientation::Horizontal, 12);
        state_top.set_valign(Align::Center);
        let dot_cls = if fw_override.is_some_and(|o| o.duration_kind == "until_changed") {
            "akrep-sev-critical"
        } else {
            "akrep-sev-attention"
        };
        let status_txt = if let Some(ov) = fw_override {
            format!(
                "{} · {}",
                lang.pick("Duraklatıldı", "Paused"),
                format_override_status_text(ov, lang)
            )
        } else {
            lang.pick("Firewalld pasif", "Firewalld inactive")
                .to_string()
        };
        let ind = build_status_indicator(dot_cls, &status_txt, false);
        ind.set_hexpand(true);
        state_top.append(&ind);

        let enable_btn = Button::with_label(lang.pick("Etkinleştir", "Enable"));
        enable_btn.add_css_class("akrep-primary-btn");
        let cmd_en = cmd_tx.clone();
        enable_btn.connect_clicked(move |_| {
            dispatch_cmd(
                &cmd_en,
                UiCommand::CreatePlan {
                    request: ActionRequest::FirewallSetState {
                        enabled: true,
                        duration: OverrideDuration::UntilChanged,
                    },
                },
            );
        });
        state_top.append(&enable_btn);
        state_box.append(&state_top);

        let dur_row = GtkBox::new(Orientation::Horizontal, 8);
        dur_row.set_valign(Align::Center);
        let st_fd = Rc::clone(state);
        dur_row.append(&build_duration_selector(
            &PROTECTION_DURATION_CHOICES,
            firewall_duration_idx,
            lang,
            Rc::new(move |idx| {
                st_fd.borrow_mut().firewall_duration_idx = idx;
            }),
        ));

        let upd_btn = Button::with_label(lang.pick("Süreyi güncelle", "Update duration"));
        upd_btn.add_css_class("akrep-ghost-btn");
        upd_btn.set_valign(Align::Center);
        let st_upd = Rc::clone(state);
        let cmd_upd = cmd_tx.clone();
        upd_btn.connect_clicked(move |_| {
            let idx = st_upd.borrow().firewall_duration_idx;
            let duration = duration_from_choice_idx(&PROTECTION_DURATION_CHOICES, idx);
            dispatch_cmd(
                &cmd_upd,
                UiCommand::CreatePlan {
                    request: ActionRequest::FirewallSetState {
                        enabled: false,
                        duration,
                    },
                },
            );
        });
        dur_row.append(&upd_btn);
        state_box.append(&dur_row);
    } else {
        let active_txt = if fw_state.is_some() || snap.is_some() {
            (
                "akrep-sev-protected",
                lang.pick("Firewalld etkin", "Firewalld active"),
                false,
            )
        } else {
            (
                "akrep-sev-unavailable",
                lang.pick("Durum bekleniyor", "Awaiting state"),
                true,
            )
        };
        state_box.append(&build_status_indicator(
            active_txt.0,
            active_txt.1,
            active_txt.2,
        ));

        let dur_row = GtkBox::new(Orientation::Horizontal, 8);
        dur_row.set_valign(Align::Center);
        let st_fd = Rc::clone(state);
        dur_row.append(&build_duration_selector(
            &PROTECTION_DURATION_CHOICES,
            firewall_duration_idx,
            lang,
            Rc::new(move |idx| {
                st_fd.borrow_mut().firewall_duration_idx = idx;
            }),
        ));

        let pause_btn = Button::with_label(lang.pick("Duraklat", "Pause"));
        pause_btn.add_css_class("akrep-ghost-btn");
        pause_btn.set_valign(Align::Center);
        let st_p = Rc::clone(state);
        let cmd_p = cmd_tx.clone();
        pause_btn.connect_clicked(move |_| {
            let idx = st_p.borrow().firewall_duration_idx;
            let duration = duration_from_choice_idx(&PROTECTION_DURATION_CHOICES, idx);
            dispatch_cmd(
                &cmd_p,
                UiCommand::CreatePlan {
                    request: ActionRequest::FirewallSetState {
                        enabled: false,
                        duration,
                    },
                },
            );
        });
        dur_row.append(&pause_btn);
        state_box.append(&dur_row);
    }
    status_card.append(&state_box);

    if let Some(fw) = &fw_state {
        status_card.append(&build_kv_row(
            lang.pick("Varsayılan bölge", "Default zone"),
            &fw.default_zone,
        ));
        let az_str = if fw.active_zones.is_empty() {
            lang.pick("Etkin arayüz ataması yok", "No active interface bindings")
                .to_string()
        } else {
            fw.active_zones
                .iter()
                .map(|(iface, z)| format!("{} → {}", iface, z))
                .collect::<Vec<_>>()
                .join(", ")
        };
        status_card.append(&build_kv_row(
            lang.pick("Arayüz eşleşmeleri", "Interface bindings"),
            &az_str,
        ));
    } else if let Some(sn) = &snap {
        if let Some(sec) = sn.sections.iter().find(|x| x.section == "firewall") {
            for m in &sec.metrics {
                status_card.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    }

    let bind_inputs = GtkBox::new(Orientation::Horizontal, 8);
    let iface_entry = Entry::new();
    iface_entry.add_css_class("akrep-entry");
    iface_entry.set_width_chars(10);
    iface_entry.set_hexpand(true);
    iface_entry.set_placeholder_text(Some(lang.pick(
        "Arayüz (eth0, wlan0)",
        "Interface (eth0, wlan0)",
    )));
    iface_entry.set_text(&draft.bind_interface);
    let st_if = Rc::clone(state);
    iface_entry.connect_changed(move |ent| {
        st_if.borrow_mut().firewall_draft.bind_interface = ent.text().to_string();
    });
    bind_inputs.append(&iface_entry);

    let bzone_entry = Entry::new();
    bzone_entry.add_css_class("akrep-entry");
    bzone_entry.set_width_chars(10);
    bzone_entry.set_hexpand(true);
    bzone_entry.set_placeholder_text(Some(lang.pick(
        "Bölge (public, home, drop)",
        "Zone (public, home, drop)",
    )));
    bzone_entry.set_text(&draft.bind_zone);
    let st_bz = Rc::clone(state);
    bzone_entry.connect_changed(move |ent| {
        st_bz.borrow_mut().firewall_draft.bind_zone = ent.text().to_string();
    });
    bind_inputs.append(&bzone_entry);
    status_card.append(&bind_inputs);

    let bind_actions = GtkBox::new(Orientation::Horizontal, 12);
    bind_actions.set_valign(Align::Center);
    let bperm_chk = CheckButton::with_label(lang.pick("Kalıcı", "Permanent"));
    bperm_chk.set_active(draft.bind_permanent);
    let st_bp = Rc::clone(state);
    bperm_chk.connect_toggled(move |chk| {
        st_bp.borrow_mut().firewall_draft.bind_permanent = chk.is_active();
    });
    bind_actions.append(&bperm_chk);

    let bind_spacer = GtkBox::new(Orientation::Horizontal, 0);
    bind_spacer.set_hexpand(true);
    bind_actions.append(&bind_spacer);

    let bind_btn = Button::with_label(lang.pick("Bölgeye bağla", "Bind to zone"));
    bind_btn.add_css_class("akrep-ghost-btn");
    let cmd_bind = cmd_tx.clone();
    let st_bind = Rc::clone(state);
    bind_btn.connect_clicked(move |_| {
        let d = st_bind.borrow().firewall_draft.clone();
        dispatch_cmd(
            &cmd_bind,
            UiCommand::CreatePlan {
                request: ActionRequest::FirewallBindInterface {
                    zone: d.bind_zone.trim().to_string(),
                    interface: d.bind_interface.trim().to_string(),
                    permanent: d.bind_permanent,
                },
            },
        );
    });
    bind_actions.append(&bind_btn);
    status_card.append(&bind_actions);

    let guard_lbl = Label::new(Some(lang.pick(
        "Güvenlik duvarını süreli veya yeniden etkinleştirene kadar duraklatabilir, bölge kuralları ekleyebilirsiniz",
        "You can pause the firewall temporarily or until re-enabled, and manage zone rules",
    )));
    guard_lbl.add_css_class("akrep-meta");
    guard_lbl.set_xalign(0.0);
    guard_lbl.set_wrap(true);
    guard_lbl.set_wrap_mode(WrapMode::WordChar);
    status_card.append(&guard_lbl);
    status_card.append(&build_admin_note(lang));
    root.append(&status_card);

    // 2. Quick Rule Builder Card
    let builder_card = GtkBox::new(Orientation::Vertical, 12);
    builder_card.add_css_class("akrep-detail-box");

    let b_title = Label::new(Some(lang.pick("Kural oluşturucu", "Rule builder")));
    b_title.add_css_class("akrep-section-hdr");
    b_title.set_xalign(0.0);
    builder_card.append(&b_title);

    let preset_lbl = Label::new(Some(lang.pick("Hazır şablonlar", "Quick presets")));
    preset_lbl.add_css_class("akrep-meta");
    preset_lbl.set_xalign(0.0);
    builder_card.append(&preset_lbl);

    let preset_flow = FlowBox::new();
    preset_flow.set_selection_mode(SelectionMode::None);
    preset_flow.set_homogeneous(false);
    preset_flow.set_column_spacing(8);
    preset_flow.set_row_spacing(8);
    preset_flow.set_max_children_per_line(4);
    preset_flow.set_min_children_per_line(1);

    for (label_tr, label_en, kind, spec, proto, cidr) in [
        (
            "Minecraft (25565/tcp)",
            "Minecraft (25565/tcp)",
            FirewallRuleInputKind::Port,
            "25565",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "KDE Connect (1714-1764/tcp)",
            "KDE Connect (1714-1764/tcp)",
            FirewallRuleInputKind::Port,
            "1714-1764",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "SSH (yerel ağ)",
            "SSH (local network)",
            FirewallRuleInputKind::Service,
            "ssh",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "HTTPS (443/tcp)",
            "HTTPS (443/tcp)",
            FirewallRuleInputKind::Port,
            "443",
            FirewallProto::Tcp,
            "",
        ),
        (
            "Samba (yerel ağ)",
            "Samba (local network)",
            FirewallRuleInputKind::Service,
            "samba",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "Web geliştirme (3000/tcp)",
            "Web dev (3000/tcp)",
            FirewallRuleInputKind::Port,
            "3000",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "Web geliştirme (8080/tcp)",
            "Web dev (8080/tcp)",
            FirewallRuleInputKind::Port,
            "8080",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "Jellyfin (8096/tcp)",
            "Jellyfin (8096/tcp)",
            FirewallRuleInputKind::Port,
            "8096",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
    ] {
        let pbtn = Button::with_label(lang.pick(label_tr, label_en));
        pbtn.add_css_class("akrep-ghost-btn");
        let st_pr = Rc::clone(state);
        let rr = Rc::clone(&on_rerender);
        pbtn.connect_clicked(move |_| {
            {
                let mut s = st_pr.borrow_mut();
                s.firewall_draft.input_kind = kind;
                s.firewall_draft.port_or_service = spec.to_string();
                s.firewall_draft.proto = proto;
                s.firewall_draft.source_cidr = cidr.to_string();
            }
            rr();
        });
        preset_flow.insert(&pbtn, -1);
    }
    builder_card.append(&preset_flow);

    // Row 1: Zone, Kind, Target, Proto
    let row1 = GtkBox::new(Orientation::Horizontal, 8);
    let zone_entry = Entry::new();
    zone_entry.add_css_class("akrep-entry");
    zone_entry.set_width_chars(10);
    zone_entry.set_placeholder_text(Some(lang.pick("Bölge", "Zone")));
    zone_entry.set_text(&draft.zone);
    let st_z = Rc::clone(state);
    zone_entry.connect_changed(move |ent| {
        st_z.borrow_mut().firewall_draft.zone = ent.text().to_string();
    });
    row1.append(&zone_entry);

    let kind_port_btn = Button::with_label(lang.pick("Port", "Port"));
    kind_port_btn.add_css_class(if draft.input_kind == FirewallRuleInputKind::Port {
        "akrep-seg-active"
    } else {
        "akrep-seg-btn"
    });
    let st_kp = Rc::clone(state);
    let rr_kp = Rc::clone(&on_rerender);
    kind_port_btn.connect_clicked(move |_| {
        st_kp.borrow_mut().firewall_draft.input_kind = FirewallRuleInputKind::Port;
        rr_kp();
    });
    row1.append(&kind_port_btn);

    let kind_svc_btn = Button::with_label(lang.pick("Servis", "Service"));
    kind_svc_btn.add_css_class(if draft.input_kind == FirewallRuleInputKind::Service {
        "akrep-seg-active"
    } else {
        "akrep-seg-btn"
    });
    let st_ks = Rc::clone(state);
    let rr_ks = Rc::clone(&on_rerender);
    kind_svc_btn.connect_clicked(move |_| {
        st_ks.borrow_mut().firewall_draft.input_kind = FirewallRuleInputKind::Service;
        rr_ks();
    });
    row1.append(&kind_svc_btn);

    let target_entry = Entry::new();
    target_entry.add_css_class("akrep-entry");
    target_entry.set_width_chars(10);
    target_entry.set_hexpand(true);
    target_entry.set_placeholder_text(Some(match draft.input_kind {
        FirewallRuleInputKind::Port => "25565",
        FirewallRuleInputKind::Service => "ssh, http, mdns",
    }));
    target_entry.set_text(&draft.port_or_service);
    let st_tg = Rc::clone(state);
    target_entry.connect_changed(move |ent| {
        st_tg.borrow_mut().firewall_draft.port_or_service = ent.text().to_string();
    });
    row1.append(&target_entry);

    if draft.input_kind == FirewallRuleInputKind::Port {
        for (plbl, pval) in [("TCP", FirewallProto::Tcp), ("UDP", FirewallProto::Udp)] {
            let b = Button::with_label(plbl);
            b.add_css_class(if draft.proto == pval {
                "akrep-seg-active"
            } else {
                "akrep-seg-btn"
            });
            let st_pr = Rc::clone(state);
            let rr_pr = Rc::clone(&on_rerender);
            b.connect_clicked(move |_| {
                st_pr.borrow_mut().firewall_draft.proto = pval;
                rr_pr();
            });
            row1.append(&b);
        }
    }
    builder_card.append(&row1);

    // Row 2: Persistence mode segmented bar
    let row2 = GtkBox::new(Orientation::Horizontal, 8);
    let modes = [
        (lang.pick("Geçici", "Temporary"), RulePersistence::Runtime),
        (
            lang.pick("15 dk", "15m"),
            RulePersistence::TimedRuntime { seconds: 900 },
        ),
        (
            lang.pick("1 sa", "1h"),
            RulePersistence::TimedRuntime { seconds: 3600 },
        ),
        (
            lang.pick("4 sa", "4h"),
            RulePersistence::TimedRuntime { seconds: 14400 },
        ),
        (lang.pick("Kalıcı", "Permanent"), RulePersistence::Permanent),
    ];
    for (mlbl, mval) in modes {
        let mb = Button::with_label(mlbl);
        mb.add_css_class(if draft.persistence == mval {
            "akrep-seg-active"
        } else {
            "akrep-seg-btn"
        });
        let st_m = Rc::clone(state);
        let rr_m = Rc::clone(&on_rerender);
        mb.connect_clicked(move |_| {
            st_m.borrow_mut().firewall_draft.persistence = mval;
            rr_m();
        });
        row2.append(&mb);
    }
    builder_card.append(&row2);

    // Row 3: Optional source network + primary preview action
    let row3 = GtkBox::new(Orientation::Horizontal, 8);
    let cidr_entry = Entry::new();
    cidr_entry.add_css_class("akrep-entry");
    cidr_entry.set_width_chars(12);
    cidr_entry.set_hexpand(true);
    cidr_entry.set_placeholder_text(Some(lang.pick(
        "İsteğe bağlı kaynak ağ (192.168.1.0/24)",
        "Optional source network (192.168.1.0/24)",
    )));
    cidr_entry.set_text(&draft.source_cidr);
    let st_cd = Rc::clone(state);
    cidr_entry.connect_changed(move |ent| {
        st_cd.borrow_mut().firewall_draft.source_cidr = ent.text().to_string();
    });
    row3.append(&cidr_entry);

    let add_btn = Button::with_label(lang.pick("Önizle", "Preview"));
    add_btn.add_css_class(if is_paused {
        "akrep-ghost-btn"
    } else {
        "akrep-primary-btn"
    });
    let cmd_add = cmd_tx.clone();
    let st_add = Rc::clone(state);
    add_btn.connect_clicked(move |_| {
        let d = st_add.borrow().firewall_draft.clone();
        let cidr_opt = {
            let t = d.source_cidr.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        };
        let req = match d.input_kind {
            FirewallRuleInputKind::Port => ActionRequest::FirewallAddPort {
                zone: d.zone.trim().to_string(),
                port: d.port_or_service.trim().to_string(),
                proto: d.proto,
                mode: d.persistence,
                source_cidr: cidr_opt,
            },
            FirewallRuleInputKind::Service => ActionRequest::FirewallAddService {
                zone: d.zone.trim().to_string(),
                service: d.port_or_service.trim().to_string(),
                mode: d.persistence,
                source_cidr: cidr_opt,
            },
        };
        dispatch_cmd(&cmd_add, UiCommand::CreatePlan { request: req });
    });
    row3.append(&add_btn);
    builder_card.append(&row3);
    root.append(&builder_card);

    // 3. Existing Rules List
    let rules_card = GtkBox::new(Orientation::Vertical, 12);
    rules_card.add_css_class("akrep-detail-box");
    let r_title = Label::new(Some(lang.pick(
        "Mevcut bölge kuralları",
        "Existing zone rules",
    )));
    r_title.add_css_class("akrep-section-hdr");
    r_title.set_xalign(0.0);
    rules_card.append(&r_title);

    if let Some(fw) = &fw_state {
        for z in &fw.zone_details {
            let has_rules = !z.runtime_ports.is_empty()
                || !z.permanent_ports.is_empty()
                || !z.runtime_services.is_empty()
                || !z.permanent_services.is_empty()
                || !z.runtime_rich_rules.is_empty()
                || !z.permanent_rich_rules.is_empty();
            if !z.active && !z.is_default && !has_rules {
                continue;
            }
            let z_box = GtkBox::new(Orientation::Vertical, 8);
            z_box.add_css_class("akrep-diff-row");

            let z_top = GtkBox::new(Orientation::Horizontal, 8);
            z_top.set_valign(Align::Center);
            let z_hdr = Label::new(Some(&format!(
                "{}: {}",
                lang.pick("Bölge", "Zone"),
                z.name
            )));
            z_hdr.add_css_class("akrep-card-title");
            z_hdr.set_xalign(0.0);
            z_hdr.set_hexpand(true);
            z_top.append(&z_hdr);

            if z.is_default {
                let dtag = Label::new(Some(lang.pick("Varsayılan", "Default")));
                dtag.add_css_class("akrep-tag");
                z_top.append(&dtag);
            }
            if z.active {
                let atag = Label::new(Some(lang.pick("Etkin", "Active")));
                atag.add_css_class("akrep-tag");
                z_top.append(&atag);
            }
            z_box.append(&z_top);

            let scope_rt = lang.pick("Geçici", "Temporary");
            let scope_perm = lang.pick("Kalıcı", "Permanent");
            let scope_both = lang.pick("Geçici ve kalıcı", "Temporary & permanent");

            // Runtime (+ Permanent) ports
            for (port, proto_str) in &z.runtime_ports {
                let is_perm = z
                    .permanent_ports
                    .iter()
                    .any(|(pp, pr)| pp == port && pr.eq_ignore_ascii_case(proto_str));
                let proto = if proto_str.eq_ignore_ascii_case("udp") {
                    FirewallProto::Udp
                } else {
                    FirewallProto::Tcp
                };
                let prow = GtkBox::new(Orientation::Horizontal, 8);
                prow.set_valign(Align::Center);
                let plbl = Label::new(Some(&format!(
                    "{}: {}/{} ({})",
                    lang.pick("Port", "Port"),
                    port,
                    proto.as_str(),
                    if is_perm { scope_both } else { scope_rt }
                )));
                plbl.add_css_class("akrep-mono");
                plbl.set_xalign(0.0);
                plbl.set_wrap(true);
                plbl.set_wrap_mode(WrapMode::WordChar);
                plbl.set_hexpand(true);
                prow.append(&plbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-danger-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let pval = port.clone();
                rm_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rm,
                        UiCommand::CreatePlan {
                            request: ActionRequest::FirewallRemovePort {
                                zone: zname.clone(),
                                port: pval.clone(),
                                proto,
                                permanent: is_perm,
                                source_cidr: None,
                            },
                        },
                    );
                });
                prow.append(&rm_btn);
                z_box.append(&prow);
            }

            // Permanent-only ports
            for (port, proto_str) in &z.permanent_ports {
                let in_rt = z
                    .runtime_ports
                    .iter()
                    .any(|(rp, rr)| rp == port && rr.eq_ignore_ascii_case(proto_str));
                if in_rt {
                    continue;
                }
                let proto = if proto_str.eq_ignore_ascii_case("udp") {
                    FirewallProto::Udp
                } else {
                    FirewallProto::Tcp
                };
                let prow = GtkBox::new(Orientation::Horizontal, 8);
                prow.set_valign(Align::Center);
                let plbl = Label::new(Some(&format!(
                    "{}: {}/{} ({})",
                    lang.pick("Port", "Port"),
                    port,
                    proto.as_str(),
                    scope_perm
                )));
                plbl.add_css_class("akrep-mono");
                plbl.set_xalign(0.0);
                plbl.set_wrap(true);
                plbl.set_wrap_mode(WrapMode::WordChar);
                plbl.set_hexpand(true);
                prow.append(&plbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-danger-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let pval = port.clone();
                rm_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rm,
                        UiCommand::CreatePlan {
                            request: ActionRequest::FirewallRemovePort {
                                zone: zname.clone(),
                                port: pval.clone(),
                                proto,
                                permanent: true,
                                source_cidr: None,
                            },
                        },
                    );
                });
                prow.append(&rm_btn);
                z_box.append(&prow);
            }

            // Runtime (+ Permanent) services
            for svc in &z.runtime_services {
                let is_perm = z.permanent_services.contains(svc);
                let srow = GtkBox::new(Orientation::Horizontal, 8);
                srow.set_valign(Align::Center);
                let slbl = Label::new(Some(&format!(
                    "{}: {} ({})",
                    lang.pick("Servis", "Service"),
                    svc,
                    if is_perm { scope_both } else { scope_rt }
                )));
                slbl.add_css_class("akrep-mono");
                slbl.set_xalign(0.0);
                slbl.set_wrap(true);
                slbl.set_wrap_mode(WrapMode::WordChar);
                slbl.set_hexpand(true);
                srow.append(&slbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-ghost-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let sval = svc.clone();
                rm_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rm,
                        UiCommand::CreatePlan {
                            request: ActionRequest::FirewallRemoveService {
                                zone: zname.clone(),
                                service: sval.clone(),
                                permanent: is_perm,
                                source_cidr: None,
                            },
                        },
                    );
                });
                srow.append(&rm_btn);
                z_box.append(&srow);
            }

            // Permanent-only services
            for svc in &z.permanent_services {
                if z.runtime_services.contains(svc) {
                    continue;
                }
                let srow = GtkBox::new(Orientation::Horizontal, 8);
                srow.set_valign(Align::Center);
                let slbl = Label::new(Some(&format!(
                    "{}: {} ({})",
                    lang.pick("Servis", "Service"),
                    svc,
                    scope_perm
                )));
                slbl.add_css_class("akrep-mono");
                slbl.set_xalign(0.0);
                slbl.set_wrap(true);
                slbl.set_wrap_mode(WrapMode::WordChar);
                slbl.set_hexpand(true);
                srow.append(&slbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-ghost-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let sval = svc.clone();
                rm_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rm,
                        UiCommand::CreatePlan {
                            request: ActionRequest::FirewallRemoveService {
                                zone: zname.clone(),
                                service: sval.clone(),
                                permanent: true,
                                source_cidr: None,
                            },
                        },
                    );
                });
                srow.append(&rm_btn);
                z_box.append(&srow);
            }

            // Runtime (+ Permanent) scoped rules
            for rr in &z.runtime_rich_rules {
                let is_perm = z.permanent_rich_rules.contains(rr);
                let rrow = GtkBox::new(Orientation::Horizontal, 8);
                rrow.set_valign(Align::Center);
                let rlbl = Label::new(Some(&format!(
                    "{} ({}): {}",
                    lang.pick("Özel kural", "Custom rule"),
                    if is_perm { scope_both } else { scope_rt },
                    rr
                )));
                rlbl.add_css_class("akrep-mono");
                rlbl.set_xalign(0.0);
                rlbl.set_wrap(true);
                rlbl.set_wrap_mode(WrapMode::WordChar);
                rlbl.set_hexpand(true);
                rrow.append(&rlbl);

                if let Some(rm_req) = rich_rule_remove_request(&z.name, rr, is_perm) {
                    let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                    rm_btn.add_css_class("akrep-danger-btn");
                    let cmd_rm = cmd_tx.clone();
                    rm_btn.connect_clicked(move |_| {
                        dispatch_cmd(
                            &cmd_rm,
                            UiCommand::CreatePlan {
                                request: rm_req.clone(),
                            },
                        );
                    });
                    rrow.append(&rm_btn);
                }
                z_box.append(&rrow);
            }

            // Permanent-only scoped rules
            for rr in &z.permanent_rich_rules {
                if z.runtime_rich_rules.contains(rr) {
                    continue;
                }
                let rrow = GtkBox::new(Orientation::Horizontal, 8);
                rrow.set_valign(Align::Center);
                let rlbl = Label::new(Some(&format!(
                    "{} ({}): {}",
                    lang.pick("Özel kural", "Custom rule"),
                    scope_perm,
                    rr
                )));
                rlbl.add_css_class("akrep-mono");
                rlbl.set_xalign(0.0);
                rlbl.set_wrap(true);
                rlbl.set_wrap_mode(WrapMode::WordChar);
                rlbl.set_hexpand(true);
                rrow.append(&rlbl);

                if let Some(rm_req) = rich_rule_remove_request(&z.name, rr, true) {
                    let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                    rm_btn.add_css_class("akrep-danger-btn");
                    let cmd_rm = cmd_tx.clone();
                    rm_btn.connect_clicked(move |_| {
                        dispatch_cmd(
                            &cmd_rm,
                            UiCommand::CreatePlan {
                                request: rm_req.clone(),
                            },
                        );
                    });
                    rrow.append(&rm_btn);
                }
                z_box.append(&rrow);
            }

            rules_card.append(&z_box);
        }
    } else {
        let hint = Label::new(Some(lang.pick(
            "Bölge portlarını, servislerini ve özel kuralları görmek için üstteki Kuralları yükle düğmesine tıklayın",
            "Select Load rules above to inspect and manage per-zone ports, services and custom rules",
        )));
        hint.add_css_class("akrep-subtitle");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        rules_card.append(&hint);
    }

    root.append(&rules_card);
    root
}
