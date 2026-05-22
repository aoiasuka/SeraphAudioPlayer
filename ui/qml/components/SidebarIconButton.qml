import QtQuick

// 通用纯图标按钮 (用于 menu/list/min/max/close 等)
Item {
    id: root
    implicitWidth: 36
    implicitHeight: 36

    property string iconName: ""
    property int iconSize: 18
    property color iconColor: "#6B7280"
    property color hoverColor: "#F0F2F5"

    signal clicked()

    Rectangle {
        anchors.fill: parent
        radius: 8
        color: hoverArea.containsMouse ? root.hoverColor : "transparent"
        Behavior on color { ColorAnimation { duration: 120 } }
    }

    AppIcon {
        anchors.centerIn: parent
        name: root.iconName
        size: root.iconSize
        color: root.iconColor
        strokeWidth: 1.8
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
