mod components;
mod dbus_client;
mod i18n;
mod pages;
mod state;
mod themes;

use crate::components::header_banner::{build_header_banner, HeaderBannerCallbacks};
use crate::components::nav_rail::{build_nav_rail, NAV_ITEMS};
use crate::components::plan_dialog::present_plan_preview_dialog;
use crate::dbus_client::{dispatch_cmd, spawn_dbus_worker, UiCommand, UiEvent};
use crate::i18n::Lang;
use crate::pages::applications::build_applications_page;
use crate::pages::device_security::build_device_security_page;
use crate::pages::firewall::build_firewall_page;
use crate::pages::history::{build_history_page, summarize_action_request};
use crate::pages::overview::build_overview_page;
use crate::pages::privacy::build_privacy_page;
use crate::pages::profiles::build_profiles_page;
use crate::pages::settings::build_settings_page;
use crate::pages::system_protection::build_system_protection_page;
use crate::pages::threat_protection::{build_threat_protection_page, format_scan_progress_view};
use crate::state::{AppUiState, NoticeBanner, NoticeKind};
use blaze_akrep_common::{
    ActionRequest, FlatpakScope, PrivacySessionController, ScanJobState, ScanProgress,
    SessionPortalPipewireBackend, UserFlatpakManager,
};
use blaze_sentinel_core::{AkrepSeverity, AKREP_SCHEMA_VERSION, AKREP_VERSION};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, HeaderBar, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
struct ShellWidgets {
    window: ApplicationWindow,
    rail_slot: GtkBox,
    banner_slot: GtkBox,
    scroller: ScrolledWindow,
}

pub fn should_update_scan_in_place(
    prev: Option<&ScanProgress>,
    next: &ScanProgress,
    has_live_widgets: bool,
) -> bool {
    if !has_live_widgets || next.state != ScanJobState::Running {
        return false;
    }
    let Some(p) = prev else {
        return false;
    };
    p.state == ScanJobState::Running
        && p.job_id == next.job_id
        && p.findings.len() == next.findings.len()
        && p.error_message == next.error_message
}

fn clear_box(container: &GtkBox) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

fn render_chrome(
    widgets: &ShellWidgets,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) {
    clear_box(&widgets.rail_slot);
    clear_box(&widgets.banner_slot);

    let st = state.borrow();
    let active_tab = st.active_tab;
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let (_, active_prof_tr, active_prof_en) = st.effective_active_profile();
    let offline = st.offline_reason.clone();
    let notice = st.notice.clone();
    drop(st);

    let w_nav = widgets.clone();
    let st_nav = Rc::clone(state);
    let cmd_nav = cmd_tx.clone();
    let on_select: Rc<dyn Fn(&'static str)> = Rc::new(move |tab: &'static str| {
        st_nav.borrow_mut().active_tab = tab;
        render_all(&w_nav, &st_nav, &cmd_nav);
    });

    let rail = build_nav_rail(active_tab, lang, Rc::clone(&on_select));
    widgets.rail_slot.append(&rail);

    let cmd_refresh = cmd_tx.clone();
    let w_lang = widgets.clone();
    let st_lang = Rc::clone(state);
    let cmd_lang = cmd_tx.clone();

    let w_prof = widgets.clone();
    let st_prof = Rc::clone(state);
    let cmd_prof = cmd_tx.clone();

    let w_dis = widgets.clone();
    let st_dis = Rc::clone(state);
    let cmd_dis = cmd_tx.clone();

    let banner = build_header_banner(
        snap.as_ref(),
        (&active_prof_tr, &active_prof_en),
        offline.as_deref(),
        notice.as_ref(),
        lang,
        HeaderBannerCallbacks {
            on_refresh: Box::new(move || {
                dispatch_cmd(&cmd_refresh, UiCommand::Refresh);
            }),
            on_toggle_lang: Box::new(move || {
                {
                    let mut s = st_lang.borrow_mut();
                    s.lang = s.lang.toggle();
                }
                render_all(&w_lang, &st_lang, &cmd_lang);
            }),
            on_open_profiles: Box::new(move || {
                st_prof.borrow_mut().active_tab = "profiles";
                render_all(&w_prof, &st_prof, &cmd_prof);
            }),
            on_dismiss_notice: Box::new(move || {
                st_dis.borrow_mut().notice = None;
                render_chrome(&w_dis, &st_dis, &cmd_dis);
            }),
        },
    );
    widgets.banner_slot.append(&banner);
}

