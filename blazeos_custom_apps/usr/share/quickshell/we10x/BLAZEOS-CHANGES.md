# BlazeOS / SolarUI Changes for We10X Shell

1. **Zero-KDE Quickshell (Qt6/QML) Native Port**:
   - Ported the `We10XOS-kde` desktop experience (`Taskbar.qml`, `StartMenu.qml`, `ActionCenter.qml`, `CalendarFlyout.qml`, `DesktopIcons.qml`, `AltTabSwitcher.qml`, `AeroSnapOverlay.qml`) directly onto `quickshell-fedora` (Qt 6.11) and `Niri` Wayland compositor.
   - `sddm/` from upstream was intentionally excluded; BlazeOS retains its stock SDDM / OOBE chain.
2. **Wayland Toplevel Minimize & Server-Side Titlebar Integration**:
   - Connected taskbar buttons, `Show Desktop` (`Win+D`), and `Alt+Tab` directly to `zwlr_foreign_toplevel_manager_v1` (`State::Minimized`) and Niri's `"background"` parking workspace.
   - Paired with Niri's optional 30px Windows 10 server-side titlebar (`─ □ ✕` with `#E81123` close hover) and `#0078D7` window border profile.
3. **Atomic Profile Snapshot & Safe Rollback**:
   - Managed by `SolarShellSwitcher` (`ShellEngine::We10x`): snapshots user `niri.kdl`, GTK icon theme, `button-layout`, and wallpaper prior to entering `We10X`, and restores all parked/minimized windows plus stock SolarUI settings when switching back to `Noctalia`, `Caelestia`, or `end4-pC`.
