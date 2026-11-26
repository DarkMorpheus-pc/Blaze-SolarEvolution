pub mod actions;
pub mod apps;
pub mod bar;
pub mod blazeai;
pub mod companion_avatar;
pub mod engine;
pub mod gui;
pub mod ipc;
pub mod noctalia_config;
pub mod pixel_clock;
pub mod providers;
pub mod router;
pub mod settings_gui;
pub mod shelf;
pub mod snap_gui;
pub mod supervisor;
pub mod switcher;
pub mod welcome_gui;
pub mod delight;
pub mod omnibar;
pub mod gamezone_gui;
pub mod gamezone_live;
pub mod installer_gui;
pub mod living_flow;
pub mod wallpaper;
pub mod we10x_profile;

use anyhow::Result;
use apps::scan_desktop_applications;
use gui::launch_gui_window;
use ipc::ShellIpcClient;
use shelf::ShelfViewModel;
use solar_common::{M3Theme, SolarConfig, SolarSystemState};
use providers::ShellProvider;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

async fn do_minimize(target_id: Option<u64>) -> Result<()> {
    let cfg = SolarConfig::load();
    if cfg.shell.engine != engine::ShellEngine::We10x {
        tracing::info!(
            "Minimize skipped: background minimization is enabled only in We10X mode (active={:?})",
            cfg.shell.engine
        );
        return Ok(());
    }
    {
        let client = solar_niri::NiriClient::new()?;
        let windows = client.get_windows().await?;
        let target = if let Some(id) = target_id {
            windows.into_iter().find(|w| w.id == id)
        } else {
            windows.into_iter().find(|w| {
                w.is_focused && {
                    let app = w.app_id.as_deref().unwrap_or("");
                    let title = w.title.as_deref().unwrap_or("");
                    let is_settings = title.contains("Ayarları") || app == "solar-settings";

                    if is_settings {
                        true
                    } else {
                        !app.contains("solar-shell")
                            && !app.contains("solar-snap")
                            && !app.contains("noctalia")
                    }
                }
            })
        };

        if let Some(win) = target {
            solar_niri::minimized::minimize(&client, win.id).await?;
        }
    }
    Ok(())
}

