import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: altTabWin

    visible: We10xState.altTabOpen
    color: "#66000000"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusiveZone: -1

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-alttab"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.Exclusive : WlrKeyboardFocus.None

    property int selectedIndex: 0

    readonly property var windowsList: {
        var raw = ToplevelManager.toplevels.values
        var out = []
        for (var i = 0; i < raw.length; i++) {
            var w = raw[i]
            if (w && w.appId !== "org.blazeos.setup") {
                out.push(w)
            }
        }
        return out
    }

    onVisibleChanged: {
        if (visible) {
            selectedIndex = windowsList.length > 1 ? 1 : 0
            keyGrabber.forceActiveFocus()
        }
    }

    Component.onCompleted: {
        if (visible) {
            selectedIndex = windowsList.length > 1 ? 1 : 0
            keyGrabber.forceActiveFocus()
        }
    }

    function activateSelected() {
        if (windowsList.length > 0) {
            var idx = Math.max(0, Math.min(windowsList.length - 1, selectedIndex))
            var target = windowsList[idx]
            We10xState.closeAllFlyouts()
            if (target) {
                target.minimized = false
                target.activate()
            }
        } else {
            We10xState.closeAllFlyouts()
        }
    }

    MouseArea {
        anchors.fill: parent
        onClicked: We10xState.closeAllFlyouts()
    }

    Item {
        id: keyGrabber
        focus: true
        Keys.onPressed: (event) => {
            if (event.key === Qt.Key_Escape) {
                We10xState.closeAllFlyouts()
                event.accepted = true
            } else if (event.key === Qt.Key_Tab || event.key === Qt.Key_Right) {
                if (altTabWin.windowsList.length > 0) {
                    altTabWin.selectedIndex = (altTabWin.selectedIndex + 1) % altTabWin.windowsList.length
                }
                event.accepted = true
            } else if (event.key === Qt.Key_Backtab || event.key === Qt.Key_Left) {
                if (altTabWin.windowsList.length > 0) {
                    altTabWin.selectedIndex = (altTabWin.selectedIndex - 1 + altTabWin.windowsList.length) % altTabWin.windowsList.length
                }
                event.accepted = true
            } else if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter || event.key === Qt.Key_Space) {
                altTabWin.activateSelected()
                event.accepted = true
            }
        }
        Keys.onReleased: (event) => {
            if (event.key === Qt.Key_Alt || event.key === Qt.Key_Meta) {
                altTabWin.activateSelected()
                event.accepted = true
            }
        }
    }

    Rectangle {
        id: switcherCard
        width: Math.min(altTabWin.width - 80, Math.max(320, cardsRow.implicitWidth + 40))
        height: 176
        anchors.centerIn: parent
        color: We10xState.flyoutBg
        border.width: 1
        border.color: We10xState.flyoutBorder
        clip: true

        MouseArea {
            anchors.fill: parent
        }

        Text {
            visible: altTabWin.windowsList.length === 0
            anchors.centerIn: parent
            text: "Açık pencere yok"
            color: We10xState.textSecondary
            font.pixelSize: 14
        }

        Flickable {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 20
            contentWidth: cardsRow.implicitWidth
            contentHeight: height
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            Row {
                id: cardsRow
                anchors.verticalCenter: parent.verticalCenter
                spacing: 12

            Repeater {
                model: altTabWin.windowsList

                delegate: Rectangle {
                    width: 168
                    height: 136
                    color: index === altTabWin.selectedIndex ? We10xState.accentSubtle : We10xState.flyoutSurface
                    border.width: index === altTabWin.selectedIndex ? 2 : 1
                    border.color: index === altTabWin.selectedIndex ? We10xState.accent : We10xState.flyoutBorder

                    ColumnLayout {
                        anchors.fill: parent
                        anchors.margins: 10
                        spacing: 8

                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 6

                            Image {
                                Layout.preferredWidth: 18
                                Layout.preferredHeight: 18
                                sourceSize.width: 36
                                sourceSize.height: 36
                                source: We10xState.resolveToplevelIcon(modelData.appId)
                                fillMode: Image.PreserveAspectFit
                            }

                            Text {
                                Layout.fillWidth: true
                                text: modelData.title || modelData.appId || "Pencere"
                                color: We10xState.textPrimary
                                font.pixelSize: 11
                                font.bold: index === altTabWin.selectedIndex
                                elide: Text.ElideRight
                            }
                        }

                        Rectangle {
                            Layout.fillWidth: true
                            Layout.fillHeight: true
                            color: We10xState.isLight ? "#EAEAEA" : "#181818"

                            Image {
                                width: 38
                                height: 38
                                sourceSize.width: 76
                                sourceSize.height: 76
                                anchors.centerIn: parent
                                source: We10xState.resolveToplevelIcon(modelData.appId)
                                fillMode: Image.PreserveAspectFit
                            }

                            Text {
                                visible: modelData.minimized
                                anchors.bottom: parent.bottom
                                anchors.right: parent.right
                                anchors.margins: 4
                                text: "Küçültüldü"
                                color: We10xState.accent
                                font.pixelSize: 9
                                font.bold: true
                            }
                        }
                    }

                    MouseArea {
                        anchors.fill: parent
                        hoverEnabled: true
                        onEntered: altTabWin.selectedIndex = index
                        onClicked: {
                            altTabWin.selectedIndex = index
                            altTabWin.activateSelected()
                        }
                    }
                    }
                }
            }
        }
    }
}
