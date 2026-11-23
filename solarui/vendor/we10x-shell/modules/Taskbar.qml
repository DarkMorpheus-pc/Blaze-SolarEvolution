import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland
import Quickshell.Services.Pipewire
import Quickshell.Services.SystemTray
import Quickshell.Services.UPower

Scope {
    id: taskbarScope

    required property var notificationServer

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

    readonly property bool isMuted: {
        if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
            return Pipewire.defaultAudioSink.audio.muted
        }
        return false
    }

    // Hover preview & right-click context menu state (rendered in unclipped overlay above 44px bar)
    property var hoveredToplevel: null
    property string hoveredTitle: ""
    property string hoveredIcon: ""
    property bool hoveredIsMinimized: false
    property real hoverCenterX: 200

    property bool taskMenuOpen: false
    property var taskMenuToplevel: null
    property string taskMenuAppId: ""
    property string taskMenuTitle: ""
    property var taskMenuCmd: []
    property real taskMenuX: 200

    // Unified task list: pinned apps (matched to running Toplevels if open) + unpinned running Toplevels
    readonly property var combinedTasks: {
        var result = []
        var usedIndices = {}
        var wins = ToplevelManager.toplevels.values

        for (var pIdx = 0; pIdx < We10xState.pinnedApps.length; pIdx++) {
            var pin = We10xState.pinnedApps[pIdx]
            var matched = []
            for (var wIdx = 0; wIdx < wins.length; wIdx++) {
                var w = wins[wIdx]
                var aid = (w.appId || "").toLowerCase()
                if (aid.length === 0) continue
                var hit = (aid === pin.id.toLowerCase())
                for (var m = 0; m < pin.matchIds.length; m++) {
                    if (aid.indexOf(pin.matchIds[m]) >= 0) {
                        hit = true
                        break
                    }
                }
                if (hit) {
                    matched.push(w)
                    usedIndices[wIdx] = true
                }
            }

            if (matched.length === 0) {
                result.push({
                    isPinnedOnly: true,
                    appId: pin.id,
                    title: pin.name,
                    iconUrl: We10xState.resolveToplevelIcon(pin.id),
                    cmd: pin.cmd,
                    toplevel: null,
                    isActive: false,
                    isMinimized: false
                })
            } else {
                for (var k = 0; k < matched.length; k++) {
                    var mw = matched[k]
                    result.push({
                        isPinnedOnly: false,
                        appId: mw.appId || pin.id,
                        title: mw.title || pin.name,
                        iconUrl: We10xState.resolveToplevelIcon(mw.appId || pin.id),
                        cmd: pin.cmd,
                        toplevel: mw,
                        isActive: mw.activated && !mw.minimized,
                        isMinimized: mw.minimized
                    })
                }
            }
        }

        for (var u = 0; u < wins.length; u++) {
            if (usedIndices[u]) continue
            var uw = wins[u]
            var uAid = uw.appId || ""
            if (uAid === "org.blazeos.setup" || uAid === "solar-gamezone") continue
            result.push({
                isPinnedOnly: false,
                appId: uAid,
                title: uw.title || uAid || "Uygulama",
                iconUrl: We10xState.resolveToplevelIcon(uAid),
                cmd: uAid.length > 0 ? [uAid] : [],
                toplevel: uw,
                isActive: uw.activated && !uw.minimized,
                isMinimized: uw.minimized
            })
        }
        return result
    }

    SystemClock {
        id: barClock
        precision: SystemClock.Minutes
    }

    PanelWindow {
        id: barWindow

        anchors {
            left: true
            right: true
            bottom: true
        }
        implicitHeight: 44
        exclusiveZone: 44
        color: We10xState.taskbarBg

        WlrLayershell.layer: WlrLayer.Top
        WlrLayershell.namespace: "we10x-taskbar"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

        // 1px top border
        Rectangle {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.right: parent.right
            height: 1
            color: We10xState.taskbarBorder
        }

        RowLayout {
            anchors.fill: parent
            spacing: 0

            // ── 1. We10X / Blaze Start Button (48x44) ──────────────────────
            Rectangle {
                id: startBtn
                Layout.preferredWidth: 48
                Layout.fillHeight: true
                color: We10xState.startMenuOpen
                    ? We10xState.activeTaskBg
                    : (startMouse.containsMouse ? We10xState.hoverBg : "transparent")

                Image {
                    width: 20
                    height: 20
                    sourceSize.width: 40
                    sourceSize.height: 40
                    anchors.centerIn: parent
                    source: We10xState.startIconUrl
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                }

                // Windows 10 #0078D7 accent tint bar on Start hover
                Rectangle {
                    visible: startMouse.containsMouse || We10xState.startMenuOpen
                    anchors.bottom: parent.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 2
                    color: We10xState.accent
                }

                MouseArea {
                    id: startMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        taskbarScope.taskMenuOpen = false
                        We10xState.toggleStartMenu(false)
                    }
                }
            }

            // ── 2. Embedded Windows 10 Search Box (232x44) ─────────────────
            Item {
                Layout.preferredWidth: 232
                Layout.fillHeight: true

                Rectangle {
                    anchors.fill: parent
                    anchors.topMargin: 5
                    anchors.bottomMargin: 5
                    anchors.rightMargin: 4
                    color: We10xState.searchBoxBg
                    border.width: (searchMouse.containsMouse || We10xState.startMenuOpen) ? 2 : 1
                    border.color: We10xState.startMenuOpen
                        ? We10xState.accent
                        : (searchMouse.containsMouse ? We10xState.accent : We10xState.searchBoxBorder)
                    radius: 0

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 10
                        spacing: 8

                        // Crisp vector magnifying glass
                        Canvas {
                            id: searchGlassCanvas
                            Layout.preferredWidth: 15
                            Layout.preferredHeight: 15
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.reset()
                                ctx.strokeStyle = We10xState.isLight ? "#333333" : "#D6D6D6"
                                ctx.lineWidth = 1.6
                                ctx.beginPath()
                                ctx.arc(6, 6, 4.6, 0, Math.PI * 2)
                                ctx.stroke()
                                ctx.beginPath()
                                ctx.moveTo(9.5, 9.5)
                                ctx.lineTo(14, 14)
                                ctx.stroke()
                            }
                            Connections {
                                target: We10xState
                                function onIsLightChanged() { searchGlassCanvas.requestPaint() }
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            text: We10xState.startSearchQuery.length > 0
                                ? We10xState.startSearchQuery
                                : "Aramak için buraya yazın"
                            color: We10xState.startSearchQuery.length > 0
                                ? We10xState.textPrimary
                                : We10xState.textSecondary
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }
                    }

                    MouseArea {
                        id: searchMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            taskbarScope.taskMenuOpen = false
                            We10xState.toggleStartMenu(true)
                        }
                    }
                }
            }

            // ── 3. Task View Button (40x44) ────────────────────────────────
            Rectangle {
                Layout.preferredWidth: 40
                Layout.fillHeight: true
                color: taskViewMouse.containsMouse ? We10xState.hoverBg : "transparent"

                Canvas {
                    id: taskViewCanvas
                    width: 18
                    height: 18
                    anchors.centerIn: parent
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.reset()
                        ctx.strokeStyle = We10xState.isLight ? "#222222" : "#FFFFFF"
                        ctx.lineWidth = 1.4
                        ctx.strokeRect(1.5, 4.5, 10, 9)
                        ctx.beginPath()
                        ctx.moveTo(5, 2.5)
                        ctx.lineTo(15.5, 2.5)
                        ctx.lineTo(15.5, 12)
                        ctx.stroke()
                        ctx.beginPath()
                        ctx.moveTo(1.5, 16)
                        ctx.lineTo(15.5, 16)
                        ctx.stroke()
                    }
                    Connections {
                        target: We10xState
                        function onIsLightChanged() { taskViewCanvas.requestPaint() }
                    }
                }

                MouseArea {
                    id: taskViewMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        We10xState.closeAllFlyouts()
                        Quickshell.execDetached(["niri", "msg", "action", "toggle-overview"])
                    }
                }
            }

            // ── 4. Pinned & Open Window Task Buttons ───────────────────────
            Row {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 2
                clip: true

                Repeater {
                    model: taskbarScope.combinedTasks

                    delegate: Rectangle {
                        id: taskBtn
                        width: 46
                        height: 44
                        color: modelData.isActive
                            ? We10xState.activeTaskBg
                            : (taskBtnMouse.containsMouse ? We10xState.hoverBg : "transparent")

                        Image {
                            width: 22
                            height: 22
                            sourceSize.width: 44
                            sourceSize.height: 44
                            anchors.centerIn: parent
                            source: modelData.iconUrl
                            fillMode: Image.PreserveAspectFit
                            opacity: modelData.isMinimized ? 0.72 : 1.0
                            smooth: true
                        }

                        // Windows 10 bottom #0078D7 indicator:
                        // - Active window: full 44px width
                        // - Open background / minimized window: short centered 20px width
                        // - Pinned closed app: hidden (expands subtly on hover)
                        Rectangle {
                            visible: !modelData.isPinnedOnly
                            height: 2.5
                            width: modelData.isActive
                                ? (parent.width - 2)
                                : (taskBtnMouse.containsMouse ? 28 : 20)
                            anchors.bottom: parent.bottom
                            anchors.horizontalCenter: parent.horizontalCenter
                            color: We10xState.accent
                            Behavior on width { NumberAnimation { duration: 110; easing.type: Easing.OutQuad } }
                        }

                        MouseArea {
                            id: taskBtnMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton | Qt.RightButton

                            onEntered: {
                                if (!modelData.isPinnedOnly && modelData.toplevel) {
                                    var pos = taskBtn.mapToItem(null, taskBtn.width / 2, 0)
                                    taskbarScope.hoverCenterX = pos.x
                                    taskbarScope.hoveredToplevel = modelData.toplevel
                                    taskbarScope.hoveredTitle = modelData.title
                                    taskbarScope.hoveredIcon = modelData.iconUrl
                                    taskbarScope.hoveredIsMinimized = modelData.isMinimized
                                }
                            }

                            onExited: {
                                hoverClearTimer.restart()
                            }

                            onClicked: (mouse) => {
                                if (mouse.button === Qt.RightButton) {
                                    var pos = taskBtn.mapToItem(null, 0, 0)
                                    taskbarScope.taskMenuX = pos.x
                                    taskbarScope.taskMenuToplevel = modelData.toplevel
                                    taskbarScope.taskMenuAppId = modelData.appId
                                    taskbarScope.taskMenuTitle = modelData.title
                                    taskbarScope.taskMenuCmd = modelData.cmd
                                    taskbarScope.taskMenuOpen = true
                                    taskbarScope.hoveredToplevel = null
                                    return
                                }

                                taskbarScope.taskMenuOpen = false
                                We10xState.closeAllFlyouts()

                                if (modelData.isPinnedOnly || !modelData.toplevel) {
                                    if (modelData.cmd && modelData.cmd.length > 0) {
                                        Quickshell.execDetached(modelData.cmd)
                                    }
                                    return
                                }

                                var tl = modelData.toplevel
                                if (tl.minimized) {
                                    tl.minimized = false
                                    tl.activate()
                                } else if (tl.activated) {
                                    tl.minimized = true
                                } else {
                                    tl.activate()
                                }
                            }
                        }
                    }
                }
            }

            // ── 5. System Tray (StatusNotifierItems) ───────────────────────
            Row {
                Layout.fillHeight: true
                spacing: 0

                Repeater {
                    model: SystemTray.items.values

                    delegate: Rectangle {
                        id: sniBtn
                        width: 28
                        height: 44
                        color: sniMouse.containsMouse ? We10xState.hoverBg : "transparent"

                        Image {
                            width: 16
                            height: 16
                            sourceSize.width: 32
                            sourceSize.height: 32
                            anchors.centerIn: parent
                            source: We10xState.resolveIcon(modelData.icon, "assets/icons/computer.svg")
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                        }

                        MouseArea {
                            id: sniMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton | Qt.RightButton
                            onClicked: (mouse) => {
                                if (mouse.button === Qt.RightButton && modelData.hasMenu) {
                                    var pt = sniBtn.mapToItem(null, 0, 0)
                                    modelData.display(barWindow, Math.round(pt.x), 0)
                                } else if (modelData.onlyMenu && modelData.hasMenu) {
                                    var pt2 = sniBtn.mapToItem(null, 0, 0)
                                    modelData.display(barWindow, Math.round(pt2.x), 0)
                                } else {
                                    modelData.activate()
                                }
                            }
                        }
                    }
                }
            }

            // ── 6. Network Indicator ───────────────────────────────────────
            Rectangle {
                Layout.preferredWidth: 30
                Layout.fillHeight: true
                color: netMouse.containsMouse ? We10xState.hoverBg : "transparent"

                Canvas {
                    id: netCanvas
                    width: 16
                    height: 16
                    anchors.centerIn: parent
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.reset()
                        ctx.strokeStyle = We10xState.isLight ? "#1A1A1A" : "#FFFFFF"
                        ctx.fillStyle = ctx.strokeStyle
                        ctx.lineWidth = 1.4
                        ctx.beginPath()
                        ctx.arc(8, 12.5, 1.4, 0, Math.PI * 2)
                        ctx.fill()
                        ctx.beginPath()
                        ctx.arc(8, 12.5, 4.5, Math.PI * 1.22, Math.PI * 1.78)
                        ctx.stroke()
                        ctx.beginPath()
                        ctx.arc(8, 12.5, 8.0, Math.PI * 1.2, Math.PI * 1.8)
                        ctx.stroke()
                    }
                    Connections {
                        target: We10xState
                        function onIsLightChanged() { netCanvas.requestPaint() }
                    }
                }

                MouseArea {
                    id: netMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: We10xState.toggleActionCenter()
                }
            }

            // ── 7. PipeWire Audio Indicator ────────────────────────────────
            Rectangle {
                Layout.preferredWidth: 32
                Layout.fillHeight: true
                color: (We10xState.volumePopupOpen || volMouse.containsMouse) ? We10xState.hoverBg : "transparent"

                Canvas {
                    id: speakerCanvas
                    width: 16
                    height: 16
                    anchors.centerIn: parent
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.reset()
                        var c = We10xState.isLight ? "#1A1A1A" : "#FFFFFF"
                        ctx.strokeStyle = c
                        ctx.fillStyle = c
                        ctx.lineWidth = 1.4
                        ctx.beginPath()
                        ctx.moveTo(2, 6)
                        ctx.lineTo(5.5, 6)
                        ctx.lineTo(9, 3)
                        ctx.lineTo(9, 13)
                        ctx.lineTo(5.5, 10)
                        ctx.lineTo(2, 10)
                        ctx.closePath()
                        ctx.fill()
                        if (taskbarScope.isMuted || taskbarScope.currentVolume <= 0.01) {
                            ctx.beginPath()
                            ctx.moveTo(11, 5.5)
                            ctx.lineTo(15, 10.5)
                            ctx.moveTo(15, 5.5)
                            ctx.lineTo(11, 10.5)
                            ctx.stroke()
                        } else {
                            ctx.beginPath()
                            ctx.arc(9.5, 8, 2.8, -0.7, 0.7)
                            ctx.stroke()
                            if (taskbarScope.currentVolume > 0.45) {
                                ctx.beginPath()
                                ctx.arc(9.5, 8, 5.2, -0.75, 0.75)
                                ctx.stroke()
                            }
                        }
                    }
                    Connections {
                        target: taskbarScope
                        function onCurrentVolumeChanged() { speakerCanvas.requestPaint() }
                        function onIsMutedChanged() { speakerCanvas.requestPaint() }
                    }
                    Connections {
                        target: We10xState
                        function onIsLightChanged() { speakerCanvas.requestPaint() }
                    }
                }

                MouseArea {
                    id: volMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.MiddleButton
                    onClicked: (mouse) => {
                        if (mouse.button === Qt.MiddleButton) {
                            if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                Pipewire.defaultAudioSink.audio.muted = !Pipewire.defaultAudioSink.audio.muted
                            }
                        } else {
                            taskbarScope.taskMenuOpen = false
                            We10xState.toggleVolumePopup()
                        }
                    }
                    onWheel: (wheel) => {
                        if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                            var step = wheel.angleDelta.y > 0 ? 0.05 : -0.05
                            Pipewire.defaultAudioSink.audio.volume = Math.max(0.0, Math.min(1.0, taskbarScope.currentVolume + step))
                        }
                    }
                }
            }

            // ── 8. Battery Indicator (if present) ──────────────────────────
            Rectangle {
                visible: UPower.displayDevice && UPower.displayDevice.isPresent
                Layout.preferredWidth: 46
                Layout.fillHeight: true
                color: batMouse.containsMouse ? We10xState.hoverBg : "transparent"

                Text {
                    anchors.centerIn: parent
                    text: UPower.displayDevice
                        ? (Math.round(UPower.displayDevice.percentage * 100) + "%")
                        : ""
                    color: We10xState.textPrimary
                    font.pixelSize: 11
                }

                MouseArea {
                    id: batMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: We10xState.toggleActionCenter()
                }
            }

            // ── 9. Keyboard Layout Indicator (TR / ENG) ────────────────────
            Rectangle {
                Layout.preferredWidth: 36
                Layout.fillHeight: true
                color: kbMouse.containsMouse ? We10xState.hoverBg : "transparent"

                Text {
                    anchors.centerIn: parent
                    text: We10xState.keyboardLayoutLabel
                    color: We10xState.textPrimary
                    font.pixelSize: 11
                    font.weight: Font.Medium
                }

                MouseArea {
                    id: kbMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: We10xState.switchNextKeyboardLayout()
                }
            }

            // ── 10. Two-Line Clock & Date (CalendarFlyout trigger) ─────────
            Rectangle {
                Layout.preferredWidth: 74
                Layout.fillHeight: true
                color: (We10xState.calendarOpen || clockMouse.containsMouse) ? We10xState.hoverBg : "transparent"

                Column {
                    anchors.centerIn: parent
                    spacing: 1

                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Qt.formatTime(barClock.date, "HH:mm")
                        color: We10xState.textPrimary
                        font.pixelSize: 12
                    }

                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        text: Qt.formatDate(barClock.date, "dd.MM.yyyy")
                        color: We10xState.textPrimary
                        font.pixelSize: 11
                    }
                }

                MouseArea {
                    id: clockMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        taskbarScope.taskMenuOpen = false
                        We10xState.toggleCalendar()
                    }
                }
            }

            // ── 11. Action Center Button + Unread Badge (42x44) ────────────
            Rectangle {
                id: acBtn
                Layout.preferredWidth: 42
                Layout.fillHeight: true
                color: (We10xState.actionCenterOpen || acMouse.containsMouse) ? We10xState.hoverBg : "transparent"

                readonly property int notifCount: taskbarScope.notificationServer
                    ? taskbarScope.notificationServer.trackedNotifications.values.length
                    : 0

                Canvas {
                    id: acIcon
                    width: 17
                    height: 17
                    anchors.centerIn: parent
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.reset()
                        ctx.strokeStyle = We10xState.isLight ? "#1A1A1A" : "#FFFFFF"
                        ctx.lineWidth = 1.4
                        ctx.beginPath()
                        ctx.moveTo(2, 2.5)
                        ctx.lineTo(15, 2.5)
                        ctx.lineTo(15, 11.5)
                        ctx.lineTo(8, 11.5)
                        ctx.lineTo(4.5, 15)
                        ctx.lineTo(4.5, 11.5)
                        ctx.lineTo(2, 11.5)
                        ctx.closePath()
                        ctx.stroke()
                    }
                    Connections {
                        target: We10xState
                        function onIsLightChanged() { acIcon.requestPaint() }
                    }
                }

                Rectangle {
                    visible: acBtn.notifCount > 0
                    width: 15
                    height: 15
                    radius: 8
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.rightMargin: 5
                    anchors.bottomMargin: 6
                    color: We10xState.accent

                    Text {
                        anchors.centerIn: parent
                        text: acBtn.notifCount > 9 ? "9+" : acBtn.notifCount.toString()
                        color: "#FFFFFF"
                        font.pixelSize: 9
                        font.bold: true
                    }
                }

                MouseArea {
                    id: acMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        taskbarScope.taskMenuOpen = false
                        We10xState.toggleActionCenter()
                    }
                }
            }

            // ── 12. Rightmost 5px Show Desktop Vertical Strip (Win+D) ──────
            Rectangle {
                Layout.preferredWidth: 6
                Layout.fillHeight: true
                color: showDeskMouse.containsMouse ? "#45FFFFFF" : "transparent"

                Rectangle {
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: 1
                    color: We10xState.isLight ? "#35000000" : "#35FFFFFF"
                }

                MouseArea {
                    id: showDeskMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    onClicked: {
                        We10xState.closeAllFlyouts()
                        Quickshell.execDetached(["solar-shell", "show-desktop"])
                    }
                }
            }
        }
    }

    Timer {
        id: hoverClearTimer
        interval: 220
        repeat: false
        onTriggered: {
            if (!previewMouse.containsMouse) {
                taskbarScope.hoveredToplevel = null
            }
        }
    }

    // ── Unclipped Taskbar Popups (Hover Preview Card, Task Context Menu, Volume Flyout) ──
    PanelWindow {
        id: taskbarPopupLayer
        visible: taskbarScope.hoveredToplevel !== null || taskbarScope.taskMenuOpen || We10xState.volumePopupOpen
        color: "transparent"

        anchors {
            left: true
            right: true
            bottom: true
        }
        margins.bottom: 0
        implicitHeight: 160
        exclusiveZone: 0

        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.namespace: "we10x-taskbar-popup"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        mask: Region {
            item: (taskbarScope.taskMenuOpen || We10xState.volumePopupOpen)
                ? fullPopupBackdrop
                : previewCard
        }

        MouseArea {
            id: fullPopupBackdrop
            enabled: taskbarScope.taskMenuOpen || We10xState.volumePopupOpen
            anchors.fill: parent
            onClicked: {
                taskbarScope.taskMenuOpen = false
                We10xState.volumePopupOpen = false
            }
        }

        // 1. Window Hover Preview Card
        Rectangle {
            id: previewCard
            visible: taskbarScope.hoveredToplevel !== null && !taskbarScope.taskMenuOpen && !We10xState.volumePopupOpen
            width: 220
            height: 56
            x: Math.max(8, Math.min(taskbarPopupLayer.width - width - 8, taskbarScope.hoverCenterX - width / 2))
            y: taskbarPopupLayer.height - height - 4
            color: We10xState.flyoutBg
            border.width: 1
            border.color: We10xState.flyoutBorder
            radius: 0

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 10
                anchors.rightMargin: 6
                spacing: 8

                Image {
                    Layout.preferredWidth: 20
                    Layout.preferredHeight: 20
                    sourceSize.width: 40
                    sourceSize.height: 40
                    source: taskbarScope.hoveredIcon
                    fillMode: Image.PreserveAspectFit
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 1

                    Text {
                        Layout.fillWidth: true
                        text: taskbarScope.hoveredTitle
                        color: We10xState.textPrimary
                        font.pixelSize: 12
                        elide: Text.ElideRight
                    }

                    Text {
                        visible: taskbarScope.hoveredIsMinimized
                        text: "Küçültüldü — Geri yüklemek için tıklayın"
                        color: We10xState.textSecondary
                        font.pixelSize: 10
                    }
                }

                Rectangle {
                    Layout.preferredWidth: 28
                    Layout.preferredHeight: 28
                    color: previewCloseMouse.containsMouse ? We10xState.closeRed : "transparent"

                    Text {
                        anchors.centerIn: parent
                        text: "✕"
                        color: previewCloseMouse.containsMouse ? "#FFFFFF" : We10xState.textPrimary
                        font.pixelSize: 12
                    }

                    MouseArea {
                        id: previewCloseMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            if (taskbarScope.hoveredToplevel) {
                                taskbarScope.hoveredToplevel.close()
                                taskbarScope.hoveredToplevel = null
                            }
                        }
                    }
                }
            }

            MouseArea {
                id: previewMouse
                anchors.fill: parent
                anchors.rightMargin: 36
                hoverEnabled: true
                onExited: hoverClearTimer.restart()
                onClicked: {
                    if (taskbarScope.hoveredToplevel) {
                        taskbarScope.hoveredToplevel.minimized = false
                        taskbarScope.hoveredToplevel.activate()
                        taskbarScope.hoveredToplevel = null
                    }
                }
            }
        }

        // 2. Right-Click Task Context Menu
        Rectangle {
            id: taskCtxCard
            visible: taskbarScope.taskMenuOpen
            width: 224
            height: taskCtxCol.implicitHeight + 8
            x: Math.max(8, Math.min(taskbarPopupLayer.width - width - 8, taskbarScope.taskMenuX))
            y: taskbarPopupLayer.height - height - 2
            color: We10xState.flyoutBg
            border.width: 1
            border.color: We10xState.flyoutBorder
            radius: 0

            Column {
                id: taskCtxCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 4
                spacing: 2

                // Yeni Pencere Aç
                Rectangle {
                    width: taskCtxCol.width
                    height: 30
                    color: newWinMouse.containsMouse ? We10xState.hoverBg : "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        text: "Yeni Pencere Aç"
                        color: We10xState.textPrimary
                        font.pixelSize: 12
                    }
                    MouseArea {
                        id: newWinMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            taskbarScope.taskMenuOpen = false
                            if (taskbarScope.taskMenuCmd && taskbarScope.taskMenuCmd.length > 0) {
                                Quickshell.execDetached(taskbarScope.taskMenuCmd)
                            }
                        }
                    }
                }

                // Sabitle / Kaldır
                Rectangle {
                    width: taskCtxCol.width
                    height: 30
                    color: pinMouse.containsMouse ? We10xState.hoverBg : "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        text: We10xState.isPinned(taskbarScope.taskMenuAppId)
                            ? "Görev Çubuğundan Kaldır"
                            : "Görev Çubuğuna Sabitle"
                        color: We10xState.textPrimary
                        font.pixelSize: 12
                    }
                    MouseArea {
                        id: pinMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            taskbarScope.taskMenuOpen = false
                            We10xState.togglePinApp(taskbarScope.taskMenuAppId, taskbarScope.taskMenuTitle)
                        }
                    }
                }

                // Küçült / Geri Yükle
                Rectangle {
                    visible: taskbarScope.taskMenuToplevel !== null
                    width: taskCtxCol.width
                    height: 30
                    color: minToggleMouse.containsMouse ? We10xState.hoverBg : "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        text: (taskbarScope.taskMenuToplevel && taskbarScope.taskMenuToplevel.minimized)
                            ? "Pencereyi Geri Yükle"
                            : "Görev Çubuğuna Küçült"
                        color: We10xState.textPrimary
                        font.pixelSize: 12
                    }
                    MouseArea {
                        id: minToggleMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            taskbarScope.taskMenuOpen = false
                            if (taskbarScope.taskMenuToplevel) {
                                var nextMin = !taskbarScope.taskMenuToplevel.minimized
                                taskbarScope.taskMenuToplevel.minimized = nextMin
                                if (!nextMin) taskbarScope.taskMenuToplevel.activate()
                            }
                        }
                    }
                }

                // Pencereyi Kapat
                Rectangle {
                    visible: taskbarScope.taskMenuToplevel !== null
                    width: taskCtxCol.width
                    height: 30
                    color: closeItemMouse.containsMouse ? We10xState.closeRed : "transparent"
                    Text {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        text: "Pencereyi Kapat"
                        color: closeItemMouse.containsMouse ? "#FFFFFF" : We10xState.textPrimary
                        font.pixelSize: 12
                    }
                    MouseArea {
                        id: closeItemMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            taskbarScope.taskMenuOpen = false
                            if (taskbarScope.taskMenuToplevel) {
                                taskbarScope.taskMenuToplevel.close()
                            }
                        }
                    }
                }
            }
        }

        // 3. Windows 10 PipeWire Volume Flyout
        Rectangle {
            visible: We10xState.volumePopupOpen
            width: 320
            height: 88
            x: Math.max(8, taskbarPopupLayer.width - width - 54)
            y: taskbarPopupLayer.height - height
            color: We10xState.flyoutBg
            border.width: 1
            border.color: We10xState.flyoutBorder
            radius: 0

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 14
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: Pipewire.defaultAudioSink
                        ? (Pipewire.defaultAudioSink.description || "Hoparlör (PipeWire)")
                        : "Hoparlör (PipeWire)"
                    color: We10xState.textPrimary
                    font.pixelSize: 12
                    elide: Text.ElideRight
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 12

                    Rectangle {
                        Layout.preferredWidth: 28
                        Layout.preferredHeight: 28
                        color: muteBtnMouse.containsMouse ? We10xState.hoverBg : "transparent"
                        Text {
                            anchors.centerIn: parent
                            text: taskbarScope.isMuted ? "✕" : ")))"
                            color: taskbarScope.isMuted ? We10xState.closeRed : We10xState.textPrimary
                            font.pixelSize: 11
                            font.bold: true
                        }
                        MouseArea {
                            id: muteBtnMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: {
                                if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                    Pipewire.defaultAudioSink.audio.muted = !Pipewire.defaultAudioSink.audio.muted
                                }
                            }
                        }
                    }

                    Item {
                        id: volSliderArea
                        Layout.fillWidth: true
                        Layout.preferredHeight: 24

                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width
                            height: 3
                            color: We10xState.isLight ? "#BDBDBD" : "#555555"
                        }

                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: Math.round(parent.width * taskbarScope.currentVolume)
                            height: 3
                            color: We10xState.accent
                        }

                        Rectangle {
                            width: 8
                            height: 20
                            radius: 4
                            y: 2
                            x: Math.max(0, Math.min(parent.width - width, Math.round(parent.width * taskbarScope.currentVolume) - 4))
                            color: We10xState.accent
                        }

                        MouseArea {
                            anchors.fill: parent
                            onPressed: (mouse) => {
                                if (Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                    var ratio = Math.max(0, Math.min(1, mouse.x / width))
                                    Pipewire.defaultAudioSink.audio.muted = false
                                    Pipewire.defaultAudioSink.audio.volume = ratio
                                }
                            }
                            onPositionChanged: (mouse) => {
                                if (pressed && Pipewire.defaultAudioSink && Pipewire.defaultAudioSink.audio) {
                                    var ratio = Math.max(0, Math.min(1, mouse.x / width))
                                    Pipewire.defaultAudioSink.audio.volume = ratio
                                }
                            }
                        }
                    }

                    Text {
                        Layout.preferredWidth: 36
                        horizontalAlignment: Text.AlignRight
                        text: taskbarScope.isMuted ? "0" : Math.round(taskbarScope.currentVolume * 100).toString()
                        color: We10xState.textPrimary
                        font.pixelSize: 16
                    }
                }
            }
        }
    }
}
