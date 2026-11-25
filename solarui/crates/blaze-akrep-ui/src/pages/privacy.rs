use crate::components::{availability_css_class, build_kv_row};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::AppUiState;
use blaze_akrep_common::PrivacyStreamKind;
use blaze_sentinel_core::Availability;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Image, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_privacy_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let priv_snap = st.privacy_snapshot.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let title = Label::new(Some(lang.pick(
        "Gizlilik ve Masaüstü Koruması (PipeWire Canlı Akışlar & XDG Portal)",
        "Privacy & Desktop Protection (Live PipeWire Streams & XDG Portals)",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr_row.append(&title);

    let consent_pill = Label::new(Some(lang.pick(
        "Sessiz Portal İzni Yasaklı (Yalnızca Geri Alma)",
        "Silent Portal Grant Forbidden (Revoke-Only)",
    )));
    consent_pill.add_css_class("akrep-ro-pill");
    hdr_row.append(&consent_pill);

    let refresh_btn = Button::with_label(lang.pick("Akışları Yenile", "Refresh Streams"));
    refresh_btn.add_css_class("akrep-primary-btn");
    let cmd_ref = cmd_tx.clone();
    refresh_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_ref, UiCommand::RefreshPrivacySession);
    });
    hdr_row.append(&refresh_btn);
    root.append(&hdr_row);

    // 1. Live PipeWire Active Microphone, Camera & Screen Share Stream Cards
    let streams_card = GtkBox::new(Orientation::Vertical, 10);
    streams_card.add_css_class("akrep-detail-box");
    let st_hdr = Label::new(Some(lang.pick(
        "Canlı Mikrofon, Kamera ve Ekran Paylaşımı Akışları (PipeWire)",
        "Active Microphone, Camera & Screen Share Streams (PipeWire)",
    )));
    st_hdr.add_css_class("akrep-section-hdr");
    st_hdr.set_xalign(0.0);
    streams_card.append(&st_hdr);

    if let Some(ps) = &priv_snap {
        for (kind, label_tr, label_en, icon_name) in [
            (
                PrivacyStreamKind::Microphone,
                "Mikrofon Yakalama",
                "Microphone Capture",
                "audio-input-microphone-symbolic",
            ),
            (
                PrivacyStreamKind::Camera,
                "Kamera Akışı",
                "Camera Capture",
                "camera-web-symbolic",
            ),
            (
                PrivacyStreamKind::ScreenCast,
                "Ekran Paylaşımı (ScreenCast)",
                "Screen Share (ScreenCast)",
                "video-display-symbolic",
            ),
        ] {
            let matching: Vec<_> = ps
                .active_streams
                .iter()
                .filter(|s| s.kind == kind)
                .collect();

            let k_box = GtkBox::new(Orientation::Vertical, 6);
            k_box.add_css_class("akrep-diff-row");

            let k_top = GtkBox::new(Orientation::Horizontal, 8);
            let k_ico = Image::from_icon_name(icon_name);
            k_ico.set_pixel_size(20);
            k_top.append(&k_ico);

            let k_lbl = Label::new(Some(lang.pick(label_tr, label_en)));
            k_lbl.add_css_class("akrep-card-title");
            k_lbl.set_xalign(0.0);
            k_lbl.set_hexpand(true);
            k_top.append(&k_lbl);

            let badge_txt = if ps.pipewire_availability != Availability::Available {
                match lang {
                    crate::i18n::Lang::Tr => ps.pipewire_availability.label_tr().to_string(),
                    crate::i18n::Lang::En => ps.pipewire_availability.label_en().to_string(),
                }
            } else if matching.is_empty() {
                lang.pick("Etkin Akış Yok", "Idle (0 Active)").to_string()
            } else {
                format!("{} {}", matching.len(), lang.pick("Etkin Akış", "Active"))
            };
            let kb = Label::new(Some(&badge_txt));
            kb.add_css_class("akrep-badge");
            kb.add_css_class(if ps.pipewire_availability != Availability::Available {
                availability_css_class(ps.pipewire_availability)
            } else if matching.is_empty() {
                "akrep-sev-protected"
            } else {
                "akrep-sev-attention"
            });
            k_top.append(&kb);
            k_box.append(&k_top);

            for stream in matching {
                let srow = GtkBox::new(Orientation::Horizontal, 8);
                let slbl = Label::new(Some(&format!(
                    "Node #{} — {} ({}) <- {} (PID: {})",
                    stream.node_id,
                    stream.application_name,
                    stream.application_id.as_deref().unwrap_or("native"),
                    stream.source_node_name,
                    stream
                        .process_id
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".to_string())
                )));
                slbl.add_css_class("akrep-mono");
                slbl.set_xalign(0.0);
                slbl.set_wrap(true);
                slbl.set_hexpand(true);
                srow.append(&slbl);

                let stop_btn = Button::with_label(lang.pick("Akışı Durdur", "Stop Stream"));
                stop_btn.add_css_class("akrep-danger-btn");
                stop_btn.set_valign(Align::Center);
                let cmd_stop = cmd_tx.clone();
                let nid = stream.node_id;
                stop_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_stop,
                        UiCommand::TerminatePipewireStream { node_id: nid },
                    );
                });
                srow.append(&stop_btn);
                k_box.append(&srow);
            }
            streams_card.append(&k_box);
        }
    } else {
        let info = Label::new(Some(lang.pick(
            "Kullanıcı oturumu PipeWire grafiği yükleniyor veya bu ortamda aktif oturum veriyolu yok.",
            "Loading user-session PipeWire graph (or no desktop session bus present in headless mode).",
        )));
        info.add_css_class("akrep-subtitle");
        info.set_xalign(0.0);
        info.set_wrap(true);
        streams_card.append(&info);
    }
    root.append(&streams_card);

    // 2. XDG Desktop Portal Capability Matrix
    let portal_card = GtkBox::new(Orientation::Vertical, 10);
    portal_card.add_css_class("akrep-detail-box");
    let p_hdr = Label::new(Some(lang.pick(
        "XDG Desktop Portal Yetenek Matrisi (Wayland Etkileşimli Rıza)",
        "XDG Desktop Portal Capability Matrix (Wayland Interactive Consent)",
    )));
    p_hdr.add_css_class("akrep-section-hdr");
    p_hdr.set_xalign(0.0);
    portal_card.append(&p_hdr);

    if let Some(ps) = &priv_snap {
        let pt = &ps.portals;
        let rows: Vec<(&str, Availability, String)> = vec![
            (
                "org.freedesktop.portal.ScreenCast",
                pt.screencast.availability,
                format!(
                    "v{} • sources: {:?}",
                    pt.screencast
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    pt.screencast.source_types
                ),
            ),
            (
                "org.freedesktop.portal.RemoteDesktop",
                pt.remote_desktop.availability,
                format!(
                    "v{} • devices: {:?}",
                    pt.remote_desktop
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    pt.remote_desktop.device_types
                ),
            ),
            (
                "org.freedesktop.portal.GlobalShortcuts",
                pt.global_shortcuts.availability,
                format!(
                    "v{}",
                    pt.global_shortcuts
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
            (
                "org.freedesktop.portal.Camera",
                pt.camera.availability,
                format!(
                    "v{} • camera_present={:?}",
                    pt.camera
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    pt.camera.is_camera_present
                ),
            ),
            (
                "org.freedesktop.portal.Usb",
                pt.usb.availability,
                format!(
                    "v{}",
                    pt.usb
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
        ];

        for (iface_name, avail, detail_str) in rows {
            let rbox = GtkBox::new(Orientation::Horizontal, 10);
            rbox.add_css_class("akrep-diff-row");

            let ilbl = Label::new(Some(iface_name));
            ilbl.add_css_class("akrep-mono");
            ilbl.set_xalign(0.0);
            ilbl.set_size_request(310, -1);
            rbox.append(&ilbl);

            let dlbl = Label::new(Some(&detail_str));
            dlbl.add_css_class("akrep-subtitle");
            dlbl.set_xalign(0.0);
            dlbl.set_hexpand(true);
            rbox.append(&dlbl);

            let ab = Label::new(Some(match lang {
                crate::i18n::Lang::Tr => avail.label_tr(),
                crate::i18n::Lang::En => avail.label_en(),
            }));
            ab.add_css_class("akrep-badge");
            ab.add_css_class(availability_css_class(avail));
            rbox.append(&ab);

            portal_card.append(&rbox);
        }
    } else {
        portal_card.append(&build_kv_row(
            "org.freedesktop.portal.Desktop",
            lang.pick("Oturum veriyolu bekleniyor", "Awaiting session D-Bus"),
        ));
    }
    root.append(&portal_card);

    // 3. PermissionStore Saved App Grants with Revoke Button
    let store_card = GtkBox::new(Orientation::Vertical, 10);
    store_card.add_css_class("akrep-detail-box");
    let s_title = Label::new(Some(lang.pick(
        "Kayıtlı Portal Uygulama İzinleri (PermissionStore)",
        "Saved Portal App Grants (PermissionStore)",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    store_card.append(&s_title);

    if let Some(ps) = &priv_snap {
        let mut total_grants = 0usize;
        for tbl in &ps.permission_tables {
            for entry in &tbl.entries {
                total_grants += 1;
                let erow = GtkBox::new(Orientation::Horizontal, 10);
                erow.add_css_class("akrep-diff-row");

                let elbl = Label::new(Some(&format!(
                    "[{}] {} (resource='{}') -> {}",
                    entry.table,
                    entry.app_id,
                    entry.resource_id,
                    entry.permissions.join(", ")
                )));
                elbl.add_css_class("akrep-mono");
                elbl.set_xalign(0.0);
                elbl.set_wrap(true);
                elbl.set_hexpand(true);
                erow.append(&elbl);

                let rev_btn = Button::with_label(lang.pick("İzni Geri Al", "Revoke"));
                rev_btn.add_css_class("akrep-danger-btn");
                let cmd_rev = cmd_tx.clone();
                let t = entry.table.clone();
                let rid = entry.resource_id.clone();
                let aid = entry.app_id.clone();
                rev_btn.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rev,
                        UiCommand::RevokePortalPermission {
                            table: t.clone(),
                            resource_id: rid.clone(),
                            app_id: aid.clone(),
                        },
                    );
                });
                erow.append(&rev_btn);
                store_card.append(&erow);
            }
        }
        if total_grants == 0 {
            let empty = Label::new(Some(lang.pick(
                "PermissionStore tablolarında kalıcı olarak kaydedilmiş uygulama izni bulunmuyor.",
                "Zero persisted application grants currently stored in XDG PermissionStore.",
            )));
            empty.add_css_class("akrep-subtitle");
            empty.set_xalign(0.0);
            store_card.append(&empty);
        }
    }
    root.append(&store_card);

    root
}
