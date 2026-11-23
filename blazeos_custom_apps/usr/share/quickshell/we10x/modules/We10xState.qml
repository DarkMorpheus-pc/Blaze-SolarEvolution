pragma Singleton
import QtQuick
import Quickshell
import Quickshell.Io

Singleton {
    id: state

    property bool ready: false
    property string themeMode: "dark"
    property string startIcon: "we10x"
    property string accentColor: "#0078D7"
    property bool colorizeSurfaces: false
    property int taskbarOpacityPct: 95
    property string customWallpaper: ""
    property bool showDesktopIcons: true
    property bool isLiveIso: false
    property bool trashHasItems: false
    property string username: "blaze"
    property string homeDir: "/home/liveuser"

    // Desktop icon scale: 0.85 (Küçük), 1.0 (Orta), 1.25 (Büyük)
    property real desktopIconScale: 1.0
    property int desktopRefreshToken: 0

    // Flyout & overlay states
    property bool startMenuOpen: false
    property string startSearchQuery: ""
    property int focusSearchCounter: 0
    property bool actionCenterOpen: false
    property bool calendarOpen: false
    property bool volumePopupOpen: false
    property bool altTabOpen: false
    property bool powerDialogOpen: false
    property string aeroSnapZone: ""

    // Quick settings & indicators
    property string keyboardLayoutLabel: "TR"
    property bool wifiEnabled: true
    property bool bluetoothEnabled: true
    property bool nightLightEnabled: false
    property string perfMode: "balanced"
    property real brightnessPercent: 85

    // Pinned taskbar launchers (desktop IDs / app IDs)
    property var pinnedApps: [
        { id: "org.gnome.Nautilus", matchIds: ["org.gnome.nautilus", "nautilus"], name: "Dosya Gezgini", icon: "system-file-manager", cmd: ["nautilus"] },
        { id: "firehub", matchIds: ["firehub"], name: "FireHub Mağaza", icon: "system-software-install", cmd: ["firehub"] },
        { id: "browser", matchIds: ["google-chrome", "google-chrome-stable", "firefox", "org.mozilla.firefox", "chromium"], name: "Web Tarayıcı", icon: "web-browser", cmd: ["sh", "-c", "command -v google-chrome-stable >/dev/null && exec google-chrome-stable || command -v firefox >/dev/null && exec firefox || exec xdg-open https://www.google.com"] },
        { id: "kitty", matchIds: ["kitty", "alacritty", "org.alacritty", "foot"], name: "Terminal", icon: "utilities-terminal", cmd: ["sh", "-c", "command -v kitty >/dev/null && exec kitty || exec alacritty"] },
        { id: "solar-settings", matchIds: ["solar-settings"], name: "SolarUI Ayarları", icon: "preferences-system", cmd: ["solar-shell", "settings"] }
    ]

    readonly property bool isLight: themeMode === "light"
    readonly property bool isOled: themeMode === "oled" || themeMode === "black"
    readonly property real taskbarAlpha: Math.max(0.45, Math.min(1.0, taskbarOpacityPct / 100.0))

    // Authentic Windows 10 / We10XOS Fluent Color Palette (Dynamic User Accent & Surface Tinting)
    readonly property color accent: Qt.color(accentColor && accentColor.length === 7 ? accentColor : "#0078D7")
    readonly property color accentHover: Qt.lighter(accent, 1.15)
    readonly property color accentSubtle: Qt.rgba(accent.r, accent.g, accent.b, 0.22)
    readonly property color accentSelectionBg: Qt.rgba(accent.r, accent.g, accent.b, 0.26)
    readonly property color accentSelectionBorder: Qt.rgba(accent.r, accent.g, accent.b, 0.62)
    readonly property color closeRed: "#E81123"

    readonly property color taskbarBg: {
        if (colorizeSurfaces) {
            return Qt.rgba(accent.r * 0.52, accent.g * 0.52, accent.b * 0.52, taskbarAlpha)
        }
        if (isLight) {
            return Qt.rgba(0.902, 0.902, 0.902, taskbarAlpha)
        }
        if (isOled) {
            return Qt.rgba(0.03, 0.03, 0.035, taskbarAlpha)
        }
        return Qt.rgba(0.063, 0.063, 0.063, taskbarAlpha)
    }
    readonly property color taskbarBorder: colorizeSurfaces
        ? Qt.rgba(accent.r, accent.g, accent.b, 0.55)
        : (isLight ? "#28000000" : "#24FFFFFF")
    readonly property color searchBoxBg: isLight && !colorizeSurfaces ? "#FFFFFF" : (isOled ? "#18181B" : "#2B2B2B")
    readonly property color searchBoxBorder: isLight && !colorizeSurfaces ? "#B8B8B8" : "#4A4A4A"

    readonly property color flyoutBg: {
        if (colorizeSurfaces) {
            return Qt.rgba(accent.r * 0.36, accent.g * 0.36, accent.b * 0.36, Math.min(1.0, taskbarAlpha + 0.03))
        }
        if (isLight) return "#FAF2F2F2"
        if (isOled) return "#FA0D0D0F"
        return "#FA1F1F1F"
    }
    readonly property color flyoutSurface: isLight && !colorizeSurfaces ? "#FFFFFF" : (isOled ? "#19191D" : "#2B2B2B")
    readonly property color flyoutBorder: colorizeSurfaces
        ? Qt.rgba(accent.r, accent.g, accent.b, 0.50)
        : (isLight ? "#CCCCCC" : "#3B3B3B")

    readonly property color textPrimary: (isLight && !colorizeSurfaces) ? "#111111" : "#FFFFFF"
    readonly property color textSecondary: (isLight && !colorizeSurfaces) ? "#595959" : "#C4C4C8"
    readonly property color hoverBg: (isLight && !colorizeSurfaces) ? "#16000000" : "#1FFFFFFF"
    readonly property color pressedBg: (isLight && !colorizeSurfaces) ? "#24000000" : "#30FFFFFF"
    readonly property color activeTaskBg: (isLight && !colorizeSurfaces) ? "#22000000" : "#28FFFFFF"
    readonly property color tileBg: (isLight && !colorizeSurfaces) ? "#D9D9D9" : "#333333"

    readonly property string wallpaperPath: {
        if (customWallpaper && customWallpaper.length > 0) {
            return customWallpaper
        }
        if (isLight) {
            return "/usr/share/backgrounds/blazeos/we10x/We10XOS-light.png"
        }
        if (isOled) {
            return "/usr/share/backgrounds/blazeos/we10x/We10XOS-black.png"
        }
        return "/usr/share/backgrounds/blazeos/we10x/We10XOS-dark.png"
    }

    readonly property string startIconUrl: startIcon === "blaze"
        ? assetUrl("assets/icons/blaze-start.svg")
        : assetUrl("assets/icons/we10x-start.svg")

    function toFileUrl(p) {
        if (!p || p.length === 0) return ""
        if (p.startsWith("file://") || p.startsWith("image://") || p.startsWith("qrc:")) return p
        return "file://" + p
    }

    function closeAllFlyouts() {
        startMenuOpen = false
        actionCenterOpen = false
        calendarOpen = false
        volumePopupOpen = false
        altTabOpen = false
        powerDialogOpen = false
    }

    function toggleStartMenu(focusSearch) {
        var next = !startMenuOpen
        if (focusSearch) {
            next = true
        }
        actionCenterOpen = false
        calendarOpen = false
        volumePopupOpen = false
        altTabOpen = false
        powerDialogOpen = false
        if (!next) {
            startSearchQuery = ""
        }
        startMenuOpen = next
        if (next && focusSearch) {
            focusSearchCounter += 1
        }
    }

    function toggleActionCenter() {
        var next = !actionCenterOpen
        closeAllFlyouts()
        actionCenterOpen = next
    }

    function toggleCalendar() {
        var next = !calendarOpen
        closeAllFlyouts()
        calendarOpen = next
    }

    function toggleVolumePopup() {
        var next = !volumePopupOpen
        startMenuOpen = false
        actionCenterOpen = false
        calendarOpen = false
        altTabOpen = false
        volumePopupOpen = next
    }

    function toggleAltTab() {
        var next = !altTabOpen
        closeAllFlyouts()
        altTabOpen = next
    }

    function togglePowerDialog() {
        var next = !powerDialogOpen
        closeAllFlyouts()
        powerDialogOpen = next
    }

    function assetUrl(relPath) {
        return toFileUrl(Quickshell.shellPath(relPath))
    }

    function resolveIcon(iconName, fallbackRelAsset) {
        if (iconName && iconName.length > 0) {
            if (iconName.startsWith("/") || iconName.startsWith("file://") || iconName.startsWith("image://")) {
                return toFileUrl(iconName)
            }
            if (Quickshell.hasThemeIcon(iconName)) {
                return Quickshell.iconPath(iconName)
            }
        }
        if (fallbackRelAsset && fallbackRelAsset.length > 0) {
            return assetUrl(fallbackRelAsset)
        }
        if (Quickshell.hasThemeIcon("application-x-executable")) {
            return Quickshell.iconPath("application-x-executable")
        }
        return assetUrl("assets/icons/computer.svg")
    }

    function resolveToplevelIcon(appId) {
        if (!appId || appId.length === 0) {
            return resolveIcon("application-x-executable", "assets/icons/computer.svg")
        }
        var lower = appId.toLowerCase()
        if (lower.indexOf("nautilus") >= 0 || lower.indexOf("files") >= 0) {
            return assetUrl("assets/icons/folder.svg")
        }
        if (lower.indexOf("kitty") >= 0 || lower.indexOf("alacritty") >= 0 || lower.indexOf("terminal") >= 0 || lower.indexOf("ptyxis") >= 0) {
            return resolveIcon("utilities-terminal", "assets/icons/computer.svg")
        }
        if (lower.indexOf("firehub") >= 0) {
            return resolveIcon("firehub", "assets/icons/blaze-start.svg")
        }
        if (lower.indexOf("welcome") >= 0 || lower.indexOf("solarui") >= 0) {
            return resolveIcon("solarui", "assets/icons/blaze-start.svg")
        }
        if (lower.indexOf("gamezone") >= 0) {
            return resolveIcon("input-gaming", "assets/icons/blaze-start.svg")
        }
        if (lower.indexOf("recovery") >= 0) {
            return resolveIcon("system-reboot", "assets/icons/drive-harddisk.svg")
        }
        if (lower.indexOf("solar-settings") >= 0 || lower.indexOf("control") >= 0 || lower.indexOf("blaze-house") >= 0) {
            return resolveIcon("preferences-system", "assets/icons/computer.svg")
        }
        var entry = DesktopEntries.heuristicLookup(appId)
        if (entry && entry.icon && entry.icon.length > 0) {
            return resolveIcon(entry.icon, "assets/icons/computer.svg")
        }
        return resolveIcon(appId, "assets/icons/computer.svg")
    }

    function launchCommand(cmdArray) {
        closeAllFlyouts()
        Quickshell.execDetached(cmdArray)
    }

    function isPinned(appId) {
        if (!appId) return false
        var lower = appId.toLowerCase()
        for (var i = 0; i < pinnedApps.length; i++) {
            var p = pinnedApps[i]
            if (p.id.toLowerCase() === lower) return true
            for (var j = 0; j < p.matchIds.length; j++) {
                if (lower.indexOf(p.matchIds[j]) >= 0) return true
            }
        }
        return false
    }

    function togglePinApp(appId, title) {
        if (!appId || appId.length === 0) return
        var lower = appId.toLowerCase()
        var next = []
        var removed = false
        for (var i = 0; i < pinnedApps.length; i++) {
            var p = pinnedApps[i]
            var matches = (p.id.toLowerCase() === lower)
            for (var j = 0; j < p.matchIds.length; j++) {
                if (lower.indexOf(p.matchIds[j]) >= 0) matches = true
            }
            if (matches) {
                removed = true
            } else {
                next.push(p)
            }
        }
        if (!removed) {
            var entry = DesktopEntries.heuristicLookup(appId)
            var cmd = (entry && entry.command && entry.command.length > 0) ? entry.command : [appId]
            var icon = (entry && entry.icon && entry.icon.length > 0) ? entry.icon : appId
            var name = (entry && entry.name && entry.name.length > 0) ? entry.name : (title || appId)
            next.push({
                id: appId,
                matchIds: [lower],
                name: name,
                icon: icon,
                cmd: cmd
            })
        }
        pinnedApps = next
    }

    function evaluateMath(raw) {
        if (!raw) return ""
        var s = raw.trim().replace(/,/g, ".")
        if (s.length < 2) return ""
        if (!/^[0-9+\-*/().%\s]+$/.test(s)) return ""
        if (!/[+\-*/%]/.test(s)) return ""
        try {
            var fn = new Function("return (" + s + ");")
            var res = fn()
            if (typeof res === "number" && isFinite(res)) {
                return Number(res.toFixed(6)).toString()
            }
        } catch (e) {}
        return ""
    }

    function reloadConfig() {
        configReader.running = true
    }

    Process {
        id: configReader
        running: true
        command: ["solar-shell", "we10x-config", "get"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    var parsed = JSON.parse(text.trim())
                    if (parsed.theme_mode) state.themeMode = parsed.theme_mode
                    if (parsed.start_icon) state.startIcon = parsed.start_icon
                    if (parsed.accent_color) state.accentColor = parsed.accent_color
                    if (typeof parsed.colorize_surfaces === "boolean") state.colorizeSurfaces = parsed.colorize_surfaces
                    if (typeof parsed.taskbar_opacity_pct === "number") state.taskbarOpacityPct = parsed.taskbar_opacity_pct
                    if (typeof parsed.wallpaper === "string") state.customWallpaper = parsed.wallpaper
                    if (typeof parsed.desktop_icons === "boolean") state.showDesktopIcons = parsed.desktop_icons
                    if (typeof parsed.is_live === "boolean") state.isLiveIso = parsed.is_live
                    if (typeof parsed.trash_has_items === "boolean") state.trashHasItems = parsed.trash_has_items
                    if (parsed.username) state.username = parsed.username
                    if (parsed.home) state.homeDir = parsed.home
                } catch (e) {}
                state.ready = true
            }
        }
    }

    Process {
        id: layoutReader
        running: true
        command: ["niri", "msg", "--json", "keyboard-layouts"]
        stdout: StdioCollector {
            onStreamFinished: {
                try {
                    var parsed = JSON.parse(text.trim())
                    var names = parsed.names || []
                    var idx = (typeof parsed.current_idx === "number") ? parsed.current_idx : 0
                    var raw = String(names[idx] || "").trim().toLowerCase()
                    if (raw.indexOf("turk") >= 0 || raw === "tr") {
                        state.keyboardLayoutLabel = "TR"
                    } else if (raw.indexOf("eng") >= 0 || raw === "us") {
                        state.keyboardLayoutLabel = "ENG"
                    } else if (raw.indexOf("ger") >= 0 || raw === "de") {
                        state.keyboardLayoutLabel = "DE"
                    } else if (raw.indexOf("fre") >= 0 || raw === "fr") {
                        state.keyboardLayoutLabel = "FR"
                    } else if (raw.indexOf("rus") >= 0 || raw === "ru") {
                        state.keyboardLayoutLabel = "RU"
                    } else if (raw.length > 0) {
                        state.keyboardLayoutLabel = raw.substring(0, 3).toUpperCase()
                    }
                } catch (e) {}
            }
        }
    }

    Process {
        id: layoutEventStream
        running: true
        command: ["niri", "msg", "-j", "event-stream"]
        stdout: SplitParser {
            splitMarker: "\n"
            onRead: (line) => {
                if (line.indexOf("KeyboardLayout") !== -1) {
                    layoutRefreshDebounce.restart()
                }
            }
        }
        onExited: layoutStreamRestart.restart()
    }

    Timer {
        id: layoutStreamRestart
        interval: 1500
        repeat: false
        onTriggered: {
            if (!layoutEventStream.running) layoutEventStream.running = true
        }
    }

    Timer {
        id: layoutRefreshDebounce
        interval: 80
        repeat: false
        onTriggered: {
            if (!layoutReader.running) layoutReader.running = true
        }
    }

    Timer {
        interval: 60000
        running: state.showDesktopIcons
        repeat: true
        onTriggered: {
            if (!configReader.running) configReader.running = true
        }
    }

    function switchNextKeyboardLayout() {
        Quickshell.execDetached(["niri", "msg", "action", "switch-layout", "next"])
        layoutRefreshDelay.restart()
    }

    Timer {
        id: layoutRefreshDelay
        interval: 180
        repeat: false
        onTriggered: {
            if (!layoutReader.running) layoutReader.running = true
        }
    }
}
