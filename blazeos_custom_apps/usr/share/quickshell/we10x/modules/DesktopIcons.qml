import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: desktopRoot

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }

    color: We10xState.isLight ? "#D8E6F3" : "#001B36"
    WlrLayershell.layer: WlrLayer.Background
    WlrLayershell.namespace: "we10x-wallpaper"
    WlrLayershell.exclusiveZone: -1
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None

    property int selectedIconIndex: -1
    property bool contextMenuVisible: false
    property real contextMenuX: 0
    property real contextMenuY: 0

    readonly property var desktopEntriesModel: {
        var list = [
            {
                key: "computer",
                label: "Bu Bilgisayar",
                iconUrl: We10xState.assetUrl("assets/icons/computer.svg"),
                cmd: ["nautilus", "computer:///"]
            },
            {
                key: "home",
                label: We10xState.username,
                iconUrl: We10xState.assetUrl("assets/icons/user-home.svg"),
                cmd: ["nautilus", We10xState.homeDir]
            },
            {
                key: "trash",
                label: "Geri Dönüşüm Kutusu",
                iconUrl: We10xState.trashHasItems
                    ? We10xState.assetUrl("assets/icons/user-trash-full.svg")
                    : We10xState.assetUrl("assets/icons/user-trash.svg"),
                cmd: ["nautilus", "trash:///"]
            },
            {
                key: "firehub",
                label: "FireHub Mağaza",
                iconUrl: We10xState.resolveIcon("firehub", "assets/icons/blaze-start.svg"),
                cmd: ["firehub"]
            },
            {
                key: "settings",
                label: "SolarUI Ayarları",
                iconUrl: We10xState.resolveIcon("preferences-system", "assets/icons/computer.svg"),
                cmd: ["solar-shell", "settings"]
            }
        ]
        if (We10xState.isLiveIso) {
            list.push({
                key: "installer",
                label: "BlazeOS'u Kur",
                iconUrl: We10xState.resolveIcon("system-os-installer", "assets/icons/drive-harddisk.svg"),
                cmd: ["sh", "-c", "command -v liveinst >/dev/null && exec liveinst || exec solar-shell installer"]
            })
        }
        return list
    }

    Image {
        id: wallpaperImg
        anchors.fill: parent
        source: We10xState.toFileUrl(We10xState.wallpaperPath)
        fillMode: Image.PreserveAspectCrop
        sourceSize.width: desktopRoot.width > 0 ? desktopRoot.width : 1920
        sourceSize.height: desktopRoot.height > 0 ? desktopRoot.height : 1080
        cache: true
        asynchronous: true
        smooth: true
    }

    // Background click & right-click area
    MouseArea {
        id: desktopBgArea
        anchors.fill: parent
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onPressed: (mouse) => {
            We10xState.closeAllFlyouts()
            desktopRoot.selectedIconIndex = -1
            if (mouse.button === Qt.RightButton) {
                var maxX = Math.max(0, desktopRoot.width - ctxMenu.width - 8)
                var maxY = Math.max(0, desktopRoot.height - 44 - ctxMenu.height - 8)
                desktopRoot.contextMenuX = Math.min(mouse.x, maxX)
                desktopRoot.contextMenuY = Math.min(mouse.y, maxY)
                desktopRoot.contextMenuVisible = true
            } else {
                desktopRoot.contextMenuVisible = false
            }
        }
    }

    // Draggable & selectable classic Windows 10 desktop icon grid
    Item {
        id: iconCanvas
        anchors.fill: parent
        anchors.bottomMargin: 44
        visible: We10xState.showDesktopIcons

        Repeater {
            model: desktopRoot.desktopEntriesModel

            delegate: Item {
                id: iconItem
                readonly property real scaleFactor: We10xState.desktopIconScale
                width: Math.round(96 * scaleFactor)
                height: Math.round(96 * scaleFactor)

                property bool userMoved: false
                readonly property int defaultRow: index % 6
                readonly property int defaultCol: Math.floor(index / 6)

                x: userMoved ? x : (12 + defaultCol * (width + 8))
                y: userMoved ? y : (14 + defaultRow * (height + 8))

                Connections {
                    target: We10xState
                    function onDesktopRefreshTokenChanged() {
                        iconItem.userMoved = false
                        iconItem.x = 12 + iconItem.defaultCol * (iconItem.width + 8)
                        iconItem.y = 14 + iconItem.defaultRow * (iconItem.height + 8)
                    }
                }

                readonly property bool isSelected: desktopRoot.selectedIconIndex === index

                Rectangle {
                    anchors.fill: parent
                    radius: 0
                    color: iconItem.isSelected
                        ? We10xState.accentSelectionBg
                        : (iconMouse.containsMouse ? "#24FFFFFF" : "transparent")
                    border.width: (iconItem.isSelected || iconMouse.containsMouse) ? 1 : 0
                    border.color: iconItem.isSelected ? We10xState.accentSelectionBorder : "#40FFFFFF"
                }

                Image {
                    id: iconGraphic
                    width: Math.round(42 * iconItem.scaleFactor)
                    height: Math.round(42 * iconItem.scaleFactor)
                    sourceSize.width: width * 2
                    sourceSize.height: height * 2
                    anchors.top: parent.top
                    anchors.topMargin: Math.round(8 * iconItem.scaleFactor)
                    anchors.horizontalCenter: parent.horizontalCenter
                    source: modelData.iconUrl
                    fillMode: Image.PreserveAspectFit
                    smooth: true
                }

                // Crisp Windows 10 drop-shadow desktop label (exact width match prevents mismatched word-wrap)
                Text {
                    id: iconShadow
                    x: iconLabel.x + 1
                    y: iconLabel.y + 1
                    width: iconLabel.width
                    text: modelData.label
                    color: "#D9000000"
                    font.pixelSize: Math.round(12 * Math.min(1.1, iconItem.scaleFactor))
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }

                Text {
                    id: iconLabel
                    anchors.top: iconGraphic.bottom
                    anchors.topMargin: 5
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.leftMargin: 2
                    anchors.rightMargin: 2
                    text: modelData.label
                    color: "#FFFFFF"
                    font.pixelSize: Math.round(12 * Math.min(1.1, iconItem.scaleFactor))
                    horizontalAlignment: Text.AlignHCenter
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }

                MouseArea {
                    id: iconMouse
                    anchors.fill: parent
                    hoverEnabled: true
                    acceptedButtons: Qt.LeftButton | Qt.RightButton
                    drag.target: iconItem
                    drag.minimumX: 4
                    drag.minimumY: 4
                    drag.maximumX: Math.max(4, iconCanvas.width - iconItem.width - 4)
                    drag.maximumY: Math.max(4, iconCanvas.height - iconItem.height - 4)

                    onPressed: (mouse) => {
                        We10xState.closeAllFlyouts()
                        desktopRoot.contextMenuVisible = false
                        desktopRoot.selectedIconIndex = index
                        if (mouse.button === Qt.RightButton) {
                            desktopRoot.contextMenuX = Math.min(iconItem.x + mouse.x, desktopRoot.width - ctxMenu.width - 8)
                            desktopRoot.contextMenuY = Math.min(iconItem.y + mouse.y, desktopRoot.height - 44 - ctxMenu.height - 8)
                            desktopRoot.contextMenuVisible = true
                        }
                    }

                    onPositionChanged: {
                        if (drag.active) {
                            iconItem.userMoved = true
                        }
                    }

                    onDoubleClicked: (mouse) => {
                        if (mouse.button === Qt.LeftButton) {
                            desktopRoot.contextMenuVisible = false
                            We10xState.launchCommand(modelData.cmd)
                        }
                    }
                }
            }
        }
    }

    // Classic Windows 10 Desktop Right-Click Context Menu
    Rectangle {
        id: ctxMenu
        visible: desktopRoot.contextMenuVisible
        x: desktopRoot.contextMenuX
        y: desktopRoot.contextMenuY
        width: 248
        height: ctxCol.implicitHeight + 8
        color: We10xState.isLight ? "#F2F2F2" : "#2B2B2B"
        border.width: 1
        border.color: We10xState.isLight ? "#CCCCCC" : "#454545"
        radius: 0

        Column {
            id: ctxCol
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.margins: 4
            spacing: 1

            Repeater {
                model: [
                    { type: "item", label: "Görünüm: Büyük Simgeler", check: We10xState.desktopIconScale > 1.15, action: "scale_large" },
                    { type: "item", label: "Görünüm: Orta Simgeler", check: We10xState.desktopIconScale >= 0.95 && We10xState.desktopIconScale <= 1.15, action: "scale_medium" },
                    { type: "item", label: "Görünüm: Küçük Simgeler", check: We10xState.desktopIconScale < 0.95, action: "scale_small" },
                    { type: "sep" },
                    { type: "item", label: "Yenile", check: false, action: "refresh" },
                    { type: "item", label: "Yeni Klasör Oluştur", check: false, action: "new_folder" },
                    { type: "item", label: "Terminalde Aç", check: false, action: "open_terminal" },
                    { type: "sep" },
                    { type: "item", label: "Görüntü Ayarları", check: false, action: "display_settings" },
                    { type: "item", label: "Kişiselleştir", check: false, action: "personalize" }
                ]

                delegate: Item {
                    width: ctxCol.width
                    height: modelData.type === "sep" ? 9 : 28

                    Rectangle {
                        visible: modelData.type === "sep"
                        anchors.verticalCenter: parent.verticalCenter
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.leftMargin: 8
                        anchors.rightMargin: 8
                        height: 1
                        color: We10xState.isLight ? "#D5D5D5" : "#3E3E3E"
                    }

                    Rectangle {
                        visible: modelData.type === "item"
                        anchors.fill: parent
                        color: menuMouse.containsMouse ? We10xState.hoverBg : "transparent"

                        Text {
                            anchors.left: parent.left
                            anchors.leftMargin: 10
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.check ? "•" : ""
                            color: We10xState.accent
                            font.pixelSize: 14
                            font.bold: true
                        }

                        Text {
                            anchors.left: parent.left
                            anchors.leftMargin: 28
                            anchors.right: parent.right
                            anchors.rightMargin: 10
                            anchors.verticalCenter: parent.verticalCenter
                            text: modelData.label || ""
                            color: We10xState.textPrimary
                            font.pixelSize: 12
                            elide: Text.ElideRight
                        }

                        MouseArea {
                            id: menuMouse
                            anchors.fill: parent
                            hoverEnabled: true
                            onClicked: {
                                desktopRoot.contextMenuVisible = false
                                var act = modelData.action
                                if (act === "scale_large") {
                                    We10xState.desktopIconScale = 1.25
                                } else if (act === "scale_medium") {
                                    We10xState.desktopIconScale = 1.0
                                } else if (act === "scale_small") {
                                    We10xState.desktopIconScale = 0.85
                                } else if (act === "refresh") {
                                    We10xState.desktopRefreshToken += 1
                                    We10xState.reloadConfig()
                                } else if (act === "new_folder") {
                                    Quickshell.execDetached([
                                        "sh", "-c",
                                        "DESK=\"$(xdg-user-dir DESKTOP 2>/dev/null || echo \"$HOME/Desktop\")\"; mkdir -p \"$DESK/Yeni Klasör\" && exec nautilus \"$DESK\""
                                    ])
                                } else if (act === "open_terminal") {
                                    We10xState.launchCommand(["sh", "-c", "command -v kitty >/dev/null && exec kitty || exec alacritty"])
                                } else if (act === "display_settings") {
                                    We10xState.launchCommand(["solar-shell", "settings", "--tab", "display"])
                                } else if (act === "personalize") {
                                    We10xState.launchCommand(["solar-shell", "settings", "--tab", "personalize"])
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
