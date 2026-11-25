mod components;
mod dbus_client;
mod i18n;
mod pages;
mod themes;

use crate::components::header_banner::build_header_banner;
use crate::components::nav_rail::build_nav_rail;
use crate::dbus_client::{spawn_dbus_worker, UiCommand, UiEvent};
use crate::i18n::Lang;
use crate::pages::home::build_home_page;
use crate::pages::section::build_section_page;
use crate::pages::settings::build_settings_page;
use blaze_sentinel_core::{AkrepSnapshot, AKREP_VERSION};
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box as GtkBox, HeaderBar, Orientation, ScrolledWindow};
use std::cell::RefCell;
use std::rc::Rc;

struct AppUiState {
    snapshot: Option<AkrepSnapshot>,
    offline_reason: Option<String>,
    active_tab: &'static str,
    lang: Lang,
}

fn render_shell(
    container: &GtkBox,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }

    let st = state.borrow();
    let active_tab = st.active_tab;
    let lang = st.lang;
    let snap_clone = st.snapshot.clone();
    let offline_clone = st.offline_reason.clone();
    drop(st);

    let st_for_nav = Rc::clone(state);
    let cont_for_nav = container.clone();
    let cmd_for_nav = cmd_tx.clone();
    let on_select: Rc<dyn Fn(&'static str)> = Rc::new(move |tab: &'static str| {
        st_for_nav.borrow_mut().active_tab = tab;
        render_shell(&cont_for_nav, &st_for_nav, &cmd_for_nav);
    });

    let rail = build_nav_rail(active_tab, lang, Rc::clone(&on_select));
    container.append(&rail);

    let main_col = GtkBox::new(Orientation::Vertical, 14);
    main_col.set_hexpand(true);
    main_col.set_vexpand(true);
    main_col.set_margin_top(16);
    main_col.set_margin_bottom(16);
    main_col.set_margin_start(18);
    main_col.set_margin_end(18);

    let cmd_refresh = cmd_tx.clone();
    let st_lang = Rc::clone(state);
    let cont_lang = container.clone();
    let cmd_lang = cmd_tx.clone();
    let banner = build_header_banner(
        snap_clone.as_ref(),
        offline_clone.as_deref(),
        lang,
        move || {
            let _ = cmd_refresh.send_blocking(UiCommand::Refresh);
        },
        move || {
            {
                let mut s = st_lang.borrow_mut();
                s.lang = s.lang.toggle();
            }
            render_shell(&cont_lang, &st_lang, &cmd_lang);
        },
    );
    main_col.append(&banner);

    let page_widget = match active_tab {
        "home" => build_home_page(snap_clone.as_ref(), lang, on_select),
        "settings" => build_settings_page(snap_clone.as_ref(), lang),
        other => build_section_page(snap_clone.as_ref(), other, lang),
    };

    let scroller = ScrolledWindow::new();
    scroller.set_vexpand(true);
    scroller.set_hexpand(true);
    scroller.set_child(Some(&page_widget));
    main_col.append(&scroller);

    container.append(&main_col);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("akrep-ui {}", AKREP_VERSION);
        return;
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
        let state = Rc::new(RefCell::new(AppUiState {
            snapshot: None,
            offline_reason: None,
            active_tab: "home",
            lang: Lang::detect(),
        }));

        let root_box = GtkBox::new(Orientation::Horizontal, 0);
        render_shell(&root_box, &state, &cmd_tx);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Akrep Security Center")
            .default_width(1060)
            .default_height(720)
            .child(&root_box)
            .build();
        window.add_css_class("akrep-window");

        let hb = HeaderBar::new();
        hb.add_css_class("akrep-headerbar");
        window.set_titlebar(Some(&hb));

        let cmd_close = cmd_tx.clone();
        window.connect_close_request(move |_| {
            let _ = cmd_close.send_blocking(UiCommand::Stop);
            gtk4::glib::Propagation::Proceed
        });

        // Receive D-Bus telemetry strictly on GTK main thread via MainContext::spawn_local
        let st_ev = Rc::clone(&state);
        let box_ev = root_box.clone();
        let cmd_ev = cmd_tx.clone();
        gtk4::glib::MainContext::default().spawn_local(async move {
            while let Ok(evt) = evt_rx.recv().await {
                match evt {
                    UiEvent::Snapshot(snap) => {
                        let mut st = st_ev.borrow_mut();
                        st.snapshot = Some(*snap);
                        st.offline_reason = None;
                    }
                    UiEvent::DaemonOffline(msg) => {
                        let mut st = st_ev.borrow_mut();
                        st.offline_reason = Some(msg);
                    }
                }
                render_shell(&box_ev, &st_ev, &cmd_ev);
            }
        });

        window.present();
    });

    app.run();
}
