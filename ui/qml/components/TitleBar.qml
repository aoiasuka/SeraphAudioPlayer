import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

// Synapse 风格标题栏 — Acrylic 半透明白 + 品牌 logo + 系统按钮
//
// 布局:
//   左 (可点击): 品牌圆形 disc 图标(slow spin) + "Synapse Audio" 文字 — 点击切换侧栏
//   中: 拖拽区
//   右: min / max / close (Win11 一体化 46x32)
Rectangle {
    id: root
    height: 40
    color: window.acrylicTitleBar
    border.width: 0

    required property Window targetWindow

    property alias minimizeButton: btnMin
    property alias maximizeButton: btnMax
    property alias closeButton: btnClose

    signal hamburgerClicked()

    readonly property bool isMaximized: targetWindow.visibility === Window.Maximized

    // 底部 1px 极细分隔
    Rectangle {
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: 1
        color: window.borderColor
    }

    // ===== 左: 品牌 logo + 名称 (可点击切换侧栏) =====
    Item {
        id: brand
        anchors.left: parent.left
        anchors.leftMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        width: brandRow.implicitWidth + 12
        height: parent.height

        Rectangle {
            anchors.fill: parent
            anchors.topMargin: 4
            anchors.bottomMargin: 4
            radius: 6
            color: brandArea.pressed
                   ? "#1A000000"
                   : (brandArea.containsMouse ? "#0A000000" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }
        }

        RowLayout {
            id: brandRow
            anchors.centerIn: parent
            spacing: 8

            // Disc 图标 (slow spin)
            Item {
                Layout.preferredWidth: 18
                Layout.preferredHeight: 18

                AppIcon {
                    id: discIcon
                    anchors.centerIn: parent
                    name: "album"
                    size: 16
                    color: window.brand
                    strokeWidth: 1.8
                    RotationAnimation on rotation {
                        from: 0; to: 360
                        duration: 12000
                        loops: Animation.Infinite
                        running: true
                    }
                }
            }

            Text {
                text: "Seraph"
                font.family: window.fontFamily
                font.pixelSize: 13
                font.weight: Font.Bold
                color: window.brand
            }

            Text {
                text: "Audio Player"
                font.family: window.fontFamily
                font.pixelSize: 13
                font.weight: Font.Medium
                color: window.textSecondary
            }

            Rectangle {
                Layout.preferredHeight: 14
                Layout.preferredWidth: badgeText.implicitWidth + 8
                radius: 3
                color: window.brandSoftBg
                border.color: Qt.rgba(8/255, 145/255, 178/255, 0.3)
                border.width: 1
                Layout.alignment: Qt.AlignVCenter

                Text {
                    id: badgeText
                    anchors.centerIn: parent
                    text: "HIFI"
                    font.family: window.fontFamily
                    font.pixelSize: 8
                    font.weight: Font.Bold
                    font.letterSpacing: 0.5
                    color: window.brand
                }
            }
        }

        MouseArea {
            id: brandArea
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: root.hamburgerClicked()
            ToolTip.visible: containsMouse
            ToolTip.delay: 500
            ToolTip.text: "展开/折叠侧栏"
        }
    }

    // ===== 拖拽区 =====
    MouseArea {
        id: dragArea
        anchors.left: brand.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.right: controls.left
        acceptedButtons: Qt.LeftButton
        onPressed: function(mouse) {
            if (mouse.button === Qt.LeftButton) {
                root.targetWindow.startSystemMove()
            }
        }
        onDoubleClicked: function(mouse) {
            if (mouse.button !== Qt.LeftButton) return
            if (root.isMaximized) root.targetWindow.showNormal()
            else                  root.targetWindow.showMaximized()
        }
    }

    // ===== 右上角三联系统按钮 (Win11 规格 46x40) =====
    Row {
        id: controls
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        spacing: 0

        // ---- 最小化 ----
        Rectangle {
            id: btnMin
            width: 46
            height: parent.height
            color: minArea.pressed
                   ? "#1F000000"
                   : (minArea.containsMouse ? "#0E000000" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            AppIcon {
                anchors.centerIn: parent
                name: "min"
                size: 12
                color: window.textSecondary
                strokeWidth: 1.2
            }

            MouseArea {
                id: minArea
                anchors.fill: parent
                hoverEnabled: true
                onClicked: root.targetWindow.showMinimized()
                ToolTip.visible: containsMouse
                ToolTip.delay: 500
                ToolTip.text: "最小化"
            }
        }

        // ---- 最大化 / 还原 ----
        Rectangle {
            id: btnMax
            width: 46
            height: parent.height
            color: maxArea.pressed
                   ? "#1F000000"
                   : (maxArea.containsMouse ? "#0E000000" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            // 普通态: 单层方框
            Item {
                anchors.centerIn: parent
                width: 11; height: 11
                visible: !root.isMaximized
                Rectangle {
                    anchors.fill: parent
                    color: "transparent"
                    border.color: window.textSecondary
                    border.width: 1.2
                    radius: 1
                }
            }
            // 还原态: 双层错位方框
            Item {
                anchors.centerIn: parent
                width: 12; height: 12
                visible: root.isMaximized
                Rectangle {
                    width: 9; height: 9
                    x: 3; y: 0
                    color: "transparent"
                    border.color: window.textSecondary
                    border.width: 1.2
                    radius: 1
                }
                Rectangle {
                    width: 9; height: 9
                    x: 0; y: 3
                    color: btnMax.color
                    border.color: window.textSecondary
                    border.width: 1.2
                    radius: 1
                }
            }

            MouseArea {
                id: maxArea
                anchors.fill: parent
                hoverEnabled: true
                onClicked: {
                    if (root.isMaximized) root.targetWindow.showNormal()
                    else                  root.targetWindow.showMaximized()
                }
                ToolTip.visible: containsMouse
                ToolTip.delay: 500
                ToolTip.text: root.isMaximized ? "向下还原" : "最大化"
            }
        }

        // ---- 关闭 ----
        Rectangle {
            id: btnClose
            width: 46
            height: parent.height
            color: closeArea.pressed
                   ? "#C5202F"
                   : (closeArea.containsMouse ? "#E81123" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            AppIcon {
                anchors.centerIn: parent
                name: "close"
                size: 12
                color: closeArea.containsMouse ? "#FFFFFF" : window.textSecondary
                strokeWidth: 1.2
            }

            MouseArea {
                id: closeArea
                anchors.fill: parent
                hoverEnabled: true
                onClicked: root.targetWindow.close()
                ToolTip.visible: containsMouse
                ToolTip.delay: 500
                ToolTip.text: "关闭"
            }
        }
    }
}