async fn do_restore(target_id: u64) -> Result<()> {
    solar_niri::minimized::restore(&solar_niri::NiriClient::new()?, target_id).await
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solar_shell=info".into()),
        )
        .init();

    // Ensure WAYLAND_DISPLAY is resolved from XDG_RUNTIME_DIR if missing
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
                let mut sockets: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with("wayland-") && !name.ends_with(".lock") {
                            Some(name)
                        } else {
                            None
                        }
                    })
                    .collect();
                sockets.sort();
                if let Some(last) = sockets.last() {
                    std::env::set_var("WAYLAND_DISPLAY", last);
                }
            }
        }
    }

    // Ensure NIRI_SOCKET is resolved from XDG_RUNTIME_DIR if missing
    if std::env::var("NIRI_SOCKET").is_err() {
        if let Ok(sock) = solar_niri::find_niri_socket() {
            std::env::set_var("NIRI_SOCKET", sock);
        }
    }

    let args: Vec<String> = std::env::args().collect();
    let bin_name = args.first().map(|s| s.as_str()).unwrap_or("");
    let default_cmd = if bin_name.ends_with("solar-settings") {
        "settings"
    } else if bin_name.ends_with("blaze-gamezone") || bin_name.ends_with("gamezone") {
        "gamezone"
    } else if bin_name.ends_with("solar-omnibar") || bin_name.ends_with("omnibar") {
        "omnibar"
    } else if bin_name.ends_with("solar-installer-gui") || bin_name.ends_with("blaze-installer") {
        "installer"
    } else {
        "run"
    };
    let command = match args.get(1).map(|s| s.trim()) {
        Some(arg) if !arg.starts_with('-') => arg,
        _ => default_cmd,
    };

    match command {
        "apps" => {
            println!("=== SolarUI Installed Applications ===");
            let apps = scan_desktop_applications();
            for app in apps {
                println!("{:<30} | {:<25} | {}", app.name, app.id, app.exec);
            }
            Ok(())
        }
        "status" => {
            let supervisor = supervisor::SolarShellSupervisor::new();
            let desired = supervisor.load_desired_state()?;
            let actual = supervisor.detect_actual_engine().await;

            let state = Arc::new(RwLock::new(SolarSystemState::default()));
            let (_client, rx) = ShellIpcClient::new(state.clone());
            let state_clone = state.clone();

            tokio::spawn(async move {
                let _ = ShellIpcClient::run_loop(state_clone, rx, None).await;
            });

            // Wait a moment for IPC connection
            tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;

            let s = state.read().await;
            let vm = ShelfViewModel::from_state(&s);

            let is_in_sync = supervisor::SolarShellSupervisor::is_reconciled(
                desired.desired_engine,
                &actual,
            );
            let sync_status = if is_in_sync {
                "Reconciled (OK)"
            } else {
                "Drifted (Repair Needed)"
            };

            println!("┌────────────────────────────────────────────────────────────────────────┐");
            println!("│                         SOLAR SHELL SUPERVISOR                         │");
            println!("├────────────────────────────────────────────────────────────────────────┤");
            println!(
                "│ Desired Shell Engine: {:<20} Profile: {:<17} │",
                desired.desired_engine.as_str(),
                desired.desired_profile
            );
            println!(
                "│ Actual Shell Engine:  {:<20} Status:  {:<17} │",
                actual.as_str(),
                sync_status
            );
            println!("├────────────────────────────────────────────────────────────────────────┤");
            println!(
                "│ Workspaces: {:<20} Time: {:<8} Date: {:<12}    │",
                vm.workspace_label, vm.clock_time, vm.clock_date
            );
            println!(
                "│ Network:    {:<20} Battery: {:<10}                     │",
                vm.network_label, vm.battery_label
            );
            println!("├────────────────────────────────────────────────────────────────────────┤");
            println!(
                "│ Active Running Windows ({}):                                            │",
                vm.open_windows.len()
            );
            for w in &vm.open_windows {
                let focused = if w.is_focused { "*" } else { " " };
                let floating = if w.is_floating { "(float)" } else { "(tile)" };
                println!(
                    "│  [{}{}] {:<35} {:<10} [id: {:<4}] │",
                    focused, w.id, w.name, floating, w.id
                );
            }
            println!("└────────────────────────────────────────────────────────────────────────┘");
            Ok(())
        }
        "bar" => {
            let state = Arc::new(RwLock::new(SolarSystemState::default()));
            let (_client, rx) = ShellIpcClient::new(state.clone());
            let state_clone = state.clone();

            tokio::spawn(async move {
                let _ = ShellIpcClient::run_loop(state_clone, rx, None).await;
            });

            tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;

            let s = state.read().await;
            let theme = M3Theme::default();
            let bar_model = bar::SolarBarModel::build(&s, &theme);
            println!("{}", bar_model.render_ascii_bar());
            Ok(())
        }
        "autostart" => {
            info!("SolarUI Autostart: Initializing portal environment & reconciling configured shell engine...");
            ensure_portal_and_graphical_session();
            let supervisor = Arc::new(supervisor::SolarShellSupervisor::new());
            if let Err(e) = supervisor.reconcile().await {
                tracing::error!("Failed to reconcile shell engine on autostart: {}", e);
            }

            // Start clipboard history daemon (cliphist + wl-paste)
            std::thread::spawn(|| {
                let _ = std::process::Command::new("wl-paste")
                    .args(["--watch", "cliphist", "store"])
                    .spawn();
            });

            // Start live GSettings wallpaper bridge (syncs Nautilus "Set as Wallpaper" & BlazeHouse to Noctalia/We10X/Caelestia/end4-pC)
            wallpaper::spawn_gsettings_wallpaper_watcher();

            // Re-apply user's saved wallpaper on session startup if configured
            {
                let startup_cfg = SolarConfig::load();
                let saved_wp = if startup_cfg.shell.engine == engine::ShellEngine::We10x
                    && !startup_cfg.shell.we10x_wallpaper.trim().is_empty()
                {
                    startup_cfg.shell.we10x_wallpaper.clone()
                } else {
                    startup_cfg.display.wallpaper.clone()
                };
                if !saved_wp.trim().is_empty() && std::path::Path::new(&saved_wp).is_file() {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(900));
                        let _ = wallpaper::apply_wallpaper_everywhere(&saved_wp, false);
                    });
                }
            }

            // Start ChromeOS Ash-level Watchdog daemon (auto-recovers shell engines within 3 seconds if crashed)
            let _watchdog_handle = supervisor.start_watchdog_loop();

            let cfg = SolarConfig::load();
            let is_live = std::path::Path::new("/run/initramfs/live").exists()
                || std::path::Path::new("/dev/mapper/live-base").exists()
                || std::fs::read_to_string("/proc/cmdline")
                    .map(|c| c.contains("rd.live.image"))
                    .unwrap_or(false);
            let live_installer_present = is_live
                && (std::path::Path::new("/usr/bin/liveinst").exists()
                    || std::path::Path::new("/usr/share/anaconda/gnome/fedora-welcome").exists());
            if cfg.shortcuts_hud.show_at_startup && !live_installer_present {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_millis(2000));
                    let _ = std::process::Command::new("solar-shell")
                        .arg("welcome")
                        .spawn();
                });
            }

            if cfg.taskbar.enabled {
                info!("SolarUI Taskbar is enabled in configuration. Launching taskbar...");
                let state = Arc::new(RwLock::new(SolarSystemState::default()));
                let (client, rx) = ShellIpcClient::new(state.clone());
                launch_gui_window(state, client, rx);
            } else {
                info!("SolarUI Taskbar is disabled in configuration. Keeping Watchdog daemon active...");
                tokio::signal::ctrl_c().await?;
            }
            Ok(())
        }
        "settings" => {
            info!("Launching SolarUI Settings GUI...");
            let mut initial_tab: Option<String> = None;
            let mut idx = 1;
            while idx < args.len() {
                if (args[idx] == "--tab" || args[idx] == "-t") && idx + 1 < args.len() {
                    initial_tab = Some(args[idx + 1].clone());
                    break;
                } else if let Some(rest) = args[idx].strip_prefix("--tab=") {
                    initial_tab = Some(rest.to_string());
                    break;
                }
                idx += 1;
            }
            tokio::task::block_in_place(move || {
                settings_gui::launch_settings_window_with_tab(initial_tab.as_deref());
            });
            Ok(())
        }
        "wallpaper" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "get" => {
                    println!("{}", wallpaper::current_effective_wallpaper());
                }
                "list" => {
                    for (label, path) in wallpaper::discover_available_wallpapers() {
                        println!("{}\t{}", label, path);
                    }
                }
                "set" => {
                    let target = args
                        .get(3)
                        .ok_or_else(|| anyhow::anyhow!("Usage: solar-shell wallpaper set <path>"))?;
                    let applied = wallpaper::apply_wallpaper_everywhere(target, true)?;
                    println!("{}", applied);
                }
                other => {
                    anyhow::bail!(
                        "Unknown wallpaper subcommand '{}'. Valid: get, list, set <path>",
                        other
                    );
                }
            }
            Ok(())
        }
        "display" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
            match sub {
                "list" => {
                    for out in settings_gui::get_detected_outputs() {
                        println!(
                            "{}\t{}\t{}\tscale={:.2}",
                            out.name, out.description, out.resolution, out.scale
                        );
                    }
                }
                "set-mode" => {
                    let out_name = args.get(3).ok_or_else(|| {
                        anyhow::anyhow!("Usage: solar-shell display set-mode <output> <WxH@Hz> [--custom]")
                    })?;
                    let mode_str = args.get(4).ok_or_else(|| {
                        anyhow::anyhow!("Usage: solar-shell display set-mode <output> <WxH@Hz> [--custom]")
                    })?;
                    let is_custom = args.iter().any(|a| a == "--custom");
                    settings_gui::apply_per_output_mode_dynamically(out_name, mode_str, is_custom)?;
                    println!("OK");
                }
                "set-scale" => {
                    let out_name = args.get(3).ok_or_else(|| {
                        anyhow::anyhow!("Usage: solar-shell display set-scale <output> <scale>")
                    })?;
                    let sc: f64 = args
                        .get(4)
                        .and_then(|v| v.parse().ok())
                        .ok_or_else(|| anyhow::anyhow!("Invalid scale value"))?;
                    settings_gui::apply_per_output_scale_dynamically(out_name, sc);
                    println!("OK");
                }
                other => {
                    anyhow::bail!(
                        "Unknown display subcommand '{}'. Valid: list, set-mode, set-scale",
                        other
                    );
                }
            }
            Ok(())
        }
        "welcome" | "shortcuts" => {
            info!("Launching SolarUI Welcome & Shortcuts HUD...");
            tokio::task::block_in_place(|| {
                welcome_gui::launch_welcome_window();
            });
            Ok(())
        }
        "installer" | "install-os" => {
            info!("Launching BlazeOS Solar Native Disk Installer Wizard...");
            tokio::task::block_in_place(|| {
                installer_gui::launch_installer_window();
            });
            Ok(())
        }
        "omnibar" | "spotlight" | "launcher" => {
            supervisor::SolarShellSupervisor::reconcile_niri_config_symlink();
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("");
            match sub {
                "disable" | "off" | "false" => {
                    let _ = SolarConfig::update(|c| {
                        c.omnibar.enabled = false;
                        c.omnibar.override_shell_launcher = false;
                    });
                    // Close any running omnibar instance
                    let _ = omnibar::close_running_omnibar();
                    println!("SolarUI Omnibar devre dışı bırakıldı. Super+Boşluk artık aktif kabuğun kendi başlatıcısını açacak.");
                    return Ok(());
                }
                "enable" | "on" | "true" => {
                    let _ = SolarConfig::update(|c| {
                        c.omnibar.enabled = true;
                    });
                    println!("SolarUI Omnibar etkinleştirildi.");
                    return Ok(());
                }
                "status" | "get" => {
                    let cfg = SolarConfig::load();
                    println!(
                        "enabled={} override_shell_launcher={}",
                        cfg.omnibar.enabled, cfg.omnibar.override_shell_launcher
                    );
                    return Ok(());
                }
                _ => {}
            }

            let force = sub == "--force" || sub == "-f";

            // Toggle behavior: if Omnibar is already open, close it and return
            if omnibar::close_running_omnibar() {
                info!("SolarUI Omnibar was already open; toggled closed.");
                return Ok(());
            }

            let cfg = SolarConfig::load();
            let should_route_to_shell_launcher = !force
                && (!cfg.omnibar.enabled
                    || (cfg.shell.engine != engine::ShellEngine::Noctalia
                        && !cfg.omnibar.override_shell_launcher));

            if should_route_to_shell_launcher {
                info!(
                    "Routing launcher shortcut to active shell ({:?}) instead of Omnibar (omnibar.enabled={}, override_shell_launcher={})",
                    cfg.shell.engine, cfg.omnibar.enabled, cfg.omnibar.override_shell_launcher
                );
                let router = router::SolarSurfaceRouter::new();
                router.route(actions::ShellAction::Launcher, &cfg.shell).await?;
                return Ok(());
            }

            info!("Launching SolarUI Omnibar Command Hub...");
            tokio::task::block_in_place(|| {
                omnibar::launch_omnibar_window();
            });
            Ok(())
        }
        "gamezone" | "gaming" | "deck" => {
            info!("Entering Blaze GameZone Standalone Game Shell mode...");
            run_gamezone_shell_session().await?;
            Ok(())
        }
        "toggle-taskbar" => {
            let mut cfg = SolarConfig::load();
            cfg.taskbar.enabled = !cfg.taskbar.enabled;
            let _ = cfg.save();
            let _ = noctalia_config::sync_taskbar_state(cfg.taskbar.enabled);
            let _ = std::process::Command::new("pkill")
                .args(["-f", "solar-shell (run|gui|taskbar|window)"])
                .status();
            println!(
                "SolarUI Taskbar state toggled: enabled = {}",
                cfg.taskbar.enabled
            );
            Ok(())
        }
        "taskbar" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
            match sub {
                "get" | "status" => {
                    let cfg = SolarConfig::load();
                    println!("{}", cfg.taskbar.enabled);
                    Ok(())
                }
                "set" => {
                    if let Some(val_str) = args.get(3) {
                        let val = val_str.parse::<bool>().unwrap_or(false);
                        let mut cfg = SolarConfig::load();
                        cfg.taskbar.enabled = val;
                        let _ = cfg.save();
                        let _ = noctalia_config::sync_taskbar_state(val);
                        let _ = std::process::Command::new("pkill")
                            .args(["-f", "solar-shell (run|gui|taskbar|window)"])
                            .status();
                        println!("SolarUI Taskbar enabled = {}", val);
                    }
                    Ok(())
                }
                _ => {
                    let cfg = SolarConfig::load();
                    println!("{}", cfg.taskbar.enabled);
                    Ok(())
                }
            }
        }
        "run" | "window" | "gui" => {
            info!("Starting SolarUI KDE Taskbar (BlazeOS Desktop)...");
            let state = Arc::new(RwLock::new(SolarSystemState::default()));
            let (client, rx) = ShellIpcClient::new(state.clone());
            launch_gui_window(state, client, rx);
            Ok(())
        }
        "minimize" => {
            let target_id = args.get(2).and_then(|s| s.parse::<u64>().ok());
            do_minimize(target_id).await?;
            Ok(())
        }
        "restore" => {
            if let Some(target_id) = args.get(2).and_then(|s| s.parse::<u64>().ok()) {
                do_restore(target_id).await?;
            }
            Ok(())
        }
        "snap" | "snap-layout" => {
            info!("Launching SolarUI Snap Layouts HUD...");
            snap_gui::launch_snap_window();
            Ok(())
        }
        "clock" | "pixel-clock" => {
            info!("Launching SolarUI Google Pixel Clock Widget...");
            pixel_clock::launch_pixel_clock();
            Ok(())
        }
        "action" => {
            let action_type = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if action_type == "minimize" {
                let target_id = args.get(3).and_then(|s| s.parse::<u64>().ok());
                do_minimize(target_id).await?;
            } else if action_type == "restore" {
                if let Some(target_id) = args.get(3).and_then(|s| s.parse::<u64>().ok()) {
                    do_restore(target_id).await?;
                }
            } else if action_type == "snap" {
                snap_gui::launch_snap_window();
            }
            Ok(())
        }
        "route" => {
            let action_str = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let action: actions::ShellAction =
                action_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            let cfg = solar_common::SolarConfig::load();
            let router = router::SolarSurfaceRouter::new();
            router.route(action, &cfg.shell).await?;
            Ok(())
        }
        "switch" => {
            let engine_str = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let engine: engine::ShellEngine =
                engine_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            if supervisor::SolarShellSupervisor::is_gamezone_active() {
                let prev_provider = match engine {
                    engine::ShellEngine::Noctalia => engine::SurfaceProvider::Noctalia,
                    engine::ShellEngine::Caelestia => engine::SurfaceProvider::Caelestia,
                    engine::ShellEngine::End4Pc => engine::SurfaceProvider::End4Pc,
                    engine::ShellEngine::We10x => engine::SurfaceProvider::We10x,
                };
                SolarConfig::update(|c| {
                    c.shell.engine = engine;
                    c.shell.launcher_provider = prev_provider;
                    c.shell.dashboard_provider = prev_provider;
                    c.shell.notifications_provider = prev_provider;
                    c.shell.osd_provider = prev_provider;
                })?;
                let prev_shell_file = solar_common::get_solar_runtime_dir().join("gamezone_prev_shell.txt");
                let _ = std::fs::write(&prev_shell_file, engine.as_str());
                info!(
                    "GameZone aktifken kabuk tercihi {:?} olarak güncellendi; GameZone'dan çıkıldığında uygulanacak.",
                    engine
                );
                return Ok(());
            }
            let switcher = switcher::SolarShellSwitcher::new();
            switcher.switch_engine(engine).await?;
            Ok(())
        }
        "engine" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("get");
            if sub == "get" {
                let supervisor = supervisor::SolarShellSupervisor::new();
                let desired = supervisor.load_desired_state()?;
                println!("{}", desired.desired_engine.as_str());
            } else if sub == "set" {
                if let Some(eng_str) = args.get(3) {
                    let engine: engine::ShellEngine =
                        eng_str.parse().map_err(|e: String| anyhow::anyhow!(e))?;
                    let switcher = switcher::SolarShellSwitcher::new();
                    switcher.switch_engine(engine).await?;
                }
            }
            Ok(())
        }
        "noctalia-config" => {
            let action = args.get(2).map(|s| s.as_str()).unwrap_or("get");
            match action {
                "get" => {
                    let json = noctalia_config::get_config_json()?;
                    println!("{}", json);
                }
                "set" => {
                    if args.len() < 5 {
                        eprintln!("Usage: solar-shell noctalia-config set <key.path> <value>");
                    } else {
                        let key = &args[3];
                        let val = &args[4];
                        noctalia_config::set_config_value(key, val)?;
                        println!("OK");
                    }
                }
                "reload" => {
                    let _ = std::process::Command::new("noctalia")
                        .args(["msg", "reload"])
                        .status();
                    println!("RELOADED");
                }
                other => {
                    eprintln!(
                        "Unknown noctalia-config action: {}. Valid: get, set, reload",
                        other
                    );
                }
            }
            Ok(())
        }
        "supervisor" | "reconcile" => {
            let supervisor = supervisor::SolarShellSupervisor::new();
            supervisor.reconcile().await?;
            Ok(())
        }
        "sync-noctalia" => {
            sync_noctalia_settings().await?;
            Ok(())
        }
        "easter-egg" | "konami" => {
            let egg = args.get(2).map(|s| s.as_str()).unwrap_or("konami");
            delight::trigger_easter_egg(egg)?;
            Ok(())
        }
        "sound" => {
            let snd = args.get(2).map(|s| s.as_str()).unwrap_or("click");
            delight::play_acoustic_feedback(snd)?;
            Ok(())
        }
        "atmosphere" | "season" => {
            let (name, color) = delight::get_seasonal_atmosphere();
            println!("Aktif Mevsimsel Atmosfer: {} (Vurgu Rengi: {})", name, color);
            Ok(())
        }
        "recovery" => {
            if std::io::IsTerminal::is_terminal(&std::io::stdout()) {
                let _ = std::process::Command::new("blaze-recovery").status();
            } else {
                let _ = std::process::Command::new("ptyxis")
                    .args(["--", "blaze-recovery"])
                    .spawn();
            }
            Ok(())
        }
        "oobe" | "setup" => {
            let _ = std::process::Command::new("/usr/bin/blaze-setup")
                .spawn();
            Ok(())
        }
        "show-desktop" => {
            we10x_profile::toggle_show_desktop().await?;
            Ok(())
        }
        "aero-snap" => {
            let dir = args.get(2).map(|s| s.as_str()).unwrap_or("up");
            we10x_profile::aero_snap_focused(dir).await?;
            Ok(())
        }
        "we10x-ipc" => {
            let target = args.get(2).map(|s| s.as_str()).unwrap_or("startMenu");
            let func = args.get(3).map(|s| s.as_str()).unwrap_or("toggle");
            providers::We10xProvider::new()
                .ipc_checked(target, func)
                .await?;
            Ok(())
        }
        "we10x-config" => {
            let sub = args.get(2).map(|s| s.as_str()).unwrap_or("get");
            match sub {
                "get" => {
                    let cfg = SolarConfig::load();
                    let is_live = std::path::Path::new("/run/initramfs/live").exists()
                        || std::path::Path::new("/dev/mapper/live-base").exists()
                        || std::fs::read_to_string("/proc/cmdline")
                            .map(|c| c.contains("rd.live.image"))
                            .unwrap_or(false);
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/home/liveuser".into());
                    let trash_files = std::path::Path::new(&home).join(".local/share/Trash/files");
                    let trash_has_items = std::fs::read_dir(&trash_files)
                        .map(|mut it| it.next().is_some())
                        .unwrap_or(false);
                    let user = std::env::var("USER").unwrap_or_else(|_| "liveuser".into());
                    let eff_wp = if !cfg.shell.we10x_wallpaper.trim().is_empty()
                        && std::path::Path::new(&cfg.shell.we10x_wallpaper).is_file()
                    {
                        cfg.shell.we10x_wallpaper.clone()
                    } else if !cfg.display.wallpaper.trim().is_empty()
                        && std::path::Path::new(&cfg.display.wallpaper).is_file()
                    {
                        cfg.display.wallpaper.clone()
                    } else {
                        String::new()
                    };
                    let payload = serde_json::json!({
                        "theme_mode": cfg.shell.we10x_theme_mode,
                        "start_icon": cfg.shell.we10x_start_icon,
                        "accent_color": we10x_profile::normalize_hex_color(&cfg.shell.we10x_accent_color),
                        "colorize_surfaces": cfg.shell.we10x_colorize_surfaces,
                        "taskbar_opacity_pct": cfg.shell.we10x_taskbar_opacity_pct,
                        "wallpaper": eff_wp,
                        "desktop_icons": cfg.shell.we10x_desktop_icons,
                        "is_live": is_live,
                        "trash_has_items": trash_has_items,
                        "username": user,
                        "home": home,
                    });
                    println!("{}", payload);
                }
                "set-theme" => {
                    if let Some(mode) = args.get(3) {
                        let normalized = if mode.eq_ignore_ascii_case("light") {
                            "light"
                        } else if mode.eq_ignore_ascii_case("oled") || mode.eq_ignore_ascii_case("black") {
                            "oled"
                        } else {
                            "dark"
                        };
                        SolarConfig::update(|c| {
                            c.shell.we10x_theme_mode = normalized.to_string();
                        })?;
                        let cfg = SolarConfig::load();
                        if cfg.shell.engine == engine::ShellEngine::We10x {
                            let _ = we10x_profile::enter_we10x_profile().await;
                            let _ = providers::We10xProvider::new()
                                .ipc("reloadConfig", "reload")
                                .await;
                        }
                    }
                }
                "set-accent" => {
                    if let Some(raw_hex) = args.get(3) {
                        let hex = we10x_profile::normalize_hex_color(raw_hex);
                        SolarConfig::update(|c| {
                            c.shell.we10x_accent_color = hex;
                        })?;
                        let cfg = SolarConfig::load();
                        if cfg.shell.engine == engine::ShellEngine::We10x {
                            let _ = we10x_profile::enter_we10x_profile().await;
                            let _ = providers::We10xProvider::new()
                                .ipc("reloadConfig", "reload")
                                .await;
                        }
                    }
                }
                "set-colorize" => {
                    if let Some(v) = args.get(3) {
                        let enabled = matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "on");
                        SolarConfig::update(|c| {
                            c.shell.we10x_colorize_surfaces = enabled;
                        })?;
                        let cfg = SolarConfig::load();
                        if cfg.shell.engine == engine::ShellEngine::We10x {
                            let _ = we10x_profile::enter_we10x_profile().await;
                            let _ = providers::We10xProvider::new()
                                .ipc("reloadConfig", "reload")
                                .await;
                        }
                    }
                }
                "set-opacity" => {
                    if let Some(v) = args.get(3) {
                        let pct = v.parse::<u8>().unwrap_or(95).clamp(45, 100);
                        SolarConfig::update(|c| {
                            c.shell.we10x_taskbar_opacity_pct = pct;
                        })?;
                        let _ = providers::We10xProvider::new()
                            .ipc("reloadConfig", "reload")
                            .await;
                    }
                }
                "set-wallpaper" => {
                    if let Some(path) = args.get(3) {
                        wallpaper::apply_wallpaper_everywhere(path, true)?;
                    }
                }
                "set-start-icon" => {
                    if let Some(icon) = args.get(3) {
                        let normalized = if icon.eq_ignore_ascii_case("blaze") {
                            "blaze"
                        } else {
                            "we10x"
                        };
                        SolarConfig::update(|c| {
                            c.shell.we10x_start_icon = normalized.to_string();
                        })?;
                        let _ = providers::We10xProvider::new()
                            .ipc("reloadConfig", "reload")
                            .await;
                    }
                }
                "set-desktop-icons" => {
                    if let Some(v) = args.get(3) {
                        let enabled = matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "on");
                        SolarConfig::update(|c| {
                            c.shell.we10x_desktop_icons = enabled;
                        })?;
                        let _ = providers::We10xProvider::new()
                            .ipc("reloadConfig", "reload")
                            .await;
                    }
                }
                "provision-user" => {
                    let target_home = args
                        .get(3)
                        .map(std::path::PathBuf::from)
                        .ok_or_else(|| anyhow::anyhow!("Usage: solar-shell we10x-config provision-user <home_dir> [dark|light]"))?;
                    let mode = args.get(4).map(|s| s.as_str()).unwrap_or("dark");
                    we10x_profile::provision_user_we10x_home(
                        &target_home,
                        std::path::Path::new("/etc/solarui/niri.kdl"),
                        mode,
                    )?;
                }
                other => {
                    anyhow::bail!(
                        "Unknown we10x-config subcommand '{}'. Valid: get, set-theme, set-start-icon, set-desktop-icons, provision-user",
                        other
                    );
                }
            }
            Ok(())
        }
        other => {
            anyhow::bail!("Unknown command: '{}'. Valid: omnibar, gamezone, route, switch, sync-noctalia, supervisor, run, autostart, settings, welcome, snap, minimize, restore, show-desktop, aero-snap, we10x-ipc, we10x-config, toggle-taskbar, apps, status, bar, easter-egg, sound, atmosphere, recovery, oobe", other);
        }
    }
}

