use niri_config::Config;
use smithay::reexports::wayland_protocols_wlr::foreign_toplevel::v1::server::zwlr_foreign_toplevel_handle_v1;
use smithay::utils::Point;

use super::*;
use crate::layout::{HitType, TitlebarButton};

#[test]
fn xdg_minimize_request_parks_on_background_and_unminimize_restores() {
    let kdl = r#"
        workspace "1"
        workspace "background"
        layout {
            server-titlebar {
                on
                allow-minimize true
            }
        }
    "#;
    let config = Config::parse_mem(kdl).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    window.attach_new_buffer();
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    // Initially mapped on workspace "1" and not on "background".
    let server_surface = {
        let mut found = None;
        for (_, _, ws) in f.niri().layout.workspaces() {
            for mapped in ws.windows() {
                found = Some((
                    mapped.toplevel().wl_surface().clone(),
                    ws.name().cloned(),
                ));
            }
        }
        found.expect("window must be mapped")
    };
    assert_eq!(server_surface.1.as_deref(), Some("1"));
    assert!(!f.niri().is_surface_on_background(&server_surface.0));

    // Client requests xdg_toplevel::set_minimized (e.g. clicking GTK4/Electron CSD minimize button).
    let window = f.client(id).window(&surface);
    window.xdg_toplevel.set_minimized();
    f.double_roundtrip(id);
    crate::protocols::foreign_toplevel::refresh(f.niri_state());

    assert!(
        f.niri().is_surface_on_background(&server_surface.0),
        "minimized window must be parked on the 'background' workspace"
    );
    let minimized_flag = zwlr_foreign_toplevel_handle_v1::State::Minimized as u32;
    let states = f
        .niri()
        .foreign_toplevel_state
        .states_for_surface(&server_surface.0)
        .expect("foreign_toplevel entry must exist");
    assert!(
        states.contains(&minimized_flag),
        "foreign_toplevel states must include State::Minimized when parked on background"
    );

    // Unminimizing (e.g. clicking taskbar button) restores window to workspace "1" and clears Minimized.
    f.niri()
        .unminimize_window_by_surface(&server_surface.0, true);
    crate::protocols::foreign_toplevel::refresh(f.niri_state());

    assert!(!f.niri().is_surface_on_background(&server_surface.0));
    let restored_ws_name = f
        .niri()
        .layout
        .workspaces()
        .find(|(_, _, ws)| ws.find_wl_surface(&server_surface.0).is_some())
        .and_then(|(_, _, ws)| ws.name().cloned());
    assert_eq!(restored_ws_name.as_deref(), Some("1"));

    let states = f
        .niri()
        .foreign_toplevel_state
        .states_for_surface(&server_surface.0)
        .unwrap();
    assert!(
        !states.contains(&minimized_flag),
        "foreign_toplevel states must clear State::Minimized upon restore"
    );
}

#[test]
fn server_titlebar_reserves_30px_and_hit_tests_caption_buttons() {
    let kdl = r##"
        layout {
            border {
                on
                width 1
                active-color "#0078d7"
            }
            server-titlebar {
                on
                height 30
                aero-snap true
                allow-minimize true
            }
        }
        window-rule {
            open-floating true
            default-column-width { fixed 600; }
            default-window-height { fixed 400; }
            server-titlebar true
        }
    "##;
    let config = Config::parse_mem(kdl).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    let (_, configure) = window.configures_received.last().unwrap().clone();
    assert_eq!(configure.size, (600, 400));
    window.attach_new_buffer();
    window.set_size(configure.size.0 as u16, configure.size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let tile_info = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        let tile = ws.tiles().next().unwrap();
        let tsz = tile.tile_size();
        let tb_h = tile.effective_titlebar_height();
        let hit_drag = tile.hit(Point::from((80.0, 15.0)));
        let hit_close = tile.hit(Point::from((tsz.w - 10.0, 15.0)));
        let hit_max = tile.hit(Point::from((tsz.w - 60.0, 15.0)));
        let hit_min = tile.hit(Point::from((tsz.w - 110.0, 15.0)));
        (tsz, tb_h, hit_drag, hit_close, hit_max, hit_min)
    };

    assert_eq!(tile_info.1, 30.0);
    // 600x400 client + 1px border on each side + 30px server titlebar => 602 x 432
    assert_eq!(tile_info.0.w, 602.0);
    assert_eq!(tile_info.0.h, 432.0);
    assert_eq!(tile_info.2, Some(HitType::Titlebar { button: None }));
    assert_eq!(
        tile_info.3,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Close)
        })
    );
    assert_eq!(
        tile_info.4,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Maximize)
        })
    );
    assert_eq!(
        tile_info.5,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Minimize)
        })
    );
}

