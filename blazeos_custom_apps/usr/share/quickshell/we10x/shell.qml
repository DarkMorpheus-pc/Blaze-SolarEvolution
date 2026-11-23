//@ pragma UseQApplication
//@ pragma Env QS_NO_RELOAD_POPUP=1
//@ pragma Env QT_QUICK_CONTROLS_STYLE=Basic
//@ pragma Env QT_LOGGING_RULES=qt.text.font.db.info=false
import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Services.Notifications
import "modules"

ShellRoot {
    id: root

    NotificationServer {
        id: notifServer
        keepOnReload: true
        bodySupported: true
        actionsSupported: true
        persistenceSupported: true
        onNotification: (notif) => {
            notif.tracked = true
        }
    }

    // ── SolarUI Supervisor Health Probe ────────────────────────────────────
    IpcHandler {
        target: "solarui"
        function ping(): string {
            return We10xState.ready ? "ready" : "loading"
        }
    }

    // ── Live Configuration Reload IPC ──────────────────────────────────────
    IpcHandler {
        target: "reloadConfig"
        function reload(): string {
            We10xState.reloadConfig()
            return "ok"
        }
    }

    // ── Start Menu & Embedded Search Box IPC ───────────────────────────────
    IpcHandler {
        target: "startMenu"
        function toggle(): string {
            We10xState.toggleStartMenu(false)
            return "ok"
        }
        function open(): string {
            We10xState.closeAllFlyouts()
            We10xState.startMenuOpen = true
            return "ok"
        }
        function close(): string {
            We10xState.startMenuOpen = false
            return "ok"
        }
    }

    IpcHandler {
        target: "searchBox"
        function toggle(): string {
            We10xState.toggleStartMenu(true)
            return "ok"
        }
    }

    // ── Action Center & Calendar IPC ───────────────────────────────────────
    IpcHandler {
        target: "actionCenter"
        function toggle(): string {
            We10xState.toggleActionCenter()
            return "ok"
        }
    }

    IpcHandler {
        target: "calendar"
        function toggle(): string {
            We10xState.toggleCalendar()
            return "ok"
        }
    }

    // ── Window Switcher (Alt+Tab), Task View, Show Desktop & Aero Snap IPC ─
    IpcHandler {
        target: "altTab"
        function toggle(): string {
            We10xState.toggleAltTab()
            return "ok"
        }
    }

    IpcHandler {
        target: "taskView"
        function toggle(): string {
            We10xState.closeAllFlyouts()
            Quickshell.execDetached(["niri", "msg", "action", "toggle-overview"])
            return "ok"
        }
    }

    IpcHandler {
        target: "showDesktop"
        function toggle(): string {
            We10xState.closeAllFlyouts()
            Quickshell.execDetached(["solar-shell", "show-desktop"])
            return "ok"
        }
    }

    IpcHandler {
        target: "aeroSnap"
        function showLeft(): string {
            We10xState.aeroSnapZone = "left"
            return "ok"
        }
        function showRight(): string {
            We10xState.aeroSnapZone = "right"
            return "ok"
        }
        function showTop(): string {
            We10xState.aeroSnapZone = "top"
            return "ok"
        }
        function hide(): string {
            We10xState.aeroSnapZone = ""
            return "ok"
        }
    }

    // ── Power & Settings IPC ───────────────────────────────────────────────
    IpcHandler {
        target: "powerDialog"
        function toggle(): string {
            We10xState.togglePowerDialog()
            return "ok"
        }
    }

    IpcHandler {
        target: "settings"
        function toggle(): string {
            We10xState.launchCommand(["solar-shell", "settings"])
            return "ok"
        }
    }

    // ── Active Wayland Desktop Surfaces ────────────────────────────────────
    DesktopIcons {}

    Taskbar {
        notificationServer: notifServer
    }

    component LatchedLazyLoader: LazyLoader {
        property bool latched: false
        required property bool wantActive
        onWantActiveChanged: if (wantActive) latched = true
        active: latched || wantActive
    }

    LatchedLazyLoader {
        wantActive: We10xState.startMenuOpen
        component: StartMenu {}
    }

    LatchedLazyLoader {
        wantActive: We10xState.actionCenterOpen
        component: ActionCenter {
            notificationServer: notifServer
        }
    }

    LatchedLazyLoader {
        wantActive: We10xState.calendarOpen
        component: CalendarFlyout {}
    }

    LatchedLazyLoader {
        wantActive: We10xState.altTabOpen
        component: AltTabSwitcher {}
    }

    LatchedLazyLoader {
        wantActive: We10xState.aeroSnapZone !== ""
        component: AeroSnapOverlay {}
    }

    LatchedLazyLoader {
        wantActive: We10xState.powerDialogOpen
        component: PowerDialog {}
    }
}
