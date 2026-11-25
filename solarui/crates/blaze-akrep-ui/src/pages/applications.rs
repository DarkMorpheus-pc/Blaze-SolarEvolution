use crate::components::{tx_state_css_class, tx_state_label};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::state::{AppFilterTab, AppUiState};
use blaze_akrep_common::{
    ActionRequest, BusKind, FlatpakAppRecord, FlatpakScope, PermissionSourceLayer, TransactionState,
};
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

type CapabilityToggleSpec<'a> = (
    &'a str,
    bool,
    Option<PermissionSourceLayer>,
    (&'a str, &'a str, &'a str),
);

pub type OverridePairList = Vec<(String, String)>;

pub fn compute_capability_override_delta(
    spec_key: &str,
    pos_tok: &str,
    neg_tok: &str,
    currently_granted: bool,
) -> (OverridePairList, OverridePairList) {
    if currently_granted {
        (
            vec![(spec_key.to_string(), neg_tok.to_string())],
            vec![(spec_key.to_string(), pos_tok.to_string())],
        )
    } else {
        (
            vec![(spec_key.to_string(), pos_tok.to_string())],
            vec![(spec_key.to_string(), neg_tok.to_string())],
        )
    }
}

fn source_layer_label(layer: Option<PermissionSourceLayer>, lang: Lang) -> &'static str {
    match layer {
        Some(PermissionSourceLayer::Manifest) => lang.pick("Manifest", "Manifest"),
        Some(PermissionSourceLayer::GlobalOverride) => {
            lang.pick("Genel Override", "Global Override")
        }
        Some(PermissionSourceLayer::AppOverride) => lang.pick("Uygulama Override", "App Override"),
        None => lang.pick("Varsayılan", "Default"),
    }
}

fn dispatch_override(
    app: &FlatpakAppRecord,
    additions: Vec<(String, String)>,
    removals: Vec<(String, String)>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) {
    match app.scope {
        FlatpakScope::User => {
            dispatch_cmd(
                cmd_tx,
                UiCommand::ApplyUserFlatpakOverride {
                    app_id: app.app_id.clone(),
                    additions,
                    removals,
                },
            );
        }
        FlatpakScope::System => {
            dispatch_cmd(
                cmd_tx,
                UiCommand::CreatePlan {
                    request: ActionRequest::FlatpakSetOverride {
                        app_id: app.app_id.clone(),
                        scope: FlatpakScope::System,
                        additions,
                        removals,
                    },
                },
            );
        }
    }
}