async fn run_gamezone_shell_session() -> Result<()> {
    // 0. Detach from parent process group so that stopping the parent shell (e.g. quickshell)
    // does not deliver SIGHUP/SIGTERM to GameZone.
    unsafe {
        libc::setsid();
    }

    // 1. Mevcut aktif/istenen masaüstü kabuk motorunu tespit et ve kaydet
    let supervisor = supervisor::SolarShellSupervisor::new();
    let desired = supervisor.load_desired_state().unwrap_or_default();
    let prev_engine = desired.desired_engine;

    let runtime_dir = solar_common::get_solar_runtime_dir();
    let _ = std::fs::create_dir_all(&runtime_dir);
    let prev_shell_file = runtime_dir.join("gamezone_prev_shell.txt");
    let lock_file = runtime_dir.join("gamezone-active.lock");

    let _ = std::fs::write(&prev_shell_file, prev_engine.as_str());
    let _ = std::fs::write(&lock_file, std::process::id().to_string());

    info!(
        "GameZone kabuk oturumu başlatıldı. Önceki masaüstü motoru kaydedildi: {:?}. Arka plan kabukları askıya alınıyor...",
        prev_engine
    );

    // 2. Arka plan masaüstü kabuklarını (Noctalia, Caelestia, End4, We10x) durdurarak RAM ve GPU'yu tamamen oyuna aç
    let noctalia = providers::NoctaliaProvider::default();
    let _ = noctalia.stop().await;

    let caelestia = providers::CaelestiaProvider::default();
    let _ = caelestia.stop().await;

    let end4 = providers::End4PcProvider::default();
    let _ = end4.stop().await;

    let we10x = providers::We10xProvider::default();
    let _ = we10x.stop().await;

    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-x", "noctalia"])
        .status();
    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-f", "(qs|quickshell).*caelestia"])
        .status();
    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-f", "(qs|quickshell).*end4"])
        .status();
    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-f", "(qs|quickshell).*we10x"])
        .status();
    let _ = std::process::Command::new("pkill")
        .args(["-TERM", "-f", "solar-shell (run|gui|taskbar|window)"])
        .status();

    // 3. GameZone 10-Foot UI Kabuğunu çalıştır (kullanıcı çıkana kadar bekler)
    tokio::task::block_in_place(|| {
        gamezone_gui::launch_gamezone_window();
    });

    // Kullanıcı GameZone içerisinden (veya SolarUI Ayarları'ndan) kabuk tercihini değiştirdiyse yeni tercihi yükle
    let target_engine = std::fs::read_to_string(&prev_shell_file)
        .ok()
        .and_then(|s| s.trim().parse::<engine::ShellEngine>().ok())
        .or_else(|| supervisor.load_desired_state().ok().map(|d| d.desired_engine))
        .unwrap_or(prev_engine);

    info!(
        "GameZone kabuğundan çıkış yapıldı. Masaüstü kabuğuna geri dönülüyor: {:?}...",
        target_engine
    );

    // 4. Kilit dosyasını kaldır
    let _ = std::fs::remove_file(&lock_file);
    let _ = std::fs::remove_file("/tmp/solar-gamezone-active.lock");

    // 5. Kaydedilen/seçilen masaüstü kabuğunu otomatik olarak yeniden başlat
    let switcher = switcher::SolarShellSwitcher::new();
    if let Err(e) = switcher.switch_engine(target_engine).await {
        tracing::error!(
            "Masaüstü kabuğunu ({:?}) geri yükleme başarısız oldu: {}. Reconcile deneniyor...",
            target_engine, e
        );
        let _ = supervisor.reconcile().await;
    }

    Ok(())
}