#[test]
fn move_grab_aero_snap_snaps_left_right_half_on_screen_and_top_maximizes() {
    use smithay::backend::input::ButtonState;
    use smithay::input::pointer::{
        ButtonEvent, CursorIcon, Focus, GrabStartData as PointerGrabStartData, MotionEvent,
    };
    use smithay::utils::SERIAL_COUNTER;
    use crate::input::move_grab::MoveGrab;
    use crate::input::AnyStartData;
    use crate::layout::LayoutElement as _;

    let kdl = r##"
        layout {
            struts {
                bottom 44
            }
            border {
                on
                width 1
                active-color "#0078d7"
            }
            server-titlebar {
                on
                height 30
                aero-snap true
                allow-minimize true
            }
        }
        window-rule {
            open-floating true
            default-column-width { fixed 600; }
            default-window-height { fixed 400; }
            server-titlebar true
        }
    "##;
    let config = Config::parse_mem(kdl).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    let (_, configure) = window.configures_received.last().unwrap().clone();
    window.attach_new_buffer();
    window.set_size(configure.size.0 as u16, configure.size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);
    f.niri_complete_animations();

    let server_win = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        ws.windows().next().unwrap().window.clone()
    };

    // Helper: drag window via MoveGrab from `start` to `end` and release left mouse button.
    let drag_and_release = |f: &mut Fixture, start: (f64, f64), end: (f64, f64)| {
        let state = f.niri_state();
        let pointer = state.niri.seat.get_pointer().unwrap();
        let s1 = SERIAL_COUNTER.next_serial();
        let start_pt = Point::from(start);
        pointer.motion(
            state,
            None,
            &MotionEvent {
                location: start_pt,
                serial: s1,
                time: 10,
            },
        );
        pointer.frame(state);

        let start_data = AnyStartData::Pointer(PointerGrabStartData {
            focus: None,
            button: 0x110,
            location: start_pt,
        });
        let grab = MoveGrab::new(
            state,
            start_data,
            server_win.clone(),
            false,
            Some(CursorIcon::Default),
        )
        .expect("MoveGrab must initialize");
        let s2 = SERIAL_COUNTER.next_serial();
        pointer.set_grab(state, grab, s2, Focus::Clear);

        // Move far enough to exceed gesture recognition threshold, then into target edge zone.
        let mid_pt = Point::from(((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0));
        pointer.motion(
            state,
            None,
            &MotionEvent {
                location: mid_pt,
                serial: SERIAL_COUNTER.next_serial(),
                time: 30,
            },
        );
        pointer.frame(state);

        pointer.motion(
            state,
            None,
            &MotionEvent {
                location: Point::from(end),
                serial: SERIAL_COUNTER.next_serial(),
                time: 60,
            },
        );
        pointer.frame(state);

        pointer.button(
            state,
            &ButtonEvent {
                button: 0x110,
                state: ButtonState::Released,
                serial: SERIAL_COUNTER.next_serial(),
                time: 90,
            },
        );
        pointer.frame(state);
    };

    // 1. Drag to RIGHT edge (x = 1918): must snap to right half (x = 960, width = 960), NOT off-screen x = 1920!
    drag_and_release(&mut f, (700.0, 200.0), (1918.0, 300.0));
    f.double_roundtrip(id);
    {
        let window = f.client(id).window(&surface);
        let (_, configure) = window.configures_received.last().unwrap().clone();
        // 1920 * 50% = 960 tile width -> minus 2px border = 958 client width
        // 1080 working_area height (no layer-shell bar mapped in headless fixture) -> minus 30px titlebar - 2px border = 1048 client height
        assert_eq!(configure.size.0, 958);
        assert_eq!(configure.size.1, 1048);
        window.set_size(configure.size.0 as u16, configure.size.1 as u16);
        window.ack_last_and_commit();
    }
    f.double_roundtrip(id);
    f.niri_complete_animations();

    let right_pos = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        ws.tiles_with_ipc_layouts()
            .next()
            .unwrap()
            .1
            .tile_pos_in_workspace_view
            .unwrap()
    };
    assert!(
        (right_pos.0 - 960.0).abs() <= 2.0,
        "RightHalf Aero Snap must place tile at x=960 (50% of 1920), got {right_pos:?}"
    );
    assert!((right_pos.1 - 0.0).abs() <= 2.0);

    // 2. Drag to LEFT edge (x = 2): must snap to left half (x = 0).
    drag_and_release(&mut f, (1100.0, 200.0), (2.0, 300.0));
    f.double_roundtrip(id);
    f.niri_complete_animations();
    let left_pos = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        ws.tiles_with_ipc_layouts()
            .next()
            .unwrap()
            .1
            .tile_pos_in_workspace_view
            .unwrap()
    };
    assert!(
        (left_pos.0 - 0.0).abs() <= 2.0,
        "LeftHalf Aero Snap must place tile at x=0, got {left_pos:?}"
    );

    // 3. Drag to TOP edge (y = 2): must maximize window.
    drag_and_release(&mut f, (300.0, 100.0), (600.0, 2.0));
    f.double_roundtrip(id);
    let is_max = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        ws.windows()
            .next()
            .unwrap()
            .pending_sizing_mode()
            .is_maximized()
    };
    assert!(is_max, "TopMaximize Aero Snap must maximize the window");
}

