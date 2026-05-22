import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 快捷键速查对话框 — F1 / Ctrl+? 触发
Dialog {
    id: root
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 720, 720)
    height: 540
    padding: 0
    standardButtons: Dialog.NoButton

    Overlay.modal: Rectangle {
        color: window.modalScrim
        Behavior on opacity { NumberAnimation { duration: 180 } }
    }

    background: Rectangle {
        radius: window.largeRadius
        color: window.glassBg
        border.color: window.glassBorderDark
        border.width: 1
        antialiasing: true

        Rectangle {
            anchors.fill: parent
            anchors.margins: -1
            radius: parent.radius + 1
            color: "transparent"
            border.color: "#22000000"
            border.width: 1
            z: -1
        }
    }

    header: Item { visible: false; height: 0 }
    footer: Item { visible: false; height: 0 }

    readonly property var groups: [
        {
            title: "播放控制",
            items: [
                { key: "Space",   desc: "播放 / 暂停" },
                { key: "←",       desc: "上一首" },
                { key: "→",       desc: "下一首" },
                { key: "↑",       desc: "音量 +5" },
                { key: "↓",       desc: "音量 -5" },
                { key: "M",       desc: "静音切换" },
                { key: "Ctrl+L",  desc: "喜欢 / 取消喜欢当前曲目" },
                { key: "Ctrl+R",  desc: "切换循环模式" },
                { key: "Ctrl+S",  desc: "切换随机播放" }
            ]
        },
        {
            title: "界面",
            items: [
                { key: "Ctrl+Q",  desc: "打开 / 收起播放队列" },
                { key: "Ctrl+E",  desc: "均衡器" },
                { key: "Esc",     desc: "返回 / 关闭队列 / 退出全屏" },
                { key: "F11",     desc: "切换全屏" },
                { key: "F1",      desc: "显示此帮助" }
            ]
        },
        {
            title: "导航",
            items: [
                { key: "1",       desc: "首页" },
                { key: "2",       desc: "音乐库" },
                { key: "3",       desc: "歌单" },
                { key: "4",       desc: "歌手" },
                { key: "5",       desc: "专辑" },
                { key: "6",       desc: "最近播放" },
                { key: "7",       desc: "我喜欢的" },
                { key: "Ctrl+,",  desc: "设置" }
            ]
        },
        {
            title: "文件",
            items: [
                { key: "拖拽",    desc: "拖拽 .wav / .flac 文件到窗口加入队列" }
            ]
        }
    ]

    contentItem: Item {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            // 标题栏 + 右上角圆形关闭
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: "键盘快捷键"
                    font.family: window.fontFamily
                    font.pixelSize: 18
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Item { Layout.fillWidth: true }

                Item {
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30

                    Rectangle {
                        anchors.fill: parent
                        radius: 15
                        color: shortcutCloseArea.containsMouse ? "#33000000" : "transparent"
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        name: "close"
                        size: 14
                        color: window.textPrimary
                        strokeWidth: 2
                    }
                    MouseArea {
                        id: shortcutCloseArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }
                }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                ColumnLayout {
                    width: root.availableWidth - 40
                    spacing: 16

                    Repeater {
                        model: root.groups
                        delegate: ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 8

                            Text {
                                text: modelData.title
                                font.family: window.fontFamily
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                                color: window.brand
                            }

                            Repeater {
                                model: modelData.items
                                delegate: RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 16

                                    Rectangle {
                                        Layout.preferredWidth: 110
                                        Layout.preferredHeight: 28
                                        radius: window.smallRadius
                                        color: window.sidebarBg
                                        border.color: window.borderColor
                                        border.width: 1

                                        Text {
                                            anchors.centerIn: parent
                                            text: modelData.key
                                            font.family: "Consolas"
                                            font.pixelSize: 12
                                            font.weight: Font.DemiBold
                                            color: window.textPrimary
                                        }
                                    }

                                    Text {
                                        Layout.fillWidth: true
                                        text: modelData.desc
                                        font.family: window.fontFamily
                                        font.pixelSize: 13
                                        color: window.textSecondary
                                        wrapMode: Text.WordWrap
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
