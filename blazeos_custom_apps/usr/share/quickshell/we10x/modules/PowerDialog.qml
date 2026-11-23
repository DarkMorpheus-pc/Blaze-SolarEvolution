import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: powerWin

    visible: We10xState.powerDialogOpen
    color: "#80000000"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusiveZone: -1

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-power"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    onVisibleChanged: {
        if (visible) {
            powerCard.forceActiveFocus()
        }
    }

    Component.onCompleted: {
        if (visible) {
            powerCard.forceActiveFocus()
        }
    }

    MouseArea {
        anchors.fill: parent
        onClicked: We10xState.closeAllFlyouts()
    }

    Rectangle {
        id: powerCard
        width: 420
        height: 290
        anchors.centerIn: parent
        color: We10xState.flyoutBg
        border.width: 1
        border.color: We10xState.accent
        focus: true

        Keys.onEscapePressed: We10xState.closeAllFlyouts()

        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 22
            spacing: 12

            Text {
                text: "BlazeOS — Oturum ve Güç Seçenekleri"
                color: We10xState.textPrimary
                font.pixelSize: 16
                font.weight: Font.DemiBold
            }

            Repeater {
                model: [
                    { label: "Ekranı Kilitle",            cmd: ["solar-lock"] },
                    { label: "Görev Yöneticisi",          cmd: ["blaze-system-monitor"] },
                    { label: "Oturumu Kapat",             cmd: ["niri", "msg", "action", "quit", "--skip-confirmation"] },
                    { label: "Yeniden Başlat",            cmd: ["systemctl", "reboot"] },
                    { label: "Bilgisayarı Kapat",         cmd: ["systemctl", "poweroff"] }
                ]

                delegate: Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    color: actMouse.containsMouse ? We10xState.accent : We10xState.flyoutSurface
                    border.width: 1
                    border.color: We10xState.flyoutBorder

                    Text {
                        anchors.centerIn: parent
                        text: modelData.label
                        color: actMouse.containsMouse ? "#FFFFFF" : We10xState.textPrimary
                        font.pixelSize: 13
                    }

                    MouseArea {
                        id: actMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: We10xState.launchCommand(modelData.cmd)
                    }
                }
            }
        }
    }
}
