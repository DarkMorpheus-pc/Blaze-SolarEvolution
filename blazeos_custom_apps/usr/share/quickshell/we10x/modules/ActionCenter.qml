import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Pipewire

PanelWindow {
    id: actionCenterWindow

    required property var notificationServer

    visible: We10xState.actionCenterOpen
    color: "transparent"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusiveZone: 0

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-actioncenter"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    PwObjectTracker {
        objects: Pipewire.defaultAudioSink ? [Pipewire.defaultAudioSink] : []
    }

    readonly property real currentVolume: {
        if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
            var v = Pipewire.defaultAudioSink.audio.volume
            return isNaN(v) ? 0.6 : Math.max(0, Math.min(1.0, v))
        }
        return 0.6
    }

    readonly property var quickTiles: [
        { key: "wifi",       title: "Wi-Fi",            active: We10xState.wifiEnabled,        glyph: "((o))" },
        { key: "bt",         title: "Bluetooth",        active: We10xState.bluetoothEnabled,   glyph: "B" },
        { key: "theme",      title: We10xState.isLight ? "Açık Tema" : "Koyu Tema", active: !We10xState.isLight, glyph: "◐" },
        { key: "night",      title: "Gece Işığı",       active: We10xState.nightLightEnabled,  glyph: "☾" },
        { key: "perf",       title: We10xState.perfMode === "ultra" ? "Güç: Performans" : "Güç: Dengeli", active: We10xState.perfMode === "ultra", glyph: "⚡" },
        { key: "network",    title: "Ağ Ayarları",      active: false,                         glyph: "⇄" },
        { key: "screenshot", title: "Ekran Alıntısı",   active: false,                         glyph: "[+]" },
        { key: "settings",   title: "Tüm Ayarlar",      active: false,                         glyph: "⚙" }
    ]

    MouseArea {
        anchors.fill: parent
        onPressed: We10xState.closeAllFlyouts()
    }

    Rectangle {
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.right: parent.right
        width: Math.min(384, parent.width)
        color: We10xState.flyoutBg
        border.width: 1
        border.color: We10xState.flyoutBorder
        radius: 0

        Keys.onEscapePressed: We10xState.closeAllFlyouts()

        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 12

            // ── Header + Clear All ─────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true

                Text {
                    Layout.fillWidth: true
                    text: "İŞLEM MERKEZİ"
                    color: We10xState.textPrimary
                    font.pixelSize: 12
                    font.bold: true
                    font.letterSpacing: 0.8
                }

                Rectangle {
                    visible: actionCenterWindow.notificationServer
                        && actionCenterWindow.notificationServer.trackedNotifications.values.length > 0
                    width: clearTxt.implicitWidth + 14
                    height: 24
                    color: clearAllMouse.containsMouse ? We10xState.hoverBg : "transparent"

                    Text {
                        id: clearTxt
                        anchors.centerIn: parent
                        text: "Tümünü Temizle"
                        color: We10xState.accent
                        font.pixelSize: 11
                    }

                    MouseArea {
                        id: clearAllMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            if (!actionCenterWindow.notificationServer) return
                            var list = actionCenterWindow.notificationServer.trackedNotifications.values.slice()
                            for (var i = 0; i < list.length; i++) {
                                if (list[i]) list[i].dismiss()
                            }
                        }
                    }
                }
            }

            // ── Live Notifications List ────────────────────────────────────
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true

                Text {
                    visible: !actionCenterWindow.notificationServer
                        || actionCenterWindow.notificationServer.trackedNotifications.values.length === 0
                    anchors.centerIn: parent
                    text: "Yeni bildirim yok"
                    color: We10xState.textSecondary
                    font.pixelSize: 13
                }

                ListView {
                    id: notifList
                    anchors.fill: parent
                    spacing: 8
                    clip: true
                    model: actionCenterWindow.notificationServer
                        ? actionCenterWindow.notificationServer.trackedNotifications.values
                        : []

                    delegate: Rectangle {
                        width: notifList.width
                        height: notifCol.implicitHeight + 20
                        color: We10xState.flyoutSurface
                        border.width: 1
                        border.color: We10xState.flyoutBorder

                        ColumnLayout {
                            id: notifCol
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            anchors.margins: 10
                            spacing: 4

                            RowLayout {
                                Layout.fillWidth: true

                                Text {
                                    Layout.fillWidth: true
                                    text: modelData.appName || "Sistem"
                                    color: We10xState.accent
                                    font.pixelSize: 11
                                    font.weight: Font.DemiBold
                                    elide: Text.ElideRight
                                }

                                Rectangle {
                                    Layout.preferredWidth: 20
                                    Layout.preferredHeight: 20
                                    color: dismissMouse.containsMouse ? We10xState.hoverBg : "transparent"
                                    Text {
                                        anchors.centerIn: parent
                                        text: "✕"
                                        color: We10xState.textSecondary
                                        font.pixelSize: 10
                                    }
                                    MouseArea {
                                        id: dismissMouse
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        onClicked: modelData.dismiss()
                                    }
                                }
                            }

                            Text {
                                Layout.fillWidth: true
                                text: modelData.summary || ""
                                color: We10xState.textPrimary
                                font.pixelSize: 12
                                font.bold: true
                                wrapMode: Text.Wrap
                                maximumLineCount: 2
                                elide: Text.ElideRight
                            }

                            Text {
                                visible: (modelData.body || "").length > 0
                                Layout.fillWidth: true
                                text: modelData.body || ""
                                color: We10xState.textSecondary
                                font.pixelSize: 11
                                wrapMode: Text.Wrap
                                maximumLineCount: 3
                                elide: Text.ElideRight
                            }
                        }
                    }
                }
            }

            // Divider
            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: We10xState.flyoutBorder
            }

            // ── 4x2 Windows 10 Quick Action Tiles ──────────────────────────
            GridLayout {
                Layout.fillWidth: true
                columns: 4
                rowSpacing: 6
                columnSpacing: 6

                Repeater {
                    model: actionCenterWindow.quickTiles

                    delegate: Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 62
                        color: modelData.active
                            ? We10xState.accent
                            : (qaMouse.containsMouse ? We10xState.hoverBg : We10xState.tileBg)
                        border.width: qaMouse.containsMouse ? 1 : 0
                        border.color: modelData.active ? "#CCFFFFFF" : We10xState.flyoutBorder

                        Column {
                            anchors.fill: parent
                            anchors.margins: 6
                            spacing: 8

                            Text {
                                text: modelData.glyph
                                color: modelData.active ? "#FFFFFF" : We10xState.textPrimary
                                font.pixelSize: 13
                                font.bold: true
                            }

                            Text {
                                width: parent.width
                                text: modelData.title
                                color: modelData.active ? "#FFFFFF" : We10xState.textPrimary
                                font.pixelSize: 10
                                elide: Text.ElideRight
                            }
                        }

                        MouseArea {
                            id: qaMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: {
                                var k = modelData.key
                                if (k === "wifi") {
                                    We10xState.wifiEnabled = !We10xState.wifiEnabled
                                    Quickshell.execDetached(["nmcli", "radio", "wifi", We10xState.wifiEnabled ? "on" : "off"])
                                } else if (k === "bt") {
                                    We10xState.bluetoothEnabled = !We10xState.bluetoothEnabled
                                    Quickshell.execDetached(["rfkill", We10xState.bluetoothEnabled ? "unblock" : "block", "bluetooth"])
                                } else if (k === "theme") {
                                    var nextTheme = We10xState.isLight ? "dark" : "light"
                                    We10xState.themeMode = nextTheme
                                    Quickshell.execDetached(["solar-shell", "we10x-config", "set-theme", nextTheme])
                                } else if (k === "night") {
                                    We10xState.nightLightEnabled = !We10xState.nightLightEnabled
                                    if (We10xState.nightLightEnabled) {
                                        Quickshell.execDetached(["sh", "-c", "command -v wlsunset >/dev/null && exec wlsunset -t 4200 -T 6500"])
                                    } else {
                                        Quickshell.execDetached(["pkill", "-x", "wlsunset"])
                                    }
                                } else if (k === "perf") {
                                    var order = ["legacy", "balanced", "ultra"]
                                    var idx = (order.indexOf(We10xState.perfMode) + 1) % order.length
                                    We10xState.perfMode = order[idx]
                                    Quickshell.execDetached(["sh", "-c", "command -v powerprofilesctl >/dev/null && powerprofilesctl set " + (We10xState.perfMode === "ultra" ? "performance" : (We10xState.perfMode === "legacy" ? "power-saver" : "balanced"))])
                                } else if (k === "network") {
                                    We10xState.launchCommand(["sh", "-c", "command -v nm-connection-editor >/dev/null && exec nm-connection-editor || exec kitty -e nmtui"])
                                } else if (k === "screenshot") {
                                    We10xState.closeAllFlyouts()
                                    Quickshell.execDetached(["sh", "-c", "sleep 0.2 && IMG=$(mktemp --suffix=.png) && grim \"$IMG\" && wl-copy < \"$IMG\" && notify-send -a We10X 'Ekran Görüntüsü Panoya Kopyalandı' \"$IMG\""])
                                } else if (k === "settings") {
                                    We10xState.launchCommand(["solar-shell", "settings"])
                                }
                            }
                        }
                    }
                }
            }

            // ── Brightness Slider ──────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Text {
                    Layout.preferredWidth: 22
                    text: "☀"
                    color: We10xState.textPrimary
                    font.pixelSize: 14
                }

                Item {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 22

                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        height: 3
                        color: We10xState.isLight ? "#BDBDBD" : "#555555"
                    }

                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.round(parent.width * (We10xState.brightnessPercent / 100.0))
                        height: 3
                        color: We10xState.accent
                    }

                    Rectangle {
                        width: 8
                        height: 18
                        radius: 4
                        y: 2
                        x: Math.max(0, Math.min(parent.width - width, Math.round(parent.width * (We10xState.brightnessPercent / 100.0)) - 4))
                        color: We10xState.accent
                    }

                    MouseArea {
                        anchors.fill: parent
                        onPressed: (mouse) => {
                            var pct = Math.max(5, Math.min(100, Math.round((mouse.x / width) * 100)))
                            We10xState.brightnessPercent = pct
                            Quickshell.execDetached(["brightnessctl", "set", pct + "%"])
                        }
                        onPositionChanged: (mouse) => {
                            if (pressed) {
                                var pct = Math.max(5, Math.min(100, Math.round((mouse.x / width) * 100)))
                                We10xState.brightnessPercent = pct
                            }
                        }
                        onReleased: {
                            Quickshell.execDetached(["brightnessctl", "set", We10xState.brightnessPercent + "%"])
                        }
                    }
                }

                Text {
                    Layout.preferredWidth: 34
                    horizontalAlignment: Text.AlignRight
                    text: Math.round(We10xState.brightnessPercent) + "%"
                    color: We10xState.textSecondary
                    font.pixelSize: 11
                }
            }

            // ── Audio Volume Slider ────────────────────────────────────────
            RowLayout {
                Layout.fillWidth: true
                spacing: 10

                Text {
                    Layout.preferredWidth: 22
                    text: ")))"
                    color: We10xState.textPrimary
                    font.pixelSize: 11
                    font.bold: true
                }

                Item {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 22

                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width
                        height: 3
                        color: We10xState.isLight ? "#BDBDBD" : "#555555"
                    }

                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.round(parent.width * actionCenterWindow.currentVolume)
                        height: 3
                        color: We10xState.accent
                    }

                    Rectangle {
                        width: 8
                        height: 18
                        radius: 4
                        y: 2
                        x: Math.max(0, Math.min(parent.width - width, Math.round(parent.width * actionCenterWindow.currentVolume) - 4))
                        color: We10xState.accent
                    }

                    MouseArea {
                        anchors.fill: parent
                        onPressed: (mouse) => {
                            if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                Pipewire.defaultAudioSink.audio.muted = false
                                Pipewire.defaultAudioSink.audio.volume = Math.max(0, Math.min(1, mouse.x / width))
                            }
                        }
                        onPositionChanged: (mouse) => {
                            if (pressed && Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                Pipewire.defaultAudioSink.audio.volume = Math.max(0, Math.min(1, mouse.x / width))
                            }
                        }
                    }
                }

                Text {
                    Layout.preferredWidth: 34
                    horizontalAlignment: Text.AlignRight
                    text: Math.round(actionCenterWindow.currentVolume * 100) + "%"
                    color: We10xState.textSecondary
                    font.pixelSize: 11
                }
            }
        }
    }
}