pub fn build_applications_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    on_rerender: Rc<dyn Fn()>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let inv_opt = st.app_inventory.clone();
    let user_txs = st.user_flatpak_txs.clone();
    let draft = st.apps_draft.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let hdr_row = GtkBox::new(Orientation::Horizontal, 10);
    let title = Label::new(Some(lang.pick(
        "Uygulama Güvenliği (Flatpak Kum Havuzu & Yerel Paket Kökeni)",
        "Application Security (Flatpak Sandbox Overrides & Native Provenance)",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    title.set_hexpand(true);
    hdr_row.append(&title);

    let sys_insp_btn = Button::with_label(lang.pick(
        "Sistem Flatpak Denetimi (Polkit)",
        "Audit System Flatpaks (Polkit)",
    ));
    sys_insp_btn.add_css_class("akrep-ghost-btn");
    let cmd_sys = cmd_tx.clone();
    sys_insp_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_sys, UiCommand::InspectSystemFlatpaks);
    });
    hdr_row.append(&sys_insp_btn);

    let refresh_btn = Button::with_label(lang.pick("Envanteri Yenile", "Refresh Inventory"));
    refresh_btn.add_css_class("akrep-ghost-btn");
    let cmd_ref = cmd_tx.clone();
    refresh_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_ref, UiCommand::RefreshUserApplications);
    });
    hdr_row.append(&refresh_btn);
    root.append(&hdr_row);

    if let Some(restart_app) = &draft.restart_needed_app {
        let notice = GtkBox::new(Orientation::Horizontal, 8);
        notice.add_css_class("akrep-offline-banner");
        let nlbl = Label::new(Some(&format!(
            "{}: {} — {}",
            lang.pick("Yeniden Başlatma Gerekli", "Restart Required"),
            restart_app,
            lang.pick(
                "Uygulama şu anda çalışıyor; kum havuzu izin değişiklikleri uygulama yeniden başlatıldığında devreye girer.",
                "Application is currently running; sandbox permission changes take effect after restarting the app."
            )
        )));
        nlbl.set_xalign(0.0);
        nlbl.set_wrap(true);
        nlbl.set_hexpand(true);
        notice.append(&nlbl);
        root.append(&notice);
    }

    // Filter & Search bar
    let filter_bar = GtkBox::new(Orientation::Horizontal, 8);
    for (lbl, tab) in [
        (lang.pick("Tümü", "All"), AppFilterTab::All),
        (
            lang.pick("Kullanıcı Flatpak", "User Flatpak"),
            AppFilterTab::UserFlatpak,
        ),
        (
            lang.pick("Sistem Flatpak", "System Flatpak"),
            AppFilterTab::SystemFlatpak,
        ),
        (
            lang.pick("Yerel Paket (Native)", "Native Package"),
            AppFilterTab::Native,
        ),
    ] {
        let b = Button::with_label(lbl);
        b.add_css_class(if draft.filter == tab {
            "akrep-primary-btn"
        } else {
            "akrep-ghost-btn"
        });
        let st_f = Rc::clone(state);
        let rr_f = Rc::clone(&on_rerender);
        b.connect_clicked(move |_| {
            st_f.borrow_mut().apps_draft.filter = tab;
            rr_f();
        });
        filter_bar.append(&b);
    }

    let search_ent = Entry::new();
    search_ent.add_css_class("akrep-entry");
    search_ent.set_hexpand(true);
    search_ent.set_placeholder_text(Some(lang.pick(
        "Uygulama adı veya kimliği ara...",
        "Filter by app name or ID...",
    )));
    search_ent.set_text(&draft.search_query);
    let st_sq = Rc::clone(state);
    search_ent.connect_changed(move |ent| {
        st_sq.borrow_mut().apps_draft.search_query = ent.text().to_string();
    });
    let rr_sq = Rc::clone(&on_rerender);
    search_ent.connect_activate(move |_| rr_sq());
    filter_bar.append(&search_ent);
    root.append(&filter_bar);

    let Some(inv) = inv_opt else {
        let loading = GtkBox::new(Orientation::Vertical, 6);
        loading.add_css_class("akrep-detail-box");
        let l = Label::new(Some(lang.pick(
            "Uygulama envanteri okunuyor...",
            "Loading application inventory...",
        )));
        l.set_xalign(0.0);
        loading.append(&l);
        root.append(&loading);
        return root;
    };

    let q = draft.search_query.trim().to_ascii_lowercase();
    let mut flatpak_candidates: Vec<&FlatpakAppRecord> = Vec::new();
    if matches!(draft.filter, AppFilterTab::All | AppFilterTab::UserFlatpak) {
        flatpak_candidates.extend(inv.user_apps.iter().filter(|a| {
            q.is_empty()
                || a.app_id.to_ascii_lowercase().contains(&q)
                || a.display_name.to_lowercase().contains(&q)
        }));
    }
    if matches!(
        draft.filter,
        AppFilterTab::All | AppFilterTab::SystemFlatpak
    ) {
        flatpak_candidates.extend(inv.system_apps.iter().filter(|a| {
            q.is_empty()
                || a.app_id.to_ascii_lowercase().contains(&q)
                || a.display_name.to_lowercase().contains(&q)
        }));
    }

    let list_card = GtkBox::new(Orientation::Vertical, 8);
    list_card.add_css_class("akrep-detail-box");
    let list_hdr = Label::new(Some(&format!(
        "{} ({} Flatpak, {} Native)",
        lang.pick("Kurulu Uygulamalar", "Installed Applications"),
        inv.user_apps.len() + inv.system_apps.len(),
        inv.native_apps.len()
    )));
    list_hdr.add_css_class("akrep-section-hdr");
    list_hdr.set_xalign(0.0);
    list_card.append(&list_hdr);

    if flatpak_candidates.is_empty() && draft.filter != AppFilterTab::Native {
        let none_fp = Label::new(Some(lang.pick(
            "Bu kapsamda kurulu Flatpak uygulaması bulunmuyor.",
            "No installed Flatpak applications found in this scope.",
        )));
        none_fp.add_css_class("akrep-subtitle");
        none_fp.set_xalign(0.0);
        list_card.append(&none_fp);
    }

    for app in flatpak_candidates.iter().take(25) {
        let arow = GtkBox::new(Orientation::Horizontal, 8);
        arow.add_css_class("akrep-diff-row");

        let scope_badge = Label::new(Some(match app.scope {
            FlatpakScope::User => "USER",
            FlatpakScope::System => "SYSTEM",
        }));
        scope_badge.add_css_class("akrep-badge");
        scope_badge.add_css_class(match app.scope {
            FlatpakScope::User => "akrep-risk-low",
            FlatpakScope::System => "akrep-risk-medium",
        });
        arow.append(&scope_badge);

        let name_lbl = Label::new(Some(&format!("{} ({})", app.display_name, app.app_id)));
        name_lbl.add_css_class("akrep-card-title");
        name_lbl.set_xalign(0.0);
        name_lbl.set_hexpand(true);
        arow.append(&name_lbl);

        if app.running {
            let run_b = Label::new(Some(lang.pick("Çalışıyor", "Running")));
            run_b.add_css_class("akrep-badge");
            run_b.add_css_class("akrep-sev-attention");
            arow.append(&run_b);
        }

        let sel_btn = Button::with_label(lang.pick("İzinleri Yönet", "Manage Permissions"));
        sel_btn.add_css_class("akrep-primary-btn");
        let st_sel = Rc::clone(state);
        let rr_sel = Rc::clone(&on_rerender);
        let aid = app.app_id.clone();
        let sc = app.scope;
        sel_btn.connect_clicked(move |_| {
            st_sel.borrow_mut().apps_draft.selected_app_key = Some((aid.clone(), Some(sc)));
            rr_sel();
        });
        arow.append(&sel_btn);
        list_card.append(&arow);
    }

    if matches!(draft.filter, AppFilterTab::All | AppFilterTab::Native) {
        for nat in inv
            .native_apps
            .iter()
            .filter(|n| {
                q.is_empty()
                    || n.desktop_id.to_ascii_lowercase().contains(&q)
                    || n.display_name.to_lowercase().contains(&q)
            })
            .take(15)
        {
            let nrow = GtkBox::new(Orientation::Horizontal, 8);
            nrow.add_css_class("akrep-diff-row");

            let nb = Label::new(Some("NATIVE RPM"));
            nb.add_css_class("akrep-badge");
            nb.add_css_class("akrep-sev-unavailable");
            nrow.append(&nb);

            let nl = Label::new(Some(&format!(
                "{} ({}) -> {}",
                nat.display_name, nat.desktop_id, nat.exec_bin
            )));
            nl.set_xalign(0.0);
            nl.set_wrap(true);
            nl.set_hexpand(true);
            nrow.append(&nl);

            let note = Label::new(Some(lang.pick(
                "SELinux + Wayland Portal ile korunur (Flatpak kum havuzu değil)",
                "Governed by SELinux + Wayland Portals (sandbox_supported=false)",
            )));
            note.add_css_class("akrep-subtitle");
            nrow.append(&note);
            list_card.append(&nrow);
        }
    }
    root.append(&list_card);

    // Selected Flatpak App Detailed Permission Editor
    let selected_flatpak = draft
        .selected_app_key
        .as_ref()
        .and_then(|(id, sc_opt)| sc_opt.and_then(|sc| inv.find_flatpak(id, sc)))
        .or_else(|| flatpak_candidates.first().copied());

    if let Some(app) = selected_flatpak {
        let perm_card = GtkBox::new(Orientation::Vertical, 10);
        perm_card.add_css_class("akrep-detail-box");

        let phdr = GtkBox::new(Orientation::Horizontal, 8);
        let ptitle = Label::new(Some(&format!(
            "{}: {} ({}) — {}",
            lang.pick("Seçili Uygulama İzinleri", "Selected App Sandbox"),
            app.display_name,
            app.app_id,
            match app.scope {
                FlatpakScope::User => lang.pick(
                    "Kullanıcı Kapsamı (Anında Atomik INI + Anahtar Düzeyinde Geri Alma)",
                    "User Scope (Instant Atomic INI + Key-Level Rollback)",
                ),
                FlatpakScope::System => lang.pick(
                    "Sistem Kapsamı (Plan Önizleme + Polkit Onayı)",
                    "System Scope (Plan Preview + Polkit Authorization)",
                ),
            }
        )));
        ptitle.add_css_class("akrep-section-hdr");
        ptitle.set_xalign(0.0);
        ptitle.set_wrap(true);
        ptitle.set_hexpand(true);
        phdr.append(&ptitle);
        perm_card.append(&phdr);

        if app.running {
            let rwarn = Label::new(Some(lang.pick(
                "Bu uygulama şu anda açık — yapacağınız izin değişiklikleri uygulamayı kapatıp yeniden açtığınızda geçerli olur.",
                "This app is currently running — override changes will apply the next time the app restarts.",
            )));
            rwarn.add_css_class("akrep-offline-banner");
            rwarn.set_xalign(0.0);
            rwarn.set_wrap(true);
            perm_card.append(&rwarn);
        }

        let ep = &app.effective_permissions;
        let toggles: Vec<CapabilityToggleSpec<'_>> = vec![
            (
                lang.pick("Ağ Erişimi (Network)", "Network Access"),
                ep.network.granted,
                ep.network.source,
                ("Context.shared", "network", "!network"),
            ),
            (
                lang.pick(
                    "Mikrofon (PulseAudio/PipeWire)",
                    "Microphone (Audio Socket)",
                ),
                ep.microphone.granted,
                ep.microphone.source,
                ("Context.sockets", "pulseaudio", "!pulseaudio"),
            ),
            (
                lang.pick(
                    "Doğrudan Kamera/Cihazlar (!all ile Kısıtla)",
                    "Direct Camera/Devices",
                ),
                ep.camera.direct_devices_all,
                ep.camera.source,
                ("Context.devices", "all", "!all"),
            ),
            (
                lang.pick("USB Cihazları", "USB Devices"),
                ep.usb.granted,
                ep.usb.source,
                ("Context.devices", "usb", "!usb"),
            ),
            (
                lang.pick("Bluetooth", "Bluetooth"),
                ep.bluetooth.granted,
                ep.bluetooth.source,
                ("Context.features", "bluetooth", "!bluetooth"),
            ),
            (
                lang.pick("GPU Donanım Hızlandırma (DRI)", "GPU Acceleration (DRI)"),
                ep.gpu.granted,
                ep.gpu.source,
                ("Context.devices", "dri", "!dri"),
            ),
        ];

        for (cap_lbl, granted, src_layer, (spec_key, pos_tok, neg_tok)) in toggles {
            let crow = GtkBox::new(Orientation::Horizontal, 10);
            crow.add_css_class("akrep-diff-row");

            let clbl = Label::new(Some(cap_lbl));
            clbl.add_css_class("akrep-card-title");
            clbl.set_xalign(0.0);
            clbl.set_hexpand(true);
            crow.append(&clbl);

            let src_b = Label::new(Some(source_layer_label(src_layer, lang)));
            src_b.add_css_class("akrep-subtitle");
            crow.append(&src_b);

            let st_b = Label::new(Some(if granted {
                lang.pick("İzinli", "Granted")
            } else {
                lang.pick("Kısıtlı / Portal", "Restricted / Portal")
            }));
            st_b.add_css_class("akrep-badge");
            st_b.add_css_class(if granted {
                "akrep-sev-attention"
            } else {
                "akrep-sev-protected"
            });
            crow.append(&st_b);

            // Blanket `devices=all` is a forbidden red-line grant, so only tightening (`!all` + remove `all`) is offered!
            if !(spec_key == "Context.devices" && pos_tok == "all" && !granted) {
                let btn_txt = if granted {
                    lang.pick("Kısıtla", "Restrict")
                } else {
                    lang.pick("İzin Ver", "Grant")
                };
                let act_btn = Button::with_label(btn_txt);
                act_btn.add_css_class("akrep-ghost-btn");
                let app_clone = app.clone();
                let cmd_c = cmd_tx.clone();
                let sk = spec_key.to_string();
                let pt = pos_tok.to_string();
                let nt = neg_tok.to_string();
                act_btn.connect_clicked(move |_| {
                    let (adds, rems) = compute_capability_override_delta(&sk, &pt, &nt, granted);
                    dispatch_override(&app_clone, adds, rems, &cmd_c);
                });
                crow.append(&act_btn);
            }
            perm_card.append(&crow);
        }

        // Narrow Filesystem Grants
        let fs_hdr = Label::new(Some(lang.pick(
            "Dosya Sistemi Erişimleri (Geniş host/home izinleri kırmızı çizgiyle engellenir)",
            "Filesystem Grants (Blanket host/home grants blocked by red-line policy)",
        )));
        fs_hdr.add_css_class("akrep-card-title");
        fs_hdr.set_xalign(0.0);
        perm_card.append(&fs_hdr);

        for fgrant in &ep.filesystems {
            let frow = GtkBox::new(Orientation::Horizontal, 8);
            frow.add_css_class("akrep-diff-row");
            let fl = Label::new(Some(&format!(
                "{} ({:?}, {})",
                fgrant.raw_token,
                fgrant.mode,
                source_layer_label(Some(fgrant.source), lang)
            )));
            fl.add_css_class("akrep-mono");
            fl.set_xalign(0.0);
            fl.set_hexpand(true);
            frow.append(&fl);

            let revoke_fs_btn =
                Button::with_label(lang.pick("Kaldır / Kısıtla", "Remove / Restrict"));
            revoke_fs_btn.add_css_class("akrep-danger-btn");
            let app_clone = app.clone();
            let cmd_f = cmd_tx.clone();
            let raw_tok = fgrant.raw_token.clone();
            let base_spec = fgrant.path_spec.clone();
            revoke_fs_btn.connect_clicked(move |_| {
                let mut adds = Vec::new();
                let rems = vec![("Context.filesystems".to_string(), raw_tok.clone())];
                if matches!(base_spec.as_str(), "host" | "home") {
                    adds.push(("Context.filesystems".to_string(), format!("!{}", base_spec)));
                }
                dispatch_override(&app_clone, adds, rems, &cmd_f);
            });
            frow.append(&revoke_fs_btn);
            perm_card.append(&frow);
        }

        let add_fs_row = GtkBox::new(Orientation::Horizontal, 8);
        let fs_ent = Entry::new();
        fs_ent.add_css_class("akrep-entry");
        fs_ent.set_hexpand(true);
        fs_ent.set_placeholder_text(Some("~/Projects:ro or xdg-download:ro"));
        fs_ent.set_text(&draft.narrow_fs_input);
        let st_fs = Rc::clone(state);
        fs_ent.connect_changed(move |ent| {
            st_fs.borrow_mut().apps_draft.narrow_fs_input = ent.text().to_string();
        });
        add_fs_row.append(&fs_ent);

        let add_fs_btn = Button::with_label(lang.pick("Dar Klasör İzni Ekle", "Add Narrow Folder"));
        add_fs_btn.add_css_class("akrep-primary-btn");
        let app_fs = app.clone();
        let cmd_afs = cmd_tx.clone();
        let st_afs = Rc::clone(state);
        add_fs_btn.connect_clicked(move |_| {
            let tok = st_afs
                .borrow()
                .apps_draft
                .narrow_fs_input
                .trim()
                .to_string();
            dispatch_override(
                &app_fs,
                vec![("Context.filesystems".to_string(), tok)],
                vec![],
                &cmd_afs,
            );
        });
        add_fs_row.append(&add_fs_btn);
        perm_card.append(&add_fs_row);

        // D-Bus policies viewer & restriction controls
        let bus_hdr = Label::new(Some(lang.pick(
            "D-Bus Oturum ve Sistem Politikaları",
            "D-Bus Session & System Policies",
        )));
        bus_hdr.add_css_class("akrep-card-title");
        bus_hdr.set_xalign(0.0);
        perm_card.append(&bus_hdr);

        if ep.bus_policies.is_empty() {
            let empty_bus = Label::new(Some(lang.pick(
                "Özel D-Bus oturum/sistem veriyolu izni yok (varsayılan portal izolasyonu aktif).",
                "No custom D-Bus session/system bus grants (default portal isolation active).",
            )));
            empty_bus.add_css_class("akrep-subtitle");
            empty_bus.set_xalign(0.0);
            perm_card.append(&empty_bus);
        } else {
            for bp in &ep.bus_policies {
                let brow = GtkBox::new(Orientation::Horizontal, 8);
                brow.add_css_class("akrep-diff-row");
                let sec_name = match bp.bus_kind {
                    BusKind::Session => "Session Bus Policy",
                    BusKind::System => "System Bus Policy",
                };
                let blbl = Label::new(Some(&format!(
                    "[{}].{} = {} ({})",
                    sec_name,
                    bp.bus_name,
                    bp.policy,
                    source_layer_label(Some(bp.source), lang)
                )));
                blbl.add_css_class("akrep-mono");
                blbl.set_xalign(0.0);
                blbl.set_wrap(true);
                blbl.set_hexpand(true);
                brow.append(&blbl);

                if bp.is_sensitive_warning {
                    let wb = Label::new(Some(lang.pick("Hassas Veriyolu", "Sensitive Bus")));
                    wb.add_css_class("akrep-badge");
                    wb.add_css_class("akrep-sev-critical");
                    brow.append(&wb);
                }

                if matches!(bp.policy.to_ascii_lowercase().as_str(), "talk" | "own") {
                    let restrict_bus_btn =
                        Button::with_label(lang.pick("Kısıtla (none)", "Restrict (none)"));
                    restrict_bus_btn.add_css_class("akrep-danger-btn");
                    let app_clone = app.clone();
                    let cmd_b = cmd_tx.clone();
                    let spec = format!("[{}].{}", sec_name, bp.bus_name);
                    let old_pol = bp.policy.clone();
                    restrict_bus_btn.connect_clicked(move |_| {
                        dispatch_override(
                            &app_clone,
                            vec![(spec.clone(), "none".to_string())],
                            vec![(spec.clone(), old_pol.clone())],
                            &cmd_b,
                        );
                    });
                    brow.append(&restrict_bus_btn);
                }

                perm_card.append(&brow);
            }
        }

        root.append(&perm_card);
    }

    // Recent User-Scope Override Transactions (Key-Level Rollback)
    if !user_txs.is_empty() {
        let utx_card = GtkBox::new(Orientation::Vertical, 8);
        utx_card.add_css_class("akrep-detail-box");
        let utx_hdr = Label::new(Some(lang.pick(
            "Son Kullanıcı Flatpak İzin İşlemleri (Anahtar Düzeyinde Geri Alma)",
            "Recent User-Scope Flatpak Transactions (Key-Level Rollback)",
        )));
        utx_hdr.add_css_class("akrep-section-hdr");
        utx_hdr.set_xalign(0.0);
        utx_card.append(&utx_hdr);

        for utx in user_txs.iter().take(10) {
            let trow = GtkBox::new(Orientation::Horizontal, 8);
            trow.add_css_class("akrep-diff-row");

            let sb = Label::new(Some(tx_state_label(utx.state, lang)));
            sb.add_css_class("akrep-badge");
            sb.add_css_class(tx_state_css_class(utx.state));
            trow.append(&sb);

            let tl = Label::new(Some(&format!(
                "{} • {}",
                utx.app_id,
                utx.created_at.chars().take(19).collect::<String>()
            )));
            tl.add_css_class("akrep-mono");
            tl.set_xalign(0.0);
            tl.set_hexpand(true);
            trow.append(&tl);

            if utx.state == TransactionState::Committed {
                let rb = Button::with_label(lang.pick("Geri Al", "Rollback"));
                rb.add_css_class("akrep-ghost-btn");
                let cmd_rb = cmd_tx.clone();
                let tid = utx.tx_id.clone();
                rb.connect_clicked(move |_| {
                    dispatch_cmd(
                        &cmd_rb,
                        UiCommand::RollbackUserFlatpakTx { tx_id: tid.clone() },
                    );
                });
                trow.append(&rb);
            }
            utx_card.append(&trow);
        }
        root.append(&utx_card);
    }

    root
}
