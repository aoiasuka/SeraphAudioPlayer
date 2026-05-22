import QtQuick

// 圆形图标按钮 — 用于播放器控件区 (shuffle/prev/next/repeat/list)
Item {
    id: root
    property string iconName: ""
    property int size: 36
    property int iconSize: 18
    property color iconColor: "#6B7280"
    property bool iconFilled: false
    // -1 表示走 AppIcon 默认 strokeWidth；显式 0 表示纯填充
    property real strokeWidthOverride: -1
    // 角标文字(空字符串则不显示),用于 repeat-one 显示 "1"
    property string badgeText: ""

    signal clicked()

    implicitWidth: size
    implicitHeight: size

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: hoverArea.containsMouse ? "#F0F2F5" : "transparent"
        Behavior on color { ColorAnimation { duration: 120 } }
    }

    AppIcon {
        anchors.centerIn: parent
        name: root.iconName
        size: root.iconSize
        color: root.iconColor
        filled: root.iconFilled
        strokeWidth: root.strokeWidthOverride >= 0 ? root.strokeWidthOverride : 1.8
    }

    // 右下角小角标(repeat-one 用)
    Rectangle {
        visible: root.badgeText.length > 0
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.rightMargin: 2
        anchors.bottomMargin: 2
        width: badgeLabel.implicitWidth + 6
        height: 12
        radius: 6
        color: root.iconColor
        Text {
            id: badgeLabel
            anchors.centerIn: parent
            text: root.badgeText
            color: "#FFFFFF"
            font.pixelSize: 9
            font.weight: Font.Bold
        }
    }

    MouseArea {
        id: hoverArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
    }
}