async fn sync_noctalia_settings() -> Result<()> {
    let home = std::env::var("HOME").unwrap_or_else(|_| {
        std::env::var("USER").map(|u| format!("/home/{}", u)).unwrap_or_else(|_| "/tmp".to_string())
    });
    let state_file = std::path::PathBuf::from(home).join(".local/state/noctalia/settings.toml");

    if !state_file.exists() {
        println!("Noctalia settings file not found: {:?}", state_file);
        return Ok(());
    }

    let content = std::fs::read_to_string(&state_file)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let mut cfg = solar_common::SolarConfig::load();
    let mut changed = false;
    let mut engine_to_switch: Option<engine::ShellEngine> = None;

    if let Some(solarui) = parsed.get("solarui") {
        if let Some(engine_val) = solarui.get("shell_engine").and_then(|v| v.as_str()) {
            if let Ok(new_engine) = engine_val.parse::<engine::ShellEngine>() {
                if new_engine != cfg.shell.engine {
                    info!(
                        "Noctalia GUI'den kabuk motoru değişimi algılandı: {:?} -> {:?}",
                        cfg.shell.engine, new_engine
                    );
                    cfg.shell.engine = new_engine;
                    changed = true;
                    engine_to_switch = Some(new_engine);
                }
            }
        }

        if let Some(launcher_val) = solarui.get("launcher_provider").and_then(|v| v.as_str()) {
            if let Ok(new_launcher) = launcher_val.parse::<engine::SurfaceProvider>() {
                if new_launcher != cfg.shell.launcher_provider {
                    info!(
                        "Noctalia GUI'den başlatıcı sağlayıcı değişimi algılandı: {:?} -> {:?}",
                        cfg.shell.launcher_provider, new_launcher
                    );
                    cfg.shell.launcher_provider = new_launcher;
                    changed = true;
                }
            }
        }

        if let Some(dash_val) = solarui.get("dashboard_provider").and_then(|v| v.as_str()) {
            if let Ok(new_dash) = dash_val.parse::<engine::SurfaceProvider>() {
                if new_dash != cfg.shell.dashboard_provider {
                    info!(
                        "Noctalia GUI'den dashboard sağlayıcı değişimi algılandı: {:?} -> {:?}",
                        cfg.shell.dashboard_provider, new_dash
                    );
                    cfg.shell.dashboard_provider = new_dash;
                    changed = true;
                }
            }
        }

        if let Some(perf_val) = solarui.get("performance_profile").and_then(|v| v.as_str()) {
            if let Ok(new_perf) = perf_val.parse::<solar_common::PerformanceProfile>() {
                if new_perf != cfg.performance.profile {
                    info!(
                        "Noctalia GUI'den performans profili değişimi algılandı: {:?} -> {:?}",
                        cfg.performance.profile, new_perf
                    );
                    cfg.performance.profile = new_perf;
                    changed = true;
                }
            }
        }
    }

    if changed {
        let _ = cfg.save();
        info!("SolarConfig güncellendi ve kaydedildi.");
    }

    if let Some(target_engine) = engine_to_switch {
        let switcher = switcher::SolarShellSwitcher::new();
        let _ = switcher.switch_engine(target_engine).await;
    }

    let _ = std::process::Command::new("notify-send")
        .args([
            "-a",
            "SolarUI",
            "SolarUI Kabuk ve Yüzey Senkronizasyonu",
            &format!(
                "Motor: {:?} | Başlatıcı: {:?} | Dashboard: {:?}",
                cfg.shell.engine, cfg.shell.launcher_provider, cfg.shell.dashboard_provider
            ),
        ])
        .status();

    Ok(())
}

