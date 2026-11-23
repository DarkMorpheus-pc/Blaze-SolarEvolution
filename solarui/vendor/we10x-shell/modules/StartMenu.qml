import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: startMenuWindow

    visible: We10xState.startMenuOpen
    color: "transparent"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusiveZone: 0

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-startmenu"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    property bool railExpanded: false
    property bool powerSubmenuOpen: false

    onVisibleChanged: {
        if (visible) {
            searchInput.forceActiveFocus()
            powerSubmenuOpen = false
            railExpanded = false
        }
    }

    Component.onCompleted: {
        if (visible) {
            searchInput.forceActiveFocus()
            powerSubmenuOpen = false
            railExpanded = false
        }
    }

    Connections {
        target: We10xState
        function onFocusSearchCounterChanged() {
            if (startMenuWindow.visible) {
                searchInput.forceActiveFocus()
            }
        }
    }

    readonly property string mathResult: We10xState.evaluateMath(We10xState.startSearchQuery)

    // Alphabetically sorted & grouped DesktopEntries
    readonly property var groupedApps: {
        var raw = DesktopEntries.applications.values
        var q = We10xState.startSearchQuery.trim().toLowerCase()
        var filtered = []
        for (var i = 0; i < raw.length; i++) {
            var app = raw[i]
            if (!app || app.noDisplay) continue
            var nm = app.name || ""
            if (nm.length === 0) continue
            if (q.length > 0) {
                var desc = (app.comment || "") + " " + (app.genericName || "") + " " + (app.id || "")
                if (nm.toLowerCase().indexOf(q) < 0 && desc.toLowerCase().indexOf(q) < 0) {
                    continue
                }
            }
            filtered.push(app)
        }

        filtered.sort(function(a, b) {
            return (a.name || "").localeCompare(b.name || "", "tr")
        })

        var rows = []
        var lastLetter = ""
        for (var j = 0; j < filtered.length; j++) {
            var entry = filtered[j]
            var ch = (entry.name || "#").charAt(0).toUpperCase()
            if (!/[A-ZÇĞİÖŞÜ]/.test(ch)) ch = "#"
            if (q.length === 0 && ch !== lastLetter) {
                rows.push({ isHeader: true, letter: ch, entry: null })
                lastLetter = ch
            }
            rows.push({ isHeader: false, letter: ch, entry: entry })
        }
        return rows
    }

    readonly property var productivityTiles: [
        { wide: true,  title: "FireHub Mağaza",     sub: "Portadora & Flathub", bg: String(We10xState.accent), icon: "firehub",  fallback: "assets/icons/blaze-start.svg", cmd: ["firehub"] },
        { wide: false, title: "BlazeHouse",          sub: "Sistem Merkezi",      bg: "#2D7D9A", icon: "preferences-desktop",     fallback: "assets/icons/computer.svg",    cmd: ["sh", "-c", "command -v blaze-house >/dev/null && exec blaze-house || exec solar-shell settings"] },
        { wide: false, title: "Web Tarayıcı",        sub: "İnternet",            bg: "#0063B1", icon: "web-browser",             fallback: "assets/icons/computer.svg",    cmd: ["sh", "-c", "command -v google-chrome-stable >/dev/null && exec google-chrome-stable || command -v firefox >/dev/null && exec firefox || exec xdg-open https://www.google.com"] },
        { wide: false, title: "Dosya Gezgini",       sub: "Bu Bilgisayar",       bg: "#CA5010", icon: "",                        fallback: "assets/icons/folder.svg",      cmd: ["nautilus"] },
        { wide: false, title: "SolarUI Ayarları",    sub: "Masaüstü & Kabuk",    bg: "#515C6B", icon: "preferences-system",      fallback: "assets/icons/computer.svg",    cmd: ["solar-shell", "settings"] }
    ]

    readonly property var exploreTiles: [
        { wide: true,  title: "Blaze GameZone",      sub: "Konsol Oyun Modu",    bg: "#107C10", icon: "input-gaming",            fallback: "assets/icons/blaze-start.svg", cmd: ["solar-shell", "gamezone"] },
        { wide: false, title: "Terminal",            sub: "Kitty Wayland",       bg: "#252526", icon: "utilities-terminal",      fallback: "assets/icons/computer.svg",    cmd: ["sh", "-c", "command -v kitty >/dev/null && exec kitty || exec alacritty"] },
        { wide: false, title: "BERE Kurtarma",       sub: "Zaman Makinesi",      bg: "#881798", icon: "system-reboot",           fallback: "assets/icons/drive-harddisk.svg", cmd: ["solar-shell", "recovery"] },
        { wide: false, title: "Noctalia'ya Dön",     sub: "Kabuk Değiştir",      bg: "#008272", icon: "preferences-desktop-theme", fallback: "assets/icons/blaze-start.svg", cmd: ["solar-shell", "switch", "noctalia"] }
    ]

    MouseArea {
        anchors.fill: parent
        onPressed: We10xState.closeAllFlyouts()
    }

    Rectangle {
        id: panelFrame
        anchors.left: parent.left
        anchors.bottom: parent.bottom
        width: Math.min(668, parent.width)
        height: Math.min(564, parent.height)
        color: We10xState.flyoutBg
        border.width: 1
        border.color: We10xState.flyoutBorder
        radius: 0
        clip: true

        Keys.onEscapePressed: {
            We10xState.closeAllFlyouts()
        }

        MouseArea {
            anchors.fill: parent
            onPressed: startMenuWindow.powerSubmenuOpen = false
        }

        // ── Column 2: Search Input + Alphabetical A-Z App List (268px) ─────
        ColumnLayout {
            id: appColumn
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            anchors.leftMargin: 48
            width: 268
            spacing: 0

                // Live Search Box
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 42
                    Layout.margins: 8
                    color: We10xState.searchBoxBg
                    border.width: searchInput.activeFocus ? 2 : 1
                    border.color: searchInput.activeFocus ? We10xState.accent : We10xState.searchBoxBorder

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 10
                        anchors.rightMargin: 8
                        spacing: 6

                        TextInput {
                            id: searchInput
                            Layout.fillWidth: true
                            color: We10xState.textPrimary
                            font.pixelSize: 13
                            selectByMouse: true
                            clip: true
                            text: We10xState.startSearchQuery
                            onTextEdited: {
                                We10xState.startSearchQuery = text
                            }
                            Keys.onEscapePressed: {
                                We10xState.closeAllFlyouts()
                            }
                            Keys.onReturnPressed: {
                                for (var i = 0; i < startMenuWindow.groupedApps.length; i++) {
                                    var row = startMenuWindow.groupedApps[i]
                                    if (!row.isHeader && row.entry) {
                                        We10xState.closeAllFlyouts()
                                        row.entry.execute()
                                        return
                                    }
                                }
                            }

                            Text {
                                visible: searchInput.text.length === 0
                                anchors.verticalCenter: parent.verticalCenter
                                text: "Uygulama, ayar veya hesaplama..."
                                color: We10xState.textSecondary
                                font.pixelSize: 12
                            }
                        }

                        Rectangle {
                            visible: searchInput.text.length > 0
                            Layout.preferredWidth: 22
                            Layout.preferredHeight: 22
                            color: clearSearchMouse.containsMouse ? We10xState.hoverBg : "transparent"
                            Text {
                                anchors.centerIn: parent
                                text: "✕"
                                color: We10xState.textSecondary
                                font.pixelSize: 11
                            }
                            MouseArea {
                                id: clearSearchMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: {
                                    searchInput.text = ""
                                    We10xState.startSearchQuery = ""
                                    searchInput.forceActiveFocus()
                                }
                            }
                        }
                    }
                }

                // Instant Math Calculator Result Card
                Rectangle {
                    visible: startMenuWindow.mathResult.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: 54
                    Layout.leftMargin: 8
                    Layout.rightMargin: 8
                    Layout.bottomMargin: 6
                    color: We10xState.accentSubtle
                    border.width: 1
                    border.color: We10xState.accent

                    Column {
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.leftMargin: 12
                        spacing: 2

                        Text {
                            text: "= " + startMenuWindow.mathResult
                            color: We10xState.textPrimary
                            font.pixelSize: 18
                            font.bold: true
                        }
                        Text {
                            text: "Panoya kopyalamak için tıklayın"
                            color: We10xState.textSecondary
                            font.pixelSize: 10
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        onClicked: {
                            Quickshell.execDetached(["wl-copy", startMenuWindow.mathResult])
                        }
                    }
                }

                // Scrollable A-Z Application List
                ListView {
                    id: appListView
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    Layout.leftMargin: 4
                    Layout.rightMargin: 4
                    clip: true
                    boundsBehavior: Flickable.StopAtBounds
                    model: startMenuWindow.groupedApps

                    delegate: Item {
                        width: appListView.width
                        height: modelData.isHeader ? 28 : 36

                        Text {
                            visible: modelData.isHeader
                            anchors.left: parent.left
                            anchors.leftMargin: 12
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.letter
                            color: We10xState.accent
                            font.pixelSize: 12
                            font.bold: true
                        }

                        Rectangle {
                            visible: !modelData.isHeader
                            anchors.fill: parent
                            color: appRowMouse.containsMouse ? We10xState.hoverBg : "transparent"

                            RowLayout {
                                anchors.fill: parent
                                anchors.leftMargin: 10
                                anchors.rightMargin: 10
                                spacing: 10

                                Image {
                                    Layout.preferredWidth: 22
                                    Layout.preferredHeight: 22
                                    sourceSize.width: 44
                                    sourceSize.height: 44
                                    source: modelData.entry
                                        ? We10xState.resolveIcon(modelData.entry.icon, "assets/icons/computer.svg")
                                        : ""
                                    fillMode: Image.PreserveAspectFit
                                    smooth: true
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: modelData.entry ? modelData.entry.name : ""
                                    color: We10xState.textPrimary
                                    font.pixelSize: 12
                                    elide: Text.ElideRight
                                }
                            }

                            MouseArea {
                                id: appRowMouse
                                anchors.fill: parent
                                hoverEnabled: true
                                onClicked: {
                                    if (modelData.entry) {
                                        We10xState.closeAllFlyouts()
                                        modelData.entry.execute()
                                    }
                                }
                            }
                        }
                    }
                }
        }

        // Vertical divider between App List and Tile Grid
        Rectangle {
            x: 316
            width: 1
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.topMargin: 10
            anchors.bottomMargin: 10
            color: We10xState.isLight ? "#20000000" : "#20FFFFFF"
        }

        // ── Column 3: Windows 10 Live Tiles Area (351px) ───────────────────
        Flickable {
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            anchors.leftMargin: 317
            anchors.right: parent.right
            contentHeight: tilesColumn.implicitHeight + 24
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: tilesColumn
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 14
                spacing: 16

                    // Group 1: Üretkenlik ve Sistem
                    Column {
                        width: parent.width
                        spacing: 8

                        Text {
                            text: "Üretkenlik ve Sistem"
                            color: We10xState.textPrimary
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                        }

                        Flow {
                            width: parent.width
                            spacing: 6

                            Repeater {
                                model: startMenuWindow.productivityTiles
                                delegate: Rectangle {
                                    width: modelData.wide ? 210 : 102
                                    height: 98
                                    color: modelData.bg
                                    border.width: prodTileMouse.containsMouse ? 2 : 0
                                    border.color: "#A8FFFFFF"

                                    Image {
                                        width: 34
                                        height: 34
                                        sourceSize.width: 68
                                        sourceSize.height: 68
                                        anchors.centerIn: parent
                                        anchors.verticalCenterOffset: -10
                                        source: We10xState.resolveIcon(modelData.icon, modelData.fallback)
                                        fillMode: Image.PreserveAspectFit
                                        smooth: true
                                    }

                                    Column {
                                        anchors.left: parent.left
                                        anchors.right: parent.right
                                        anchors.bottom: parent.bottom
                                        anchors.margins: 8
                                        spacing: 1

                                        Text {
                                            width: parent.width
                                            text: modelData.title
                                            color: "#FFFFFF"
                                            font.pixelSize: 11
                                            font.weight: Font.DemiBold
                                            elide: Text.ElideRight
                                        }
                                        Text {
                                            visible: modelData.wide
                                            width: parent.width
                                            text: modelData.sub
                                            color: "#D9FFFFFF"
                                            font.pixelSize: 10
                                            elide: Text.ElideRight
                                        }
                                    }

                                    MouseArea {
                                        id: prodTileMouse
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        onClicked: We10xState.launchCommand(modelData.cmd)
                                    }
                                }
                            }
                        }
                    }

                    // Group 2: Keşfet ve Oyun
                    Column {
                        width: parent.width
                        spacing: 8

                        Text {
                            text: "Keşfet ve Oyun"
                            color: We10xState.textPrimary
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                        }

                        Flow {
                            width: parent.width
                            spacing: 6

                            Repeater {
                                model: startMenuWindow.exploreTiles
                                delegate: Rectangle {
                                    width: modelData.wide ? 210 : 102
                                    height: 98
                                    color: modelData.bg
                                    border.width: expTileMouse.containsMouse ? 2 : 0
                                    border.color: "#A8FFFFFF"

                                    Image {
                                        width: 34
                                        height: 34
                                        sourceSize.width: 68
                                        sourceSize.height: 68
                                        anchors.centerIn: parent
                                        anchors.verticalCenterOffset: -10
                                        source: We10xState.resolveIcon(modelData.icon, modelData.fallback)
                                        fillMode: Image.PreserveAspectFit
                                        smooth: true
                                    }

                                    Column {
                                        anchors.left: parent.left
                                        anchors.right: parent.right
                                        anchors.bottom: parent.bottom
                                        anchors.margins: 8
                                        spacing: 1

                                        Text {
                                            width: parent.width
                                            text: modelData.title
                                            color: "#FFFFFF"
                                            font.pixelSize: 11
                                            font.weight: Font.DemiBold
                                            elide: Text.ElideRight
                                        }
                                        Text {
                                            visible: modelData.wide
                                            width: parent.width
                                            text: modelData.sub
                                            color: "#D9FFFFFF"
                                            font.pixelSize: 10
                                            elide: Text.ElideRight
                                        }
                                    }

                                    MouseArea {
                                        id: expTileMouse
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        onClicked: We10xState.launchCommand(modelData.cmd)
                                    }
                                }
                            }
                        }
                    }
                }
            }

        // ── Column 1: Expandable 48px -> 208px Windows 10 Left Rail ────────
        Rectangle {
            id: leftRail
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            anchors.left: parent.left
            width: startMenuWindow.railExpanded ? 208 : 48
            color: startMenuWindow.railExpanded
                ? (We10xState.isLight ? "#EBEBEB" : "#171717")
                : (We10xState.isLight ? "#10000000" : "#14000000")
            border.width: startMenuWindow.railExpanded ? 1 : 0
            border.color: We10xState.flyoutBorder
            clip: true

            Behavior on width { NumberAnimation { duration: 130; easing.type: Easing.OutCubic } }

            HoverHandler {
                id: railHover
                onHoveredChanged: {
                    if (!hovered && !startMenuWindow.powerSubmenuOpen) {
                        startMenuWindow.railExpanded = false
                    }
                }
            }

            ColumnLayout {
                anchors.fill: parent
                spacing: 0

                // Top Hamburger Button
                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 48
                    color: burgerMouse.containsMouse ? We10xState.hoverBg : "transparent"

                    RowLayout {
                        anchors.fill: parent
                        spacing: 0

                        Item {
                            Layout.preferredWidth: 48
                            Layout.fillHeight: true
                            Text {
                                anchors.centerIn: parent
                                text: "☰"
                                color: We10xState.textPrimary
                                font.pixelSize: 16
                            }
                        }

                        Text {
                            visible: startMenuWindow.railExpanded
                            Layout.fillWidth: true
                            text: "BAŞLAT"
                            color: We10xState.textPrimary
                            font.pixelSize: 12
                            font.bold: true
                        }
                    }

                    MouseArea {
                        id: burgerMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: startMenuWindow.railExpanded = !startMenuWindow.railExpanded
                    }
                }

                Item { Layout.fillHeight: true }

                // Rail Shortcuts: User, Belgeler, İndirilenler, Dosya Gezgini, Ayarlar, Güç
                Repeater {
                    model: [
                        { key: "user",      label: We10xState.username },
                        { key: "docs",      label: "Belgeler" },
                        { key: "downloads", label: "İndirilenler" },
                        { key: "explorer",  label: "Dosya Gezgini" },
                        { key: "settings",  label: "Ayarlar" },
                        { key: "power",     label: "Güç" }
                    ]

                    delegate: Rectangle {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 44
                        color: railBtnMouse.containsMouse ? We10xState.hoverBg : "transparent"

                        RowLayout {
                            anchors.fill: parent
                            spacing: 0

                            Item {
                                Layout.preferredWidth: 48
                                Layout.fillHeight: true

                                Canvas {
                                    id: railGlyph
                                    width: 18
                                    height: 18
                                    anchors.centerIn: parent
                                    onPaint: {
                                        var ctx = getContext("2d")
                                        ctx.reset()
                                        var c = We10xState.isLight ? "#1A1A1A" : "#FFFFFF"
                                        ctx.strokeStyle = c
                                        ctx.fillStyle = c
                                        ctx.lineWidth = 1.35
                                        ctx.lineCap = "round"
                                        ctx.lineJoin = "round"
                                        var k = modelData.key
                                        if (k === "user") {
                                            ctx.beginPath()
                                            ctx.arc(9, 6, 3.2, 0, Math.PI * 2)
                                            ctx.stroke()
                                            ctx.beginPath()
                                            ctx.arc(9, 16.5, 5.6, Math.PI * 1.12, Math.PI * 1.88)
                                            ctx.stroke()
                                        } else if (k === "docs") {
                                            ctx.beginPath()
                                            ctx.moveTo(4, 2.5)
                                            ctx.lineTo(11, 2.5)
                                            ctx.lineTo(14, 5.5)
                                            ctx.lineTo(14, 15.5)
                                            ctx.lineTo(4, 15.5)
                                            ctx.closePath()
                                            ctx.stroke()
                                            ctx.beginPath()
                                            ctx.moveTo(10.8, 2.5)
                                            ctx.lineTo(10.8, 5.7)
                                            ctx.lineTo(14, 5.7)
                                            ctx.stroke()
                                        } else if (k === "downloads") {
                                            ctx.beginPath()
                                            ctx.moveTo(9, 2.5)
                                            ctx.lineTo(9, 11.5)
                                            ctx.moveTo(5.8, 8.5)
                                            ctx.lineTo(9, 11.8)
                                            ctx.lineTo(12.2, 8.5)
                                            ctx.moveTo(3.5, 13.5)
                                            ctx.lineTo(3.5, 15.5)
                                            ctx.lineTo(14.5, 15.5)
                                            ctx.lineTo(14.5, 13.5)
                                            ctx.stroke()
                                        } else if (k === "explorer") {
                                            ctx.beginPath()
                                            ctx.moveTo(2.5, 4.5)
                                            ctx.lineTo(7.2, 4.5)
                                            ctx.lineTo(8.8, 6.2)
                                            ctx.lineTo(15.5, 6.2)
                                            ctx.lineTo(15.5, 14.5)
                                            ctx.lineTo(2.5, 14.5)
                                            ctx.closePath()
                                            ctx.stroke()
                                        } else if (k === "settings") {
                                            ctx.beginPath()
                                            ctx.arc(9, 9, 2.4, 0, Math.PI * 2)
                                            ctx.stroke()
                                            ctx.beginPath()
                                            ctx.arc(9, 9, 5.2, 0, Math.PI * 2)
                                            ctx.stroke()
                                            for (var t = 0; t < 8; t++) {
                                                var ang = (t * Math.PI) / 4
                                                ctx.beginPath()
                                                ctx.moveTo(9 + Math.cos(ang) * 5.0, 9 + Math.sin(ang) * 5.0)
                                                ctx.lineTo(9 + Math.cos(ang) * 6.8, 9 + Math.sin(ang) * 6.8)
                                                ctx.stroke()
                                            }
                                        } else if (k === "power") {
                                            ctx.beginPath()
                                            ctx.moveTo(9, 2.5)
                                            ctx.lineTo(9, 8.5)
                                            ctx.stroke()
                                            ctx.beginPath()
                                            ctx.arc(9, 9.5, 5.4, -Math.PI * 0.3, Math.PI * 1.3)
                                            ctx.stroke()
                                        }
                                    }
                                    Connections {
                                        target: We10xState
                                        function onIsLightChanged() { railGlyph.requestPaint() }
                                    }
                                }
                            }

                            Text {
                                visible: startMenuWindow.railExpanded
                                Layout.fillWidth: true
                                text: modelData.label
                                color: We10xState.textPrimary
                                font.pixelSize: 12
                                elide: Text.ElideRight
                            }
                        }

                        MouseArea {
                            id: railBtnMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onEntered: startMenuWindow.railExpanded = true
                            onClicked: {
                                var k = modelData.key
                                if (k === "user") {
                                    We10xState.launchCommand(["nautilus", We10xState.homeDir])
                                } else if (k === "docs") {
                                    We10xState.launchCommand(["sh", "-c", "exec nautilus \"$(xdg-user-dir DOCUMENTS 2>/dev/null || echo \"$HOME\")\""])
                                } else if (k === "downloads") {
                                    We10xState.launchCommand(["sh", "-c", "exec nautilus \"$(xdg-user-dir DOWNLOAD 2>/dev/null || echo \"$HOME\")\""])
                                } else if (k === "explorer") {
                                    We10xState.launchCommand(["nautilus"])
                                } else if (k === "settings") {
                                    We10xState.launchCommand(["solar-shell", "settings"])
                                } else if (k === "power") {
                                    startMenuWindow.powerSubmenuOpen = !startMenuWindow.powerSubmenuOpen
                                }
                            }
                        }
                    }
                }
            }
        }

        // Power Submenu Popup (above Güç button)
        Rectangle {
            visible: startMenuWindow.powerSubmenuOpen
            x: 8
            y: panelFrame.height - height - 48
            width: 192
            height: powerMenuCol.implicitHeight + 8
            color: We10xState.flyoutSurface
            border.width: 1
            border.color: We10xState.flyoutBorder

            Column {
                id: powerMenuCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 4
                spacing: 2

                Repeater {
                    model: [
                        { label: "Kilitle",          cmd: ["solar-lock"] },
                        { label: "Uyku",             cmd: ["systemctl", "suspend"] },
                        { label: "Yeniden Başlat",   cmd: ["systemctl", "reboot"] },
                        { label: "Bilgisayarı Kapat", cmd: ["systemctl", "poweroff"] }
                    ]

                    delegate: Rectangle {
                        width: powerMenuCol.width
                        height: 32
                        color: pwrMouse.containsMouse ? We10xState.hoverBg : "transparent"

                        Text {
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.left: parent.left
                            anchors.leftMargin: 12
                            text: modelData.label
                            color: We10xState.textPrimary
                            font.pixelSize: 12
                        }

                        MouseArea {
                            id: pwrMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: {
                                startMenuWindow.powerSubmenuOpen = false
                                We10xState.launchCommand(modelData.cmd)
                            }
                        }
                    }
                }
            }
        }
    }
}
