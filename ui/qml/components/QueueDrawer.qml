import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects

// 当前播放队列抽屉(从右侧滑入,悬浮岛风格)
Item {
    id: root

    property bool open: false
    property bool _initialized: false
    visible: open || slideAnim.running

    function show()   { root.open = true }
    function hide()   { root.open = false }
    function toggle() { root.open = !root.open }

    // 滑入动画 — 初始化完成前禁用，避免启动时闪现
    Behavior on x {
        enabled: root._initialized
        NumberAnimation { id: slideAnim; duration: 220; easing.type: Easing.OutQuart }
    }

    // 默认隐藏在右侧外
    Component.onCompleted: {
        updatePos()
        // 延迟一帧再启用动画，确保首次定位是瞬时的
        Qt.callLater(function() { root._initialized = true })
    }
    onWidthChanged: updatePos()
    onOpenChanged: updatePos()
    function updatePos() {
        x = open ? (parent ? parent.width - width - 16 : 0)
                 : (parent ? parent.width + 16 : 0)
    }

    // 点击外侧关闭(透明遮罩)
    Item {
        id: overlay
        parent: root.parent
        anchors.fill: parent
        visible: root.open
        z: root.z - 1

        MouseArea {
            anchors.fill: parent
            onClicked: root.hide()
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: window.largeRadius
        // 与左侧 Sidebar 一致的半透明毛玻璃材质，融入底部紫色渐变
        color: window.sidebarBg
        border.color: window.glassBorder
        border.width: 1
        antialiasing: true

        // 柔和弥散阴影 (轻量实现: 四周淡黑色描边层)
        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            radius: parent.radius + 1
            color: "transparent"
            border.color: "#14000000"
            border.width: 1
            z: -1
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 16
            spacing: 8

            // 标题栏
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: "当前队列"
                    font.family: window.fontFamily
                    font.pixelSize: 18
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: "(" + (playerVM.queue ? playerVM.queue.length : 0) + ")"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    color: window.textSecondary
                }
                Item { Layout.fillWidth: true }
                SidebarIconButton {
                    iconName: "close"
                    iconSize: 14
                    implicitWidth: 30
                    implicitHeight: 30
                    onClicked: root.hide()
                }
            }

            // 操作行
            RowLayout {
                Layout.fillWidth: true
                spacing: 6

                Rectangle {
                    Layout.preferredHeight: 30
                    Layout.preferredWidth: clearTxt.implicitWidth + 24
                    radius: 15
                    color: clearArea.containsMouse ? "#FEE2E2" : "transparent"
                    border.color: "#33EF4444"
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    Text {
                        id: clearTxt
                        anchors.centerIn: parent
                        text: "清空"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: "#DC2626"
                    }

                    MouseArea {
                        id: clearArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: playerVM.clearQueue()
                    }
                }

                Item { Layout.fillWidth: true }

                Text {
                    text: ["顺序", "列表循环", "单曲循环"][playerVM.repeatMode]
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    color: window.textSecondary
                }
                Rectangle { Layout.preferredWidth: 1; Layout.preferredHeight: 12; color: window.borderColor }
                Text {
                    text: playerVM.shuffle ? "随机开" : "随机关"
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    color: window.textSecondary
                }
            }

            // 列表
            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                ListView {
                    id: list
                    model: playerVM.queue
                    spacing: 2

                    // 空态
                    Text {
                        anchors.centerIn: parent
                        visible: list.count === 0
                        text: "队列为空 — 从首页打开音频文件"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textTertiary
                    }

                    delegate: Item {
                        width: list.width
                        height: 64

                        Rectangle {
                            anchors.fill: parent
                            anchors.margins: 2
                            radius: 10
                            color: modelData.isCurrent ? window.activeBg
                                 : (rowArea.containsMouse ? window.hoverBg : "transparent")
                            border.color: modelData.isCurrent ? window.brand : "transparent"
                            border.width: 1
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 12
                            anchors.rightMargin: 8
                            spacing: 10

                            // 序号 / 喇叭
                            Item {
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22

                                Text {
                                    anchors.centerIn: parent
                                    visible: !modelData.isCurrent
                                    text: (index + 1)
                                    font.family: window.fontFamily
                                    font.pixelSize: 12
                                    color: window.textTertiary
                                }

                                AppIcon {
                                    anchors.centerIn: parent
                                    visible: modelData.isCurrent
                                    name: "volume"
                                    size: 14
                                    color: window.brand
                                    strokeWidth: 2
                                }
                            }

                            // 封面缩略图
                            Item {
                                Layout.preferredWidth: 36
                                Layout.preferredHeight: 36
                                clip: true

                                Rectangle {
                                    anchors.fill: parent
                                    radius: 6
                                    color: modelData.isCurrent ? window.brandSoft : "transparent"
                                    border.color: window.borderColor
                                    border.width: 1
                                }

                                Rectangle {
                                    id: qTnMask
                                    width: qTn.width
                                    height: qTn.height
                                    radius: 6
                                    color: "black"
                                    antialiasing: true
                                }

                                Image {
                                    id: qTn
                                    anchors.fill: parent
                                    source: modelData.coverUrl || ""
                                    visible: source.toString().length > 0 && status === Image.Ready
                                    fillMode: Image.PreserveAspectCrop
                                    asynchronous: true
                                    cache: true

                                    layer.enabled: true
                                    layer.effect: MultiEffect {
                                        maskEnabled: true
                                        maskSource: ShaderEffectSource {
                                            sourceItem: qTnMask
                                            hideSource: true
                                        }
                                    }
                                }

                                AppIcon {
                                    anchors.centerIn: parent
                                    visible: !qTn.visible
                                    name: "music"
                                    size: 16
                                    color: window.textTertiary
                                    strokeWidth: 1.6
                                }
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 2
                                Text {
                                    Layout.fillWidth: true
                                    text: modelData.title
                                    font.family: window.fontFamily
                                    font.pixelSize: 13
                                    font.weight: modelData.isCurrent ? Font.DemiBold : Font.Medium
                                    color: modelData.isCurrent ? window.brand : window.textPrimary
                                    elide: Text.ElideRight
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: (modelData.artist || modelData.suffix) + (modelData.duration ? " · " + modelData.duration : "")
                                    font.family: window.fontFamily
                                    font.pixelSize: 11
                                    color: window.textTertiary
                                    elide: Text.ElideRight
                                }
                            }

                            // 移除按钮
                            Item {
                                Layout.preferredWidth: 26
                                Layout.preferredHeight: 26
                                visible: rowArea.containsMouse || delArea.containsMouse

                                Rectangle {
                                    anchors.fill: parent
                                    radius: 13
                                    color: delArea.containsMouse ? "#FEE2E2" : "transparent"
                                    Behavior on color { ColorAnimation { duration: 120 } }
                                }

                                AppIcon {
                                    anchors.centerIn: parent
                                    name: "close"
                                    size: 12
                                    color: delArea.containsMouse ? "#DC2626" : window.textTertiary
                                    strokeWidth: 2
                                }

                                MouseArea {
                                    id: delArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: playerVM.removeAt(index)
                                }
                            }

                            // 上移
                            Item {
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22
                                visible: rowArea.containsMouse
                                AppIcon {
                                    anchors.centerIn: parent
                                    name: "chevron"; rotation: -90; size: 12
                                    color: qUp.containsMouse ? window.brand : window.textTertiary
                                    strokeWidth: 2
                                }
                                MouseArea {
                                    id: qUp
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: playerVM.moveQueueItem(index, index - 1)
                                }
                            }
                            // 下移
                            Item {
                                Layout.preferredWidth: 22
                                Layout.preferredHeight: 22
                                visible: rowArea.containsMouse
                                AppIcon {
                                    anchors.centerIn: parent
                                    name: "chevron"; rotation: 90; size: 12
                                    color: qDown.containsMouse ? window.brand : window.textTertiary
                                    strokeWidth: 2
                                }
                                MouseArea {
                                    id: qDown
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: playerVM.moveQueueItem(index, index + 1)
                                }
                            }
                        }

                        MouseArea {
                            id: rowArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: playerVM.playIndex(index)
                            z: -1
                        }
                    }
                }
            }
        }
    }
}