fn ensure_portal_and_graphical_session() {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = std::path::PathBuf::from(home);

        let portal_dir = home_path.join(".config/xdg-desktop-portal");
        let _ = std::fs::create_dir_all(&portal_dir);
        let portal_conf = portal_dir.join("solarui-portals.conf");
        if !portal_conf.exists() {
            let content = "[preferred]\ndefault=gnome;gtk;\norg.freedesktop.impl.portal.Access=gtk;\norg.freedesktop.impl.portal.Notification=gtk;\norg.freedesktop.impl.portal.Secret=gnome-keyring;\n";
            let _ = std::fs::write(&portal_conf, content);
        }

        let systemd_dir = home_path.join(".config/systemd/user");
        let _ = std::fs::create_dir_all(&systemd_dir);
        let service_file = systemd_dir.join("solarui-session.service");
        if !service_file.exists() {
            let content = "[Unit]\nDescription=SolarUI Session Manager\nBindsTo=graphical-session.target\nBefore=graphical-session.target\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/usr/bin/true\nNice=-10\nOOMScoreAdjust=-500\n";
            let _ = std::fs::write(&service_file, content);
        }
    }

    std::env::set_var("XDG_CURRENT_DESKTOP", "SolarUI");
    std::env::set_var("XDG_SESSION_DESKTOP", "SolarUI");
    std::env::set_var("DESKTOP_SESSION", "SolarUI");

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args([
            "--user",
            "import-environment",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ])
        .status();
    let _ = std::process::Command::new("dbus-update-activation-environment")
        .args([
            "--systemd",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ])
        .status();
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "start", "solarui-session.service"])
        .status();
}
