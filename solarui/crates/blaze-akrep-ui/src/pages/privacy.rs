use crate::components::{
    availability_css_class, build_availability_indicator, build_kv_row, build_status_indicator,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::state::AppUiState;
use blaze_akrep_common::PrivacyStreamKind;
use blaze_sentinel_core::Availability;
use gtk4::pango::WrapMode;
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

    let root = GtkBox::new(Orientation::Vertical, 20);

    let intro_col = GtkBox::new(Orientation::Vertical, 4);
    let hdr_row = GtkBox::new(Orientation::Horizontal, 12);
    hdr_row.set_valign(Align::Center);

    let page_desc = Label::new(Some(lang.pick(
        "Canlı mikrofon, kamera ve ekran paylaşımı akışları ile uygulama izinleri",
        "Live microphone, camera and screen share streams and app permissions",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    page_desc.set_hexpand(true);
    hdr_row.append(&page_desc);

    let refresh_btn = Button::with_label(lang.pick("Akışları yenile", "Refresh streams"));
    refresh_btn.add_css_class("akrep-primary-btn");
    refresh_btn.set_valign(Align::Center);
    let cmd_ref = cmd_tx.clone();
    refresh_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_ref, UiCommand::RefreshPrivacySession);
    });
    hdr_row.append(&refresh_btn);
    intro_col.append(&hdr_row);

    let consent_note = Label::new(Some(lang.pick(
        "Uygulamalara sessiz erişim verilmez; kayıtlı izinleri buradan geri alabilirsiniz",
        "Apps cannot be granted silent access; you can revoke saved grants here",
    )));
    consent_note.add_css_class("akrep-meta");
    consent_note.set_xalign(0.0);
    consent_note.set_wrap(true);
    intro_col.append(&consent_note);
    root.append(&intro_col);

    // 1. Live PipeWire Active Microphone, Camera & Screen Share Streams
    let streams_card = GtkBox::new(Orientation::Vertical, 12);
    streams_card.add_css_class("akrep-detail-box");
    let st_hdr = Label::new(Some(lang.pick(
        "Canlı erişim akışları",
        "Live capture streams",
    )));
    st_hdr.add_css_class("akrep-section-hdr");
    st_hdr.set_xalign(0.0);
    streams_card.append(&st_hdr);

    if let Some(ps) = &priv_snap {
        for (kind, label_tr, label_en, icon_name) in [
            (
                PrivacyStreamKind::Microphone,
                "Mikrofon",
                "Microphone",
                "audio-input-microphone-symbolic",
            ),
            (
                PrivacyStreamKind::Camera,
                "Kamera",
                "Camera",
                "camera-web-symbolic",
            ),
            (
                PrivacyStreamKind::ScreenCast,
                "Ekran paylaşımı",
                "Screen share",
                "video-display-symbolic",
            ),
        ] {
            let matching: Vec<_> = ps
                .active_streams
                .iter()
                .filter(|s| s.kind == kind)
                .collect();

            let k_box = GtkBox::new(Orientation::Vertical, 8);
            k_box.add_css_class("akrep-diff-row");

            let k_top = GtkBox::new(Orientation::Horizontal, 8);
            k_top.set_valign(Align::Center);
            let k_ico = Image::from_icon_name(icon_name);
            k_ico.set_pixel_size(16);
            k_ico.add_css_class("akrep-icon-secondary");
            k_top.append(&k_ico);

            let k_lbl = Label::new(Some(lang.pick(label_tr, label_en)));
            k_lbl.add_css_class("akrep-card-title");
            k_lbl.set_xalign(0.0);
            k_lbl.set_hexpand(true);
            k_top.append(&k_lbl);

            let (dot_cls, badge_txt, muted) = if ps.pipewire_availability != Availability::Available {
                (
                    availability_css_class(ps.pipewire_availability),
                    match lang {
                        crate::i18n::Lang::Tr => ps.pipewire_availability.label_tr().to_string(),
                        crate::i18n::Lang::En => ps.pipewire_availability.label_en().to_string(),
                    },
                    true,
                )
            } else if matching.is_empty() {
                (
                    "akrep-sev-protected",
                    lang.pick("Etkin akış yok", "Idle").to_string(),
                    false,
                )
            } else {
                (
                    "akrep-sev-attention",
                    format!("{} {}", matching.len(), lang.pick("etkin", "active")),
                    false,
                )
            };
            k_top.append(&build_status_indicator(dot_cls, &badge_txt, muted));
            k_box.append(&k_top);

            for stream in matching {
                let srow = GtkBox::new(Orientation::Horizontal, 12);
                srow.set_valign(Align::Center);
                let slbl = Label::new(Some(&format!(
                    "#{} · {} ({}) ← {} (PID {})",
                    stream.node_id,
                    stream.application_name,
                    stream
                        .application_id
                        .as_deref()
                        .unwrap_or(lang.pick("yerel", "native")),
                    stream.source_node_name,
                    stream
                        .process_id
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".to_string())
                )));
                slbl.add_css_class("akrep-mono");
                slbl.set_xalign(0.0);
                slbl.set_wrap(true);
                slbl.set_wrap_mode(WrapMode::WordChar);
                slbl.set_hexpand(true);
                srow.append(&slbl);

                let stop_btn = Button::with_label(lang.pick("Durdur", "Stop"));
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
            "PipeWire oturum bilgisi bekleniyor",
            "Waiting for PipeWire session state",
        )));
        info.add_css_class("akrep-subtitle");
        info.set_xalign(0.0);
        info.set_wrap(true);
        streams_card.append(&info);
    }
    root.append(&streams_card);

    // 2. Desktop Portal Capabilities
    let portal_card = GtkBox::new(Orientation::Vertical, 12);
    portal_card.add_css_class("akrep-detail-box");
    let p_hdr = Label::new(Some(lang.pick(
        "Masaüstü portal yetenekleri",
        "Desktop portal capabilities",
    )));
    p_hdr.add_css_class("akrep-section-hdr");
    p_hdr.set_xalign(0.0);
    portal_card.append(&p_hdr);

    if let Some(ps) = &priv_snap {
        let pt = &ps.portals;
        let rows: Vec<(&str, &str, &str, Availability, String)> = vec![
            (
                "Ekran paylaşımı",
                "Screen share",
                "org.freedesktop.portal.ScreenCast",
                pt.screencast.availability,
                format!(
                    "v{}",
                    pt.screencast
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
            (
                "Uzak masaüstü",
                "Remote desktop",
                "org.freedesktop.portal.RemoteDesktop",
                pt.remote_desktop.availability,
                format!(
                    "v{}",
                    pt.remote_desktop
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
            (
                "Genel kısayollar",
                "Global shortcuts",
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
                "Kamera",
                "Camera",
                "org.freedesktop.portal.Camera",
                pt.camera.availability,
                format!(
                    "v{}",
                    pt.camera
                        .version
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string())
                ),
            ),
            (
                "USB erişimi",
                "USB access",
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

        for (name_tr, name_en, iface_name, avail, ver_str) in rows {
            let rbox = GtkBox::new(Orientation::Horizontal, 12);
            rbox.add_css_class("akrep-diff-row");
            rbox.set_valign(Align::Center);

            let left = GtkBox::new(Orientation::Vertical, 2);
            left.set_hexpand(true);
            let nlbl = Label::new(Some(lang.pick(name_tr, name_en)));
            nlbl.add_css_class("akrep-card-title");
            nlbl.set_xalign(0.0);
            left.append(&nlbl);

            let ilbl = Label::new(Some(&format!("{} · {}", iface_name, ver_str)));
            ilbl.add_css_class("akrep-mono");
            ilbl.set_xalign(0.0);
            ilbl.set_wrap(true);
            ilbl.set_wrap_mode(WrapMode::WordChar);
            left.append(&ilbl);
            rbox.append(&left);

            rbox.append(&build_availability_indicator(avail, lang));
            portal_card.append(&rbox);
        }
    } else {
        portal_card.append(&build_kv_row(
            "org.freedesktop.portal.Desktop",
            lang.pick("Oturum servisi bekleniyor", "Waiting for session service"),
        ));
    }
    root.append(&portal_card);

    // 3. Saved Portal Permissions with Revoke Button
    let store_card = GtkBox::new(Orientation::Vertical, 12);
    store_card.add_css_class("akrep-detail-box");
    let s_title = Label::new(Some(lang.pick(
        "Kayıtlı uygulama izinleri",
        "Saved app permissions",
    )));
    s_title.add_css_class("akrep-section-hdr");
    s_title.set_xalign(0.0);
    store_card.append(&s_title);

    if let Some(ps) = &priv_snap {
        let mut total_grants = 0usize;
        for tbl in &ps.permission_tables {
            for entry in &tbl.entries {
                total_grants += 1;
                let erow = GtkBox::new(Orientation::Horizontal, 12);
                erow.add_css_class("akrep-diff-row");
                erow.set_valign(Align::Center);

                let elbl = Label::new(Some(&format!(
                    "{} · {} ({}) → {}",
                    entry.table,
                    entry.app_id,
                    entry.resource_id,
                    entry.permissions.join(", ")
                )));
                elbl.add_css_class("akrep-mono");
                elbl.set_xalign(0.0);
                elbl.set_wrap(true);
                elbl.set_wrap_mode(WrapMode::WordChar);
                elbl.set_hexpand(true);
                erow.append(&elbl);

                let rev_btn = Button::with_label(lang.pick("Geri al", "Revoke"));
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
                "Kalıcı olarak kaydedilmiş uygulama izni yok",
                "No saved application permissions",
            )));
            empty.add_css_class("akrep-subtitle");
            empty.set_xalign(0.0);
            store_card.append(&empty);
        }
    }
    root.append(&store_card);

    root
}
