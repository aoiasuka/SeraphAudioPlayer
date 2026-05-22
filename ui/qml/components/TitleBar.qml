import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

// 一体化窗口控制条 — 透明拖拽区 + 右上角浮岛式系统按钮 (纯 Qt frameless, 不依赖 QWindowKit)
// 设计语言:与应用的玻璃态/悬浮岛保持一致
Rectangle {
    id: root
    height: 36
    color: "transparent"

    required property Window targetWindow

    // 暴露按钮 (无外部 hit-test 库,但保留 alias 以备扩展)
    property alias minimizeButton: btnMin
    property alias maximizeButton: btnMax
    property alias closeButton: btnClose

    readonly property bool isMaximized: targetWindow.visibility === Window.Maximized

    // ===== 拖拽区 (除去按钮区域以外的整条 titleBar 都可拖窗 & 双击切最大化) =====
    MouseArea {
        id: dragArea
        anchors.left: parent.left
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

    // ===== 右上角三联系统按钮 (Win11 一体化规格 46×32, 合体无间隙) =====
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
            color: minHover.containsPress
                   ? "#22000000"
                   : (minHover.containsMouse ? "#11000000" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            AppIcon {
                anchors.centerIn: parent
                name: "min"
                size: 10
                color: window.textPrimary
                strokeWidth: 1.5
            }

            HoverHandler { id: minHover; cursorShape: Qt.ArrowCursor }
            TapHandler {
                onTapped: root.targetWindow.showMinimized()
            }
        }

        // ---- 最大化 / 还原 ----
        Rectangle {
            id: btnMax
            width: 46
            height: parent.height
            color: maxHover.containsPress
                   ? "#22000000"
                   : (maxHover.containsMouse ? "#11000000" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            // 普通态:单层方框
            Item {
                anchors.centerIn: parent
                width: 12; height: 12
                visible: !root.isMaximized
                Rectangle {
                    anchors.fill: parent
                    color: "transparent"
                    border.color: window.textPrimary
                    border.width: 1.2
                    radius: 1
                }
            }
            // 还原态:双层错位方框
            Item {
                anchors.centerIn: parent
                width: 12; height: 12
                visible: root.isMaximized
                Rectangle {
                    width: 9; height: 9
                    x: 3; y: 0
                    color: "transparent"
                    border.color: window.textPrimary
                    border.width: 1.2
                    radius: 1
                }
                Rectangle {
                    width: 9; height: 9
                    x: 0; y: 3
                    color: btnMax.color
                    border.color: window.textPrimary
                    border.width: 1.2
                    radius: 1
                }
            }

            HoverHandler { id: maxHover; cursorShape: Qt.ArrowCursor }
            TapHandler {
                onTapped: {
                    if (root.isMaximized) root.targetWindow.showNormal()
                    else                  root.targetWindow.showMaximized()
                }
            }
        }

        // ---- 关闭 (Win11 标准红 #E81123) ----
        Rectangle {
            id: btnClose
            width: 46
            height: parent.height
            color: closeHover.containsPress
                   ? "#C5202F"
                   : (closeHover.containsMouse ? "#E81123" : "transparent")
            Behavior on color { ColorAnimation { duration: 120 } }

            AppIcon {
                anchors.centerIn: parent
                name: "close"
                size: 10
                color: closeHover.containsMouse ? "#FFFFFF" : window.textPrimary
                strokeWidth: 1.5
            }

            HoverHandler { id: closeHover; cursorShape: Qt.ArrowCursor }
            TapHandler {
                onTapped: root.targetWindow.close()
            }
        }
    }
}