#[test]
fn server_titlebar_hit_test_matches_at_fractional_hidpi_scale() {
    let kdl = r##"
        output "headless-1" {
            scale 1.25
        }
        layout {
            border {
                on
                width 1
            }
            server-titlebar {
                on
                height 30
            }
        }
        window-rule {
            open-floating true
            default-column-width { fixed 600; }
            default-window-height { fixed 400; }
            server-titlebar true
        }
    "##;
    let config = Config::parse_mem(kdl).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    let (_, configure) = window.configures_received.last().unwrap().clone();
    window.attach_new_buffer();
    window.set_size(configure.size.0 as u16, configure.size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let (_, _, ws) = f
        .niri()
        .layout
        .workspaces()
        .find(|(_, _, ws)| ws.windows().count() > 0)
        .unwrap();
    let tile = ws.tiles().next().unwrap();
    let tsz = tile.tile_size();
    // At scale 1.25, physical btn_w = round(46 * 1.25) = 58px => 46.4 logical px.
    // Verify boundary right at rx = 46.2 hits Close (matching Cairo's 58px button) rather than Maximize!
    let bw = tile.effective_border_width().unwrap_or(0.);
    let hit_close_edge = tile.hit(Point::from((tsz.w - bw - 46.2, 15.0)));
    let hit_max_edge = tile.hit(Point::from((tsz.w - bw - 46.6, 15.0)));
    assert_eq!(
        hit_close_edge,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Close)
        })
    );
    assert_eq!(
        hit_max_edge,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Maximize)
        })
    );
}

#[test]
fn allow_minimize_false_disables_minimize_button_and_xdg_minimize() {
    let kdl = r##"
        workspace "1"
        workspace "background"
        layout {
            border {
                on
                width 1
            }
            server-titlebar {
                on
                height 30
                allow-minimize false
            }
        }
        window-rule {
            open-floating true
            default-column-width { fixed 600; }
            default-window-height { fixed 400; }
            server-titlebar true
        }
    "##;
    let config = Config::parse_mem(kdl).unwrap();
    let mut f = Fixture::with_config(config);
    f.add_output(1, (1920, 1080));

    let id = f.add_client();
    let window = f.client(id).create_window();
    let surface = window.surface.clone();
    window.commit();
    f.roundtrip(id);

    let window = f.client(id).window(&surface);
    let (_, configure) = window.configures_received.last().unwrap().clone();
    window.attach_new_buffer();
    window.set_size(configure.size.0 as u16, configure.size.1 as u16);
    window.ack_last_and_commit();
    f.double_roundtrip(id);

    let (server_surface, hits) = {
        let (_, _, ws) = f
            .niri()
            .layout
            .workspaces()
            .find(|(_, _, ws)| ws.windows().count() > 0)
            .unwrap();
        let tile = ws.tiles().next().unwrap();
        let tsz = tile.tile_size();
        let hit_close = tile.hit(Point::from((tsz.w - 10.0, 15.0)));
        let hit_max = tile.hit(Point::from((tsz.w - 60.0, 15.0)));
        let hit_min_slot = tile.hit(Point::from((tsz.w - 110.0, 15.0)));
        let mapped = ws.windows().next().unwrap();
        (
            mapped.toplevel().wl_surface().clone(),
            (hit_close, hit_max, hit_min_slot),
        )
    };

    assert_eq!(
        hits.0,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Close)
        })
    );
    assert_eq!(
        hits.1,
        Some(HitType::Titlebar {
            button: Some(TitlebarButton::Maximize)
        })
    );
    assert_eq!(
        hits.2,
        Some(HitType::Titlebar { button: None }),
        "When allow-minimize is false, the 3rd caption slot must act as a drag region rather than Minimize"
    );

    let window = f.client(id).window(&surface);
    window.xdg_toplevel.set_minimized();
    f.double_roundtrip(id);
    crate::protocols::foreign_toplevel::refresh(f.niri_state());

    assert!(
        !f.niri().is_surface_on_background(&server_surface),
        "xdg_toplevel::set_minimized must not park window on 'background' when allow-minimize is false"
    );
    let minimized_flag = zwlr_foreign_toplevel_handle_v1::State::Minimized as u32;
    let states = f
        .niri()
        .foreign_toplevel_state
        .states_for_surface(&server_surface)
        .unwrap_or_default();
    assert!(
        !states.contains(&minimized_flag),
        "foreign_toplevel state must not become Minimized when allow-minimize is false"
    );
}
