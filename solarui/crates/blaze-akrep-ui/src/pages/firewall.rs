use crate::components::{build_kv_row, severity_css_class};
use crate::dbus_client::UiCommand;
use crate::state::{AppUiState, FirewallRuleInputKind};
use blaze_akrep_common::{ActionRequest, FirewallProto, RulePersistence};
use blaze_sentinel_core::AkrepSeverity;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub fn extract_source_cidr_from_rich_rule(rule: &str) -> Option<String> {
    let marker = "source address=\"";
    let idx = rule.find(marker)?;
    let rest = &rule[idx + marker.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn extract_port_proto_from_rich_rule(rule: &str) -> Option<(String, FirewallProto)> {
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
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let title = Label::new(Some(lang.pick(
        "Güvenlik Duvarı (Firewalld Bölgeleri, Süreli Port & LAN CIDR Yönetimi)",
        "Firewall (Firewalld Zones, Timed Ports & Scoped LAN CIDR Rules)",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    root.append(&title);

    // 1. Active Zones & Interface Bindings Card (NO disable firewalld button!)
    let status_card = GtkBox::new(Orientation::Vertical, 10);
    status_card.add_css_class("akrep-detail-box");

    let s_hdr = GtkBox::new(Orientation::Horizontal, 8);
    let s_title = Label::new(Some(lang.pick(
        "Etkin Bölgeler ve Ağ Arayüzü Bağlamaları",
        "Active Zones & Network Interface Bindings",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    s_title.set_hexpand(true);
    s_hdr.append(&s_title);

    let redline_pill = Label::new(Some(
        lang.pick("Firewalld Kapatma Yasaklı", "Firewalld Disable Forbidden"),
    ));
    redline_pill.add_css_class("akrep-ro-pill");
    s_hdr.append(&redline_pill);

    let inspect_btn =
        Button::with_label(lang.pick("Tüm Kuralları Yükle (Polkit)", "Inspect Rules (Polkit)"));
    inspect_btn.add_css_class("akrep-primary-btn");
    let cmd_insp = cmd_tx.clone();
    inspect_btn.connect_clicked(move |_| {
        let _ = cmd_insp.send_blocking(UiCommand::InspectFirewall {
            zone_filter: String::new(),
        });
    });
    s_hdr.append(&inspect_btn);
    status_card.append(&s_hdr);

    if let Some(fw) = &fw_state {
        let sev = if fw.running {
            AkrepSeverity::Protected
        } else {
            AkrepSeverity::Critical
        };
        let b = Label::new(Some(if fw.running {
            lang.pick("Firewalld Çalışıyor", "Firewalld Running")
        } else {
            lang.pick("Firewalld Pasif", "Firewalld Inactive")
        }));
        b.add_css_class("akrep-badge");
        b.add_css_class(severity_css_class(sev));
        b.set_halign(Align::Start);
        status_card.append(&b);

        status_card.append(&build_kv_row(
            lang.pick("Varsayılan Bölge", "Default Zone"),
            &fw.default_zone,
        ));
        let az_str = if fw.active_zones.is_empty() {
            lang.pick("Etkin arayüz ataması yok", "No active interface bindings")
                .to_string()
        } else {
            fw.active_zones
                .iter()
                .map(|(iface, z)| format!("{} -> {}", iface, z))
                .collect::<Vec<_>>()
                .join(", ")
        };
        status_card.append(&build_kv_row(
            lang.pick("Arayüz -> Bölge Eşleşmeleri", "Interface -> Zone Map"),
            &az_str,
        ));
    } else if let Some(sn) = &snap {
        if let Some(sec) = sn.sections.iter().find(|x| x.section == "firewall") {
            for m in &sec.metrics {
                status_card.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    }

    let bind_row = GtkBox::new(Orientation::Horizontal, 8);
    let iface_entry = Entry::new();
    iface_entry.add_css_class("akrep-entry");
    iface_entry.set_placeholder_text(Some(
        lang.pick("Arayüz (örn: eth0, wlan0)", "Interface (e.g. eth0, wlan0)"),
    ));
    iface_entry.set_text(&draft.bind_interface);
    let st_if = Rc::clone(state);
    iface_entry.connect_changed(move |ent| {
        st_if.borrow_mut().firewall_draft.bind_interface = ent.text().to_string();
    });
    bind_row.append(&iface_entry);

    let bzone_entry = Entry::new();
    bzone_entry.add_css_class("akrep-entry");
    bzone_entry.set_placeholder_text(Some("public / drop / home"));
    bzone_entry.set_text(&draft.bind_zone);
    let st_bz = Rc::clone(state);
    bzone_entry.connect_changed(move |ent| {
        st_bz.borrow_mut().firewall_draft.bind_zone = ent.text().to_string();
    });
    bind_row.append(&bzone_entry);

    let bperm_chk = CheckButton::with_label(lang.pick("Kalıcı", "Permanent"));
    bperm_chk.set_active(draft.bind_permanent);
    let st_bp = Rc::clone(state);
    bperm_chk.connect_toggled(move |chk| {
        st_bp.borrow_mut().firewall_draft.bind_permanent = chk.is_active();
    });
    bind_row.append(&bperm_chk);

    let bind_btn =
        Button::with_label(lang.pick("Bölgeye Bağla (Önizle)", "Bind Interface (Preview)"));
    bind_btn.add_css_class("akrep-ghost-btn");
    let cmd_bind = cmd_tx.clone();
    let st_bind = Rc::clone(state);
    bind_btn.connect_clicked(move |_| {
        let d = st_bind.borrow().firewall_draft.clone();
        let _ = cmd_bind.send_blocking(UiCommand::CreatePlan {
            request: ActionRequest::FirewallBindInterface {
                zone: d.bind_zone.trim().to_string(),
                interface: d.bind_interface.trim().to_string(),
                permanent: d.bind_permanent,
            },
        });
    });
    bind_row.append(&bind_btn);
    status_card.append(&bind_row);
    root.append(&status_card);

    // 2. Quick Rule Builder Card
    let builder_card = GtkBox::new(Orientation::Vertical, 10);
    builder_card.add_css_class("akrep-detail-box");

    let b_title = Label::new(Some(lang.pick(
        "Hızlı ve Kapsamlı Kural Oluşturucu",
        "Scoped Firewall Rule Builder",
    )));
    b_title.add_css_class("akrep-section-hdr");
    b_title.set_xalign(0.0);
    builder_card.append(&b_title);

    // Preset chips
    let preset_row = GtkBox::new(Orientation::Horizontal, 8);
    let preset_lbl = Label::new(Some(lang.pick("Hazır Şablonlar:", "Quick Presets:")));
    preset_lbl.add_css_class("akrep-subtitle");
    preset_row.append(&preset_lbl);

    for (label, kind, spec, proto, cidr) in [
        (
            "Minecraft LAN (25565/tcp)",
            FirewallRuleInputKind::Port,
            "25565",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "KDE Connect (1714-1764/tcp)",
            FirewallRuleInputKind::Port,
            "1714-1764",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "SSH (LAN)",
            FirewallRuleInputKind::Service,
            "ssh",
            FirewallProto::Tcp,
            "192.168.1.0/24",
        ),
        (
            "HTTPS (443/tcp)",
            FirewallRuleInputKind::Port,
            "443",
            FirewallProto::Tcp,
            "",
        ),
    ] {
        let pbtn = Button::with_label(label);
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
        preset_row.append(&pbtn);
    }
    builder_card.append(&preset_row);

    let row1 = GtkBox::new(Orientation::Horizontal, 8);
    let zone_entry = Entry::new();
    zone_entry.add_css_class("akrep-entry");
    zone_entry.set_placeholder_text(Some("Zone (public / FedoraWorkstation)"));
    zone_entry.set_text(&draft.zone);
    let st_z = Rc::clone(state);
    zone_entry.connect_changed(move |ent| {
        st_z.borrow_mut().firewall_draft.zone = ent.text().to_string();
    });
    row1.append(&zone_entry);

    let kind_port_btn = Button::with_label(lang.pick("Port", "Port"));
    kind_port_btn.add_css_class(if draft.input_kind == FirewallRuleInputKind::Port {
        "akrep-primary-btn"
    } else {
        "akrep-ghost-btn"
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
        "akrep-primary-btn"
    } else {
        "akrep-ghost-btn"
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
    target_entry.set_hexpand(true);
    target_entry.set_placeholder_text(Some(match draft.input_kind {
        FirewallRuleInputKind::Port => "25565 or 1714-1764",
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
                "akrep-primary-btn"
            } else {
                "akrep-ghost-btn"
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

    let row2 = GtkBox::new(Orientation::Horizontal, 8);
    let modes = [
        (
            lang.pick("Geçici (Runtime)", "Runtime"),
            RulePersistence::Runtime,
        ),
        (
            lang.pick("Süreli 15dk", "Timed 15m"),
            RulePersistence::TimedRuntime { seconds: 900 },
        ),
        (
            lang.pick("Süreli 1sa", "Timed 1h"),
            RulePersistence::TimedRuntime { seconds: 3600 },
        ),
        (
            lang.pick("Süreli 4sa", "Timed 4h"),
            RulePersistence::TimedRuntime { seconds: 14400 },
        ),
        (lang.pick("Kalıcı", "Permanent"), RulePersistence::Permanent),
    ];
    for (mlbl, mval) in modes {
        let mb = Button::with_label(mlbl);
        mb.add_css_class(if draft.persistence == mval {
            "akrep-primary-btn"
        } else {
            "akrep-ghost-btn"
        });
        let st_m = Rc::clone(state);
        let rr_m = Rc::clone(&on_rerender);
        mb.connect_clicked(move |_| {
            st_m.borrow_mut().firewall_draft.persistence = mval;
            rr_m();
        });
        row2.append(&mb);
    }

    let cidr_entry = Entry::new();
    cidr_entry.add_css_class("akrep-entry");
    cidr_entry.set_hexpand(true);
    cidr_entry.set_placeholder_text(Some(lang.pick(
        "İsteğe bağlı kaynak CIDR (örn: 192.168.1.0/24)",
        "Optional LAN CIDR (e.g. 192.168.1.0/24)",
    )));
    cidr_entry.set_text(&draft.source_cidr);
    let st_cd = Rc::clone(state);
    cidr_entry.connect_changed(move |ent| {
        st_cd.borrow_mut().firewall_draft.source_cidr = ent.text().to_string();
    });
    row2.append(&cidr_entry);

    let add_btn = Button::with_label(lang.pick("Kural Planını Önizle", "Preview Rule Plan"));
    add_btn.add_css_class("akrep-primary-btn");
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
        let _ = cmd_add.send_blocking(UiCommand::CreatePlan { request: req });
    });
    row2.append(&add_btn);
    builder_card.append(&row2);
    root.append(&builder_card);

    // 3. Existing Rules List with Remove Actions
    let rules_card = GtkBox::new(Orientation::Vertical, 10);
    rules_card.add_css_class("akrep-detail-box");
    let r_title = Label::new(Some(lang.pick(
        "Mevcut Bölge Kuralları (Servisler, Portlar & Rich Rules)",
        "Existing Zone Rules (Services, Ports & Rich Rules)",
    )));
    r_title.add_css_class("akrep-section-hdr");
    r_title.set_xalign(0.0);
    rules_card.append(&r_title);

    if let Some(fw) = &fw_state {
        for z in &fw.zone_details {
            if !z.active
                && !z.is_default
                && z.runtime_ports.is_empty()
                && z.permanent_ports.is_empty()
                && z.runtime_rich_rules.is_empty()
            {
                continue;
            }
            let z_box = GtkBox::new(Orientation::Vertical, 6);
            z_box.add_css_class("akrep-diff-row");
            let z_hdr = Label::new(Some(&format!(
                "Zone: {}{}{}",
                z.name,
                if z.is_default { " [Default]" } else { "" },
                if z.active { " [Active]" } else { "" }
            )));
            z_hdr.add_css_class("akrep-card-title");
            z_hdr.set_xalign(0.0);
            z_box.append(&z_hdr);

            for (port, proto_str) in &z.runtime_ports {
                let is_perm = z
                    .permanent_ports
                    .iter()
                    .any(|(pp, pr)| pp == port && pr == proto_str);
                let proto = if proto_str.eq_ignore_ascii_case("udp") {
                    FirewallProto::Udp
                } else {
                    FirewallProto::Tcp
                };
                let prow = GtkBox::new(Orientation::Horizontal, 8);
                let plbl = Label::new(Some(&format!(
                    "Port: {}/{} ({})",
                    port,
                    proto.as_str(),
                    if is_perm {
                        "Runtime + Permanent"
                    } else {
                        "Runtime"
                    }
                )));
                plbl.add_css_class("akrep-mono");
                plbl.set_xalign(0.0);
                plbl.set_hexpand(true);
                prow.append(&plbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-danger-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let pval = port.clone();
                rm_btn.connect_clicked(move |_| {
                    let _ = cmd_rm.send_blocking(UiCommand::CreatePlan {
                        request: ActionRequest::FirewallRemovePort {
                            zone: zname.clone(),
                            port: pval.clone(),
                            proto,
                            permanent: is_perm,
                            source_cidr: None,
                        },
                    });
                });
                prow.append(&rm_btn);
                z_box.append(&prow);
            }

            for svc in &z.runtime_services {
                let is_perm = z.permanent_services.contains(svc);
                let srow = GtkBox::new(Orientation::Horizontal, 8);
                let slbl = Label::new(Some(&format!(
                    "Service: {} ({})",
                    svc,
                    if is_perm {
                        "Runtime + Permanent"
                    } else {
                        "Runtime"
                    }
                )));
                slbl.add_css_class("akrep-mono");
                slbl.set_xalign(0.0);
                slbl.set_hexpand(true);
                srow.append(&slbl);

                let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                rm_btn.add_css_class("akrep-ghost-btn");
                let cmd_rm = cmd_tx.clone();
                let zname = z.name.clone();
                let sval = svc.clone();
                rm_btn.connect_clicked(move |_| {
                    let _ = cmd_rm.send_blocking(UiCommand::CreatePlan {
                        request: ActionRequest::FirewallRemoveService {
                            zone: zname.clone(),
                            service: sval.clone(),
                            permanent: is_perm,
                            source_cidr: None,
                        },
                    });
                });
                srow.append(&rm_btn);
                z_box.append(&srow);
            }

            for rr in &z.runtime_rich_rules {
                let is_perm = z.permanent_rich_rules.contains(rr);
                let rrow = GtkBox::new(Orientation::Horizontal, 8);
                let rlbl = Label::new(Some(&format!("Rich Rule: {}", rr)));
                rlbl.add_css_class("akrep-mono");
                rlbl.set_xalign(0.0);
                rlbl.set_wrap(true);
                rlbl.set_hexpand(true);
                rrow.append(&rlbl);

                if let (Some(cidr), Some((port, proto))) = (
                    extract_source_cidr_from_rich_rule(rr),
                    extract_port_proto_from_rich_rule(rr),
                ) {
                    let rm_btn = Button::with_label(lang.pick("Kaldır", "Remove"));
                    rm_btn.add_css_class("akrep-danger-btn");
                    let cmd_rm = cmd_tx.clone();
                    let zname = z.name.clone();
                    rm_btn.connect_clicked(move |_| {
                        let _ = cmd_rm.send_blocking(UiCommand::CreatePlan {
                            request: ActionRequest::FirewallRemovePort {
                                zone: zname.clone(),
                                port: port.clone(),
                                proto,
                                permanent: is_perm,
                                source_cidr: Some(cidr.clone()),
                            },
                        });
                    });
                    rrow.append(&rm_btn);
                }
                z_box.append(&rrow);
            }

            rules_card.append(&z_box);
        }
    } else {
        let hint = Label::new(Some(lang.pick(
            "Tüm bölge portlarını, servislerini ve zengin kuralları listelemek veya kaldırmak için üstteki 'Tüm Kuralları Yükle (Polkit)' düğmesine tıklayın.",
            "Click 'Inspect Rules (Polkit)' above to load and manage per-zone ports, services, and rich rules.",
        )));
        hint.add_css_class("akrep-subtitle");
        hint.set_xalign(0.0);
        hint.set_wrap(true);
        rules_card.append(&hint);
    }

    root.append(&rules_card);
    root
}