fn render_page(
    widgets: &ShellWidgets,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) {
    let active_tab = {
        let mut st = state.borrow_mut();
        if st.active_tab != "threat_protection" {
            st.live_scan_widgets = None;
            st.live_scan_path_entry = None;
        }
        st.active_tab
    };

    let w_nav = widgets.clone();
    let st_nav = Rc::clone(state);
    let cmd_nav = cmd_tx.clone();
    let on_select: Rc<dyn Fn(&'static str)> = Rc::new(move |tab: &'static str| {
        st_nav.borrow_mut().active_tab = tab;
        render_all(&w_nav, &st_nav, &cmd_nav);
    });

    let w_rr = widgets.clone();
    let st_rr = Rc::clone(state);
    let cmd_rr = cmd_tx.clone();
    let on_rerender: Rc<dyn Fn()> = Rc::new(move || {
        render_all(&w_rr, &st_rr, &cmd_rr);
    });

    let page_widget = match active_tab {
        "overview" => build_overview_page(state, on_select),
        "threat_protection" => build_threat_protection_page(state, cmd_tx, &widgets.window),
        "system_protection" => build_system_protection_page(state, cmd_tx),
        "firewall" => build_firewall_page(state, cmd_tx, on_rerender),
        "applications" => build_applications_page(state, cmd_tx, on_rerender),
        "device_security" => build_device_security_page(state, cmd_tx),
        "privacy" => build_privacy_page(state, cmd_tx),
        "profiles" => build_profiles_page(state, cmd_tx, &widgets.window, on_rerender),
        "history" => build_history_page(state, cmd_tx),
        "settings" => build_settings_page(state, cmd_tx, on_rerender),
        _ => build_overview_page(state, on_select),
    };

    widgets.scroller.set_child(Some(&page_widget));
}

fn render_all(
    widgets: &ShellWidgets,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) {
    render_chrome(widgets, state, cmd_tx);
    render_page(widgets, state, cmd_tx);
}

fn print_help() {
    println!(
        "akrep-ui {} — BlazeOS Akrep Security Center (GTK4 Dual-Bus Client)",
        AKREP_VERSION
    );
    println!(
        "Usage: akrep-ui [--version | --smoke-test | --privacy-json | --user-flatpak-apply <APP_ID> <KEY> <TOKEN> | --user-flatpak-rollback <TX_ID> | --help]"
    );
}

fn run_privacy_json_cli() -> i32 {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        eprintln!("Failed to initialize Tokio runtime");
        return 1;
    };
    rt.block_on(async {
        let backend = match SessionPortalPipewireBackend::connect_session().await {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{}", e.to_redacted_json());
                return 1;
            }
        };
        let controller = PrivacySessionController::new(Arc::new(backend));
        match controller.snapshot().await {
            Ok(snap) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&snap).expect("privacy json")
                );
                0
            }
            Err(e) => {
                eprintln!("{}", e.to_redacted_json());
                1
            }
        }
    })
}

fn run_user_flatpak_apply_cli(app_id: &str, key: &str, token: &str) -> i32 {
    let mgr = match UserFlatpakManager::from_xdg_env() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e.to_redacted_json());
            return 1;
        }
    };
    let additions = vec![(key.to_string(), token.to_string())];
    match mgr.apply_user_override(app_id, &additions, &[]) {
        Ok(tx) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&tx).expect("user flatpak tx json")
            );
            0
        }
        Err(e) => {
            eprintln!("{}", e.to_redacted_json());
            1
        }
    }
}

