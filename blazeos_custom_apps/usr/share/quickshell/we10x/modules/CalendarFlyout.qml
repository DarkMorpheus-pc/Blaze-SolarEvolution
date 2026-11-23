import QtQuick
import QtQuick.Layouts
import Quickshell
import Quickshell.Wayland

PanelWindow {
    id: calWindow

    visible: We10xState.calendarOpen
    color: "transparent"

    anchors {
        top: true
        bottom: true
        left: true
        right: true
    }
    exclusiveZone: 0

    WlrLayershell.layer: WlrLayer.Overlay
    WlrLayershell.namespace: "we10x-calendar"
    WlrLayershell.keyboardFocus: visible ? WlrKeyboardFocus.OnDemand : WlrKeyboardFocus.None

    SystemClock {
        id: secClock
        precision: SystemClock.Seconds
    }

    property int viewYear: new Date().getFullYear()
    property int viewMonth: new Date().getMonth()

    onVisibleChanged: {
        if (visible) {
            var now = new Date()
            viewYear = now.getFullYear()
            viewMonth = now.getMonth()
        }
    }

    Component.onCompleted: {
        if (visible) {
            var now = new Date()
            viewYear = now.getFullYear()
            viewMonth = now.getMonth()
        }
    }

    readonly property var monthNamesTr: [
        "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran",
        "Temmuz", "Ağustos", "Eylül", "Ekim", "Kasım", "Aralık"
    ]
    readonly property var dayNamesFullTr: [
        "Pazar", "Pazartesi", "Salı", "Çarşamba", "Perşembe", "Cuma", "Cumartesi"
    ]
    readonly property var dayHeadersTr: ["Pt", "Sa", "Ça", "Pe", "Cu", "Ct", "Pz"]

    readonly property var calendarCells: {
        var first = new Date(viewYear, viewMonth, 1)
        // Monday-based index (0=Mon .. 6=Sun)
        var startDow = (first.getDay() + 6) % 7
        var daysInMonth = new Date(viewYear, viewMonth + 1, 0).getDate()
        var daysInPrev = new Date(viewYear, viewMonth, 0).getDate()
        var now = new Date()

        var cells = []
        for (var p = startDow - 1; p >= 0; p--) {
            cells.push({ day: daysInPrev - p, inMonth: false, isToday: false })
        }
        for (var d = 1; d <= daysInMonth; d++) {
            var today = (d === now.getDate() && viewMonth === now.getMonth() && viewYear === now.getFullYear())
            cells.push({ day: d, inMonth: true, isToday: today })
        }
        var nextDay = 1
        while (cells.length < 42) {
            cells.push({ day: nextDay++, inMonth: false, isToday: false })
        }
        return cells
    }

    MouseArea {
        anchors.fill: parent
        onPressed: We10xState.closeAllFlyouts()
    }

    Rectangle {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        width: Math.min(360, parent.width)
        height: Math.min(432, parent.height)
        color: We10xState.flyoutBg
        border.width: 1
        border.color: We10xState.flyoutBorder
        radius: 0

        MouseArea {
            anchors.fill: parent
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 18
            spacing: 12

            // Live Seconds Clock + Full Turkish Date
            Column {
                Layout.fillWidth: true
                spacing: 4

                Text {
                    text: Qt.formatTime(secClock.date, "HH:mm:ss")
                    color: We10xState.textPrimary
                    font.pixelSize: 38
                    font.weight: Font.Light
                }

                Text {
                    text: {
                        var d = secClock.date
                        return d.getDate() + " " + calWindow.monthNamesTr[d.getMonth()] + " " + d.getFullYear() + " " + calWindow.dayNamesFullTr[d.getDay()]
                    }
                    color: We10xState.accent
                    font.pixelSize: 13
                }
            }

            Rectangle {
                Layout.fillWidth: true
                height: 1
                color: We10xState.flyoutBorder
            }

            // Month Navigation Row
            RowLayout {
                Layout.fillWidth: true

                Text {
                    Layout.fillWidth: true
                    text: calWindow.monthNamesTr[calWindow.viewMonth] + " " + calWindow.viewYear
                    color: We10xState.textPrimary
                    font.pixelSize: 14
                    font.weight: Font.Medium
                }

                Rectangle {
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    color: prevMouse.containsMouse ? We10xState.hoverBg : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: "▲"
                        color: We10xState.textPrimary
                        font.pixelSize: 11
                    }
                    MouseArea {
                        id: prevMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            if (calWindow.viewMonth === 0) {
                                calWindow.viewMonth = 11
                                calWindow.viewYear -= 1
                            } else {
                                calWindow.viewMonth -= 1
                            }
                        }
                    }
                }

                Rectangle {
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    color: nextMouse.containsMouse ? We10xState.hoverBg : "transparent"
                    Text {
                        anchors.centerIn: parent
                        text: "▼"
                        color: We10xState.textPrimary
                        font.pixelSize: 11
                    }
                    MouseArea {
                        id: nextMouse
                        anchors.fill: parent
                        hoverEnabled: true
                        onClicked: {
                            if (calWindow.viewMonth === 11) {
                                calWindow.viewMonth = 0
                                calWindow.viewYear += 1
                            } else {
                                calWindow.viewMonth += 1
                            }
                        }
                    }
                }
            }

            // Day-of-week Header
            GridLayout {
                Layout.fillWidth: true
                columns: 7
                columnSpacing: 2

                Repeater {
                    model: calWindow.dayHeadersTr
                    delegate: Text {
                        Layout.fillWidth: true
                        horizontalAlignment: Text.AlignHCenter
                        text: modelData
                        color: We10xState.textSecondary
                        font.pixelSize: 11
                        font.bold: true
                    }
                }
            }

            // 6x7 Monthly Day Grid
            GridLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                columns: 7
                rowSpacing: 2
                columnSpacing: 2

                Repeater {
                    model: calWindow.calendarCells

                    delegate: Rectangle {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        color: modelData.isToday
                            ? We10xState.accent
                            : (dayMouse.containsMouse ? We10xState.hoverBg : "transparent")
                        border.width: dayMouse.containsMouse ? 1 : 0
                        border.color: We10xState.accent

                        Text {
                            anchors.centerIn: parent
                            text: modelData.day.toString()
                            color: modelData.isToday
                                ? "#FFFFFF"
                                : (modelData.inMonth ? We10xState.textPrimary : We10xState.textSecondary)
                            opacity: (!modelData.inMonth && !modelData.isToday) ? 0.55 : 1.0
                            font.pixelSize: 12
                            font.bold: modelData.isToday
                        }

                        MouseArea {
                            id: dayMouse
                            anchors.fill: parent
                            hoverEnabled: true
                        }
                    }
                }
            }
        }
    }
}
