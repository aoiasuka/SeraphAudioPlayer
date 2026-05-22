import QtQuick
import QtQuick.Layouts

// 侧栏导航项 (icon + label + active 指示条)
Item {
    id: root
    implicitHeight: 40

    property string navKey: ""
    property string label: ""
    property string iconName: ""
    property bool active: false

    signal clicked()

    // 整体按压缩放反馈 (极简灵动微动效)
    scale: hoverArea.pressed ? 0.96 : 1.0
    Behavior on scale { NumberAnimation { duration: 100; easing.type: Easing.OutQuad } }

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        // 仅绘制悬浮背景（激活背景与指示条由 Sidebar 的 sharedHighlight 统一接管，实现极致顺滑的滑动独占高亮）
        color: hoverArea.containsMouse ? "#153B82F6" : "transparent"
        Behavior on color { ColorAnimation { duration: 150 } }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16  // 稍微加宽边距，呼应气泡悬浮感
        anchors.rightMargin: 12
        spacing: 12

        AppIcon {
            name: root.iconName
            size: 18
            // 悬浮时 icon 也会有渐变品牌色反馈，体验更精致
            color: root.active ? window.brand : (hoverArea.containsMouse ? window.brand : window.textSecondary)
            strokeWidth: root.active ? 2 : 1.8
            Behavior on color { ColorAnimation { duration: 120 } }
            
            // 激活时图标微调缩放，增加趣味性
            transform: Scale {
                origin.x: 9; origin.y: 9
                xScale: root.active ? 1.1 : 1.0
                yScale: root.active ? 1.1 : 1.0
                Behavior on xScale { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
                Behavior on yScale { NumberAnimation { duration: 200; easing.type: Easing.OutBack } }
            }
        }

        Text {
            Layout.fillWidth: true
            text: root.label
            font.family: window.fontFamily
            font.pixelSize: 13
            font.weight: root.active ? Font.DemiBold : Font.Medium
            // 激活和悬浮状态下文本颜色过渡
            color: root.active ? window.brand : (hoverArea.containsMouse ? window.textPrimary : window.textSecondary)
            elide: Text.ElideRight
            Behavior on color { ColorAnimation { duration: 120 } }
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