fn run_user_flatpak_rollback_cli(tx_id: &str) -> i32 {
    let mgr = match UserFlatpakManager::from_xdg_env() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e.to_redacted_json());
            return 1;
        }
    };
    match mgr.rollback_user_override(tx_id) {
        Ok(tx) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&tx).expect("user flatpak rollback json")
            );
            0
        }
        Err(e) => {
            eprintln!("{}", e.to_redacted_json());
            1
        }
    }
}

fn run_headless_smoke_test() {
    let nav_ids: Vec<&str> = NAV_ITEMS.iter().map(|i| i.0).collect();
    let severities: Vec<(&str, &str)> = [
        AkrepSeverity::Protected,
        AkrepSeverity::Attention,
        AkrepSeverity::Critical,
        AkrepSeverity::Unavailable,
        AkrepSeverity::Unknown,
    ]
    .into_iter()
    .map(|s| (s.as_str(), components::severity_css_class(s)))
    .collect();

    let payload = serde_json::json!({
        "binary": "akrep-ui",
        "version": AKREP_VERSION,
        "schema_version": AKREP_SCHEMA_VERSION,
        "nav_count": nav_ids.len(),
        "nav_ids": nav_ids,
        "severity_classes": severities,
        "default_lang": Lang::detect().code(),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&payload).expect("smoke json")
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("akrep-ui {}", AKREP_VERSION);
        return;
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    if args.iter().any(|a| a == "--smoke-test") {
        run_headless_smoke_test();
        return;
    }
    if args.iter().any(|a| a == "--privacy-json") {
        std::process::exit(run_privacy_json_cli());
    }
    if let Some(pos) = args.iter().position(|a| a == "--user-flatpak-apply") {
        let Some(app_id) = args.get(pos + 1) else {
            eprintln!("Missing <APP_ID> <KEY> <TOKEN> for --user-flatpak-apply");
            std::process::exit(2);
        };
        let Some(key) = args.get(pos + 2) else {
            eprintln!("Missing <KEY> <TOKEN> for --user-flatpak-apply");
            std::process::exit(2);
        };
        let Some(tok) = args.get(pos + 3) else {
            eprintln!("Missing <TOKEN> for --user-flatpak-apply");
            std::process::exit(2);
        };
        std::process::exit(run_user_flatpak_apply_cli(app_id, key, tok));
    }
    if let Some(pos) = args.iter().position(|a| a == "--user-flatpak-rollback") {
        let Some(tx_id) = args.get(pos + 1) else {
            eprintln!("Missing <TX_ID> for --user-flatpak-rollback");
            std::process::exit(2);
        };
        std::process::exit(run_user_flatpak_rollback_cli(tx_id));
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let app = Application::builder()
        .application_id("tr.com.blazeos.Akrep")
        .build();

    app.connect_activate(|app| {
        themes::solarui::apply_solarui_theme();

        let (cmd_tx, evt_rx) = spawn_dbus_worker();
        let mut init_state = AppUiState::new(Lang::detect());
        if let Ok(req_tab) = std::env::var("AKREP_INITIAL_TAB") {
            if let Some(&(static_id, _, _, _)) =
                NAV_ITEMS.iter().find(|&&(id, _, _, _)| id == req_tab)
            {
                init_state.active_tab = static_id;
            }
        }
        let state = Rc::new(RefCell::new(init_state));

        let root_box = GtkBox::new(Orientation::Horizontal, 0);
        let rail_slot = GtkBox::new(Orientation::Vertical, 0);
        root_box.append(&rail_slot);

        let main_col = GtkBox::new(Orientation::Vertical, 14);
        main_col.set_hexpand(true);
        main_col.set_vexpand(true);
        main_col.set_margin_top(16);
        main_col.set_margin_bottom(16);
        main_col.set_margin_start(18);
        main_col.set_margin_end(18);

        let banner_slot = GtkBox::new(Orientation::Vertical, 0);
        main_col.append(&banner_slot);

        let scroller = ScrolledWindow::new();
        scroller.set_vexpand(true);
        scroller.set_hexpand(true);
        main_col.append(&scroller);

        root_box.append(&main_col);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Akrep Security Center")
            .default_width(1140)
            .default_height(760)
            .child(&root_box)
            .build();
        window.add_css_class("akrep-window");

        let hb = HeaderBar::new();
        hb.add_css_class("akrep-headerbar");
        window.set_titlebar(Some(&hb));

        let widgets = ShellWidgets {
            window: window.clone(),
            rail_slot,
            banner_slot,
            scroller,
        };
        render_all(&widgets, &state, &cmd_tx);

        let cmd_close = cmd_tx.clone();
        window.connect_close_request(move |_| {
            dispatch_cmd(&cmd_close, UiCommand::Stop);
            gtk4::glib::Propagation::Proceed
        });

        let st_ev = Rc::clone(&state);
        let w_ev = widgets.clone();
        let cmd_ev = cmd_tx.clone();
        gtk4::glib::MainContext::default().spawn_local(async move {
            while let Ok(evt) = evt_rx.recv().await {
                match evt {
                    UiEvent::Snapshot(snap) => {
                        let (had_snap, tab) = {
                            let mut st = st_ev.borrow_mut();
                            let had = st.snapshot.is_some();
                            st.snapshot = Some(*snap);
                            st.offline_reason = None;
                            (had, st.active_tab)
                        };
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                        if !had_snap || matches!(tab, "overview" | "settings" | "profiles") {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::DaemonOffline(msg) => {
                        let tab = {
                            let mut st = st_ev.borrow_mut();
                            st.offline_reason = Some(msg);
                            st.active_tab
                        };
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                        if matches!(tab, "overview" | "settings") {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::PlanStaged {
                        plan,
                        is_profile_preview,
                    } => {
                        let lang = {
                            let mut st = st_ev.borrow_mut();
                            if is_profile_preview {
                                st.last_profile_preview = Some((*plan).clone());
                            }
                            st.lang
                        };
                        if is_profile_preview && st_ev.borrow().active_tab == "profiles" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                        let w_cancel = w_ev.clone();
                        let st_cancel = Rc::clone(&st_ev);
                        let cmd_cancel = cmd_ev.clone();
                        let on_cancel: Rc<dyn Fn()> = Rc::new(move || {
                            if st_cancel.borrow().active_tab == "profiles" {
                                render_page(&w_cancel, &st_cancel, &cmd_cancel);
                            }
                        });
                        present_plan_preview_dialog(
                            &w_ev.window,
                            &st_ev,
                            &plan,
                            lang,
                            cmd_ev.clone(),
                            Some(on_cancel),
                        );
                    }
                    UiEvent::TransactionCommitted(rec) => {
                        {
                            let mut st = st_ev.borrow_mut();
                            let sum_tr = summarize_action_request(&rec.request, Lang::Tr);
                            let sum_en = summarize_action_request(&rec.request, Lang::En);
                            st.notice = Some(NoticeBanner {
                                kind: NoticeKind::Success,
                                message_tr: format!(
                                    "İşlem onaylandı ve doğrulandı ({}): {}",
                                    rec.tx_id, sum_tr
                                ),
                                message_en: format!(
                                    "Transaction committed & verified ({}): {}",
                                    rec.tx_id, sum_en
                                ),
                            });
                            if matches!(rec.request, ActionRequest::ApplyProfile { .. }) {
                                st.last_profile_preview = None;
                            }
                            if let ActionRequest::FlatpakSetOverride {
                                ref app_id,
                                scope: FlatpakScope::System,
                                ..
                            } = rec.request
                            {
                                let is_running = st
                                    .app_inventory
                                    .as_ref()
                                    .and_then(|inv| inv.find_flatpak(app_id, FlatpakScope::System))
                                    .is_some_and(|a| a.running);
                                if is_running {
                                    st.apps_draft.restart_needed_app = Some(app_id.clone());
                                }
                            }
                            st.history_records.retain(|r| r.tx_id != rec.tx_id);
                            st.history_records.insert(0, *rec);
                        }
                        render_all(&w_ev, &st_ev, &cmd_ev);
                    }
                    UiEvent::TransactionRolledBack(rec) => {
                        {
                            let mut st = st_ev.borrow_mut();
                            st.notice = Some(NoticeBanner {
                                kind: NoticeKind::Info,
                                message_tr: format!("İşlem başarıyla geri alındı ({}).", rec.tx_id),
                                message_en: format!(
                                    "Transaction rolled back cleanly ({}).",
                                    rec.tx_id
                                ),
                            });
                            if let Some(existing) =
                                st.history_records.iter_mut().find(|r| r.tx_id == rec.tx_id)
                            {
                                *existing = *rec;
                            } else {
                                st.history_records.insert(0, *rec);
                            }
                        }
                        render_all(&w_ev, &st_ev, &cmd_ev);
                    }
                    UiEvent::HistoryLoaded(list) => {
                        st_ev.borrow_mut().history_records = list;
                        if st_ev.borrow().active_tab == "history" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::SelinuxDiagnosed(diag) => {
                        st_ev.borrow_mut().selinux_diag = Some(*diag);
                        if st_ev.borrow().active_tab == "system_protection" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::FirewallInspected(fw) => {
                        {
                            let mut st = st_ev.borrow_mut();
                            if !fw.default_zone.is_empty() && st.firewall_draft.zone == "public" {
                                st.firewall_draft.zone = fw.default_zone.clone();
                            }
                            st.firewall_state = Some(*fw);
                        }
                        if st_ev.borrow().active_tab == "firewall" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::SystemFlatpaksInspected(snap) => {
                        let count = snap.apps.len();
                        {
                            let mut st = st_ev.borrow_mut();
                            st.privileged_system_apps = Some(snap.apps);
                            st.privileged_system_global_tokens = Some(snap.global_override_tokens);
                            st.merge_privileged_system_apps_into_inventory();
                            st.notice = Some(NoticeBanner {
                                kind: NoticeKind::Info,
                                message_tr: format!(
                                    "Sistem Flatpak denetimi yüklendi ({} uygulama).",
                                    count
                                ),
                                message_en: format!(
                                    "System Flatpak audit loaded ({} application(s)).",
                                    count
                                ),
                            });
                        }
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                        if st_ev.borrow().active_tab == "applications" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::DeviceSecurityInspected(status) => {
                        st_ev.borrow_mut().device_security = Some(*status);
                        if st_ev.borrow().active_tab == "device_security" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::ScannerInspected(rep) => {
                        st_ev.borrow_mut().scanner_report = Some(*rep);
                        if matches!(st_ev.borrow().active_tab, "threat_protection" | "history") {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::ScanProgressUpdated(prog) => {
                        let updated_in_place = {
                            let mut st = st_ev.borrow_mut();
                            let in_place = should_update_scan_in_place(
                                st.active_scan.as_ref(),
                                &prog,
                                st.live_scan_widgets.is_some(),
                            );
                            if in_place {
                                let lang = st.lang;
                                let (frac, bar_txt, cur_txt) =
                                    format_scan_progress_view(&prog, lang);
                                if let Some((ref pbar, ref cur_lbl)) = st.live_scan_widgets {
                                    pbar.set_fraction(frac);
                                    pbar.set_text(Some(&bar_txt));
                                    cur_lbl.set_text(&cur_txt);
                                    cur_lbl.set_visible(!cur_txt.is_empty());
                                }
                            }
                            st.active_scan = Some(*prog);
                            in_place
                        };
                        if !updated_in_place && st_ev.borrow().active_tab == "threat_protection" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::QuarantineLoaded(inv) => {
                        st_ev.borrow_mut().quarantine = Some(*inv);
                        if st_ev.borrow().active_tab == "threat_protection" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::ProfilesLoaded(cat) => {
                        st_ev.borrow_mut().profile_catalog = Some(*cat);
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                        if matches!(st_ev.borrow().active_tab, "profiles" | "overview") {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::PrivVersionLoaded(ver) => {
                        st_ev.borrow_mut().privd_version = Some(ver);
                        if st_ev.borrow().active_tab == "settings" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::UserApplicationsLoaded {
                        inventory,
                        user_txs,
                    } => {
                        {
                            let mut st = st_ev.borrow_mut();
                            st.app_inventory = Some(*inventory);
                            st.user_flatpak_txs = user_txs;
                            st.merge_privileged_system_apps_into_inventory();
                        }
                        if matches!(st_ev.borrow().active_tab, "applications" | "history") {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::UserFlatpakOverrideApplied(tx) => {
                        {
                            let mut st = st_ev.borrow_mut();
                            if tx.restart_required {
                                st.apps_draft.restart_needed_app = Some(tx.app_id.clone());
                            }
                            st.notice = Some(NoticeBanner {
                                kind: NoticeKind::Success,
                                message_tr: format!(
                                    "{} kullanıcı izinleri güncellendi (tx={}).",
                                    tx.app_id, tx.tx_id
                                ),
                                message_en: format!(
                                    "Applied user Flatpak override for {} (tx={}).",
                                    tx.app_id, tx.tx_id
                                ),
                            });
                        }
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                    }
                    UiEvent::UserFlatpakTxRolledBack(tx) => {
                        {
                            let mut st = st_ev.borrow_mut();
                            st.notice = Some(NoticeBanner {
                                kind: NoticeKind::Info,
                                message_tr: format!(
                                    "{} kullanıcı izni anahtar düzeyinde geri alındı.",
                                    tx.app_id
                                ),
                                message_en: format!(
                                    "Reverted key-level user override for {}.",
                                    tx.app_id
                                ),
                            });
                        }
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                    }
                    UiEvent::PrivacySessionLoaded(ps) => {
                        st_ev.borrow_mut().privacy_snapshot = Some(*ps);
                        if st_ev.borrow().active_tab == "privacy" {
                            render_page(&w_ev, &st_ev, &cmd_ev);
                        }
                    }
                    UiEvent::OperationInfo { tr, en } => {
                        st_ev.borrow_mut().notice = Some(NoticeBanner {
                            kind: NoticeKind::Info,
                            message_tr: tr,
                            message_en: en,
                        });
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                    }
                    UiEvent::OperationError { tr, en } => {
                        st_ev.borrow_mut().notice = Some(NoticeBanner {
                            kind: NoticeKind::Error,
                            message_tr: tr,
                            message_en: en,
                        });
                        render_chrome(&w_ev, &st_ev, &cmd_ev);
                    }
                }
            }
        });

        window.present();
    });

    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::nav_rail::map_card_id_to_tab;
    use crate::components::severity_css_class;
    use crate::dbus_client::format_dbus_or_akrep_error;
    use crate::pages::applications::compute_capability_override_delta;
    use crate::pages::firewall::{
        extract_port_proto_from_rich_rule, extract_service_from_rich_rule,
        extract_source_cidr_from_rich_rule, rich_rule_remove_request,
    };
    use crate::pages::history::can_rollback_request;
    use blaze_akrep_common::{
        ActionRequest, AkrepError, FirewallProto, ManagedProfileState, ProfileCatalogResponse,
        ScanFinding, ScanVerdict,
    };
    use std::collections::HashSet;

    #[test]
    fn test_ten_navigation_sections_and_card_routing() {
        assert_eq!(NAV_ITEMS.len(), 10);
        let ids: Vec<&str> = NAV_ITEMS.iter().map(|n| n.0).collect();
        assert_eq!(
            ids,
            vec![
                "overview",
                "threat_protection",
                "system_protection",
                "firewall",
                "applications",
                "device_security",
                "privacy",
                "profiles",
                "history",
                "settings",
            ]
        );
        assert_eq!(map_card_id_to_tab("selinux"), "system_protection");
        assert_eq!(map_card_id_to_tab("flatpak"), "applications");
        assert_eq!(map_card_id_to_tab("threat_integrity"), "threat_protection");
        assert_eq!(map_card_id_to_tab("packages"), "device_security");
        assert_eq!(map_card_id_to_tab("privacy"), "privacy");
        assert_eq!(map_card_id_to_tab("profiles"), "profiles");
    }

    #[test]
    fn test_five_severity_badges_have_distinct_css_classes() {
        let classes: HashSet<&str> = [
            AkrepSeverity::Protected,
            AkrepSeverity::Attention,
            AkrepSeverity::Critical,
            AkrepSeverity::Unavailable,
            AkrepSeverity::Unknown,
        ]
        .into_iter()
        .map(severity_css_class)
        .collect();
        assert_eq!(
            classes.len(),
            5,
            "All 5 AkrepSeverity variants must have visually distinct CSS classes"
        );
        for cls in &classes {
            assert!(
                themes::solarui::SOLARUI_CSS.contains(cls),
                "SOLARUI_CSS missing rule for .{}",
                cls
            );
        }
    }

    #[test]
    fn test_firewall_rich_rule_cidr_port_service_and_accept_guard() {
        let port_rule = "rule family=\"ipv4\" source address=\"192.168.1.0/24\" port port=\"25565\" protocol=\"tcp\" accept";
        assert_eq!(
            extract_source_cidr_from_rich_rule(port_rule).as_deref(),
            Some("192.168.1.0/24")
        );
        assert_eq!(
            extract_port_proto_from_rich_rule(port_rule),
            Some(("25565".to_string(), FirewallProto::Tcp))
        );
        assert_eq!(
            rich_rule_remove_request("public", port_rule, false),
            Some(ActionRequest::FirewallRemovePort {
                zone: "public".to_string(),
                port: "25565".to_string(),
                proto: FirewallProto::Tcp,
                permanent: false,
                source_cidr: Some("192.168.1.0/24".to_string()),
            })
        );

        let svc_rule =
            "rule family=\"ipv4\" source address=\"192.168.1.0/24\" service name=\"ssh\" accept";
        assert_eq!(
            extract_service_from_rich_rule(svc_rule).as_deref(),
            Some("ssh")
        );
        assert_eq!(
            rich_rule_remove_request("public", svc_rule, true),
            Some(ActionRequest::FirewallRemoveService {
                zone: "public".to_string(),
                service: "ssh".to_string(),
                permanent: true,
                source_cidr: Some("192.168.1.0/24".to_string()),
            })
        );

        // Reject/drop rich rules must not synthesize an accept-removal request
        let drop_rule =
            "rule family=\"ipv4\" source address=\"192.168.1.0/24\" port port=\"25565\" protocol=\"tcp\" drop";
        assert_eq!(rich_rule_remove_request("public", drop_rule, false), None);
    }

    #[test]
    fn test_in_place_scan_progress_and_camera_override_delta() {
        let p1 = ScanProgress {
            job_id: "job-1".to_string(),
            target_path: "/tmp".to_string(),
            scanned_files: 10,
            total_files: 100,
            percent: 10,
            current_path: Some("/tmp/a".to_string()),
            findings: vec![],
            state: ScanJobState::Running,
            started_at: "2026-11-25T19:00:00Z".to_string(),
            finished_at: None,
            error_message: None,
        };
        let mut p2 = p1.clone();
        p2.scanned_files = 45;
        p2.percent = 45;
        p2.current_path = Some("/tmp/b".to_string());
        assert!(should_update_scan_in_place(Some(&p1), &p2, true));

        let mut p3 = p2.clone();
        p3.findings.push(ScanFinding {
            path: "/tmp/eicar".to_string(),
            verdict: ScanVerdict::Infected {
                signature: "EICAR".to_string(),
            },
            threat_name: Some("EICAR".to_string()),
            size_bytes: 68,
        });
        assert!(!should_update_scan_in_place(Some(&p2), &p3, true));

        let mut p4 = p2.clone();
        p4.state = ScanJobState::Completed;
        assert!(!should_update_scan_in_place(Some(&p2), &p4, true));

        let (adds, rems) =
            compute_capability_override_delta("Context.devices", "all", "!all", true);
        assert_eq!(
            adds,
            vec![("Context.devices".to_string(), "!all".to_string())]
        );
        assert_eq!(
            rems,
            vec![("Context.devices".to_string(), "all".to_string())]
        );
    }

    #[test]
    fn test_effective_active_profile_prefers_live_catalog() {
        let mut st = AppUiState::new(Lang::En);
        assert_eq!(st.effective_active_profile().0, "balanced");

        st.profile_catalog = Some(ProfileCatalogResponse {
            active_profile_id: Some("gaming".to_string()),
            managed_state: ManagedProfileState::default(),
            profiles: vec![],
        });
        assert_eq!(
            st.effective_active_profile(),
            (
                "gaming".to_string(),
                "Oyun".to_string(),
                "Gaming".to_string()
            )
        );
    }

    #[test]
    fn test_dbus_error_formatting_redacts_secrets_and_parses_akrep_error() {
        let raw_payload =
            AkrepError::PolkitDenied("auth cancelled token=super_secret_tok_999".to_string())
                .to_redacted_json();
        let zerr = zbus::Error::FDO(Box::new(zbus::fdo::Error::AccessDenied(raw_payload)));
        let (tr, en) = format_dbus_or_akrep_error(&zerr);
        assert!(!tr.contains("super_secret_tok_999"));
        assert!(!en.contains("super_secret_tok_999"));
        assert!(en.contains("Administrator authentication denied"));
    }

    #[test]
    fn test_history_rollback_eligibility_and_state_preservation() {
        assert!(can_rollback_request(&ActionRequest::ApplyProfile {
            profile_id: "gaming".to_string()
        }));
        assert!(!can_rollback_request(&ActionRequest::DeleteQuarantine {
            quarantine_id: "q1".to_string(),
            confirm_permanent_delete: true
        }));
        assert!(!can_rollback_request(
            &ActionRequest::InstallOptionalScanner
        ));

        let mut st = AppUiState::new(Lang::Tr);
        st.firewall_draft.port_or_service = "25565".to_string();
        st.active_tab = "system_protection";
        st.active_tab = "firewall";
        assert_eq!(st.firewall_draft.port_or_service, "25565");
    }

    #[test]
    fn test_user_session_flatpak_channel_roundtrip_and_rollback() {
        use blaze_akrep_common::{MockFlatpakRuntimeProbe, TransactionState, UserFlatpakManager};
        use std::sync::Arc;

        let tmp = tempfile::tempdir().unwrap();
        let user_fp = tmp.path().join("user-flatpak");
        let sys_fp = tmp.path().join("sys-flatpak");
        let apps_dir = tmp.path().join("applications");
        let tx_dir = tmp.path().join("user-tx");

        let active_dir = user_fp.join("app/org.example.Notes/x86_64/stable/active");
        std::fs::create_dir_all(&active_dir).unwrap();
        std::fs::write(
            active_dir.join("metadata"),
            "[Application]\nname=org.example.Notes\nruntime=org.freedesktop.Platform/x86_64/25.08\n[Context]\nshared=network;\n",
        )
        .unwrap();

        let probe = Arc::new(MockFlatpakRuntimeProbe::new());
        probe.set_running("org.example.Notes", true);
        let mgr = UserFlatpakManager::with_paths(user_fp, sys_fp, vec![apps_dir], tx_dir, probe);

        let tx = mgr
            .apply_user_override(
                "org.example.Notes",
                &[("Context.shared".to_string(), "!network".to_string())],
                &[("Context.shared".to_string(), "network".to_string())],
            )
            .unwrap();
        assert!(tx.restart_required);
        assert_eq!(tx.state, TransactionState::Committed);

        let inv_after = mgr.inspect_unified().unwrap();
        let rec = inv_after
            .find_flatpak("org.example.Notes", FlatpakScope::User)
            .unwrap();
        assert!(!rec.effective_permissions.network.granted);

        let rb = mgr.rollback_user_override(&tx.tx_id).unwrap();
        assert_eq!(rb.state, TransactionState::RolledBack);
    }
}
