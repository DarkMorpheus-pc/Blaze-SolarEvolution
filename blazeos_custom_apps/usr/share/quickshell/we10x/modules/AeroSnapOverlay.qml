import QtQuick
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: snapOverlay

    visible: We10xState.aeroSnapZone !== ""
    color: "transparent"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-aerosnap"
    WlrLayershell.exclusiveZone: -1
    WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
    mask: Region {}

    readonly property real usableHeight: Math.max(100, height - 44)

    Rectangle {
        id: snapPreview
        visible: We10xState.aeroSnapZone !== ""
        color: We10xState.accentSubtle
        border.width: 2
        border.color: We10xState.accent
        radius: 0

        x: We10xState.aeroSnapZone === "right" ? Math.round(snapOverlay.width / 2) + 4 : 4
        y: 4
        width: (We10xState.aeroSnapZone === "left" || We10xState.aeroSnapZone === "right")
            ? Math.round(snapOverlay.width / 2) - 8
            : snapOverlay.width - 8
        height: snapOverlay.usableHeight - 8

        Behavior on x { NumberAnimation { duration: 90; easing.type: Easing.OutQuad } }
        Behavior on width { NumberAnimation { duration: 90; easing.type: Easing.OutQuad } }
    }
}
