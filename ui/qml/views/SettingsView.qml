import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"

// 设置视图：输出设备选择 + 播放偏好
Item {
    id: root
    objectName: "settingsView"

    // 顶部标题
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 80

        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            anchors.topMargin: 16
            spacing: 4

            Text {
                text: "设置"
                font.family: window.fontFamily
                font.pixelSize: 26
                font.weight: Font.Bold
                color: window.textPrimary
            }
            Text {
                text: "配置输出设备与播放偏好"
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
            }
        }
    }

    Flickable {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 8
        contentWidth: width
        contentHeight: settingsCol.implicitHeight + 32
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

        ColumnLayout {
            id: settingsCol
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            spacing: 16

            // ---- 输出设备 ----
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: deviceCol.implicitHeight + 32
                radius: 16
                color: window.sidebarBg
                border.color: "#33FFFFFF"
                border.width: 1

                ColumnLayout {
                    id: deviceCol
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 12

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        Rectangle {
                            width: 36; height: 36; radius: 10
                            color: "#DBEAFE"
                            AppIcon {
                                anchors.centerIn: parent
                                name: "volume"; size: 18
                                color: window.brand; strokeWidth: 2
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: "输出设备 (WASAPI 独占)"
                                font.family: window.fontFamily
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                color: window.textPrimary
                            }
                            Text {
                                text: "选择音频输出 — 切换后立即生效"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        Rectangle {
                            Layout.preferredWidth: 64
                            Layout.preferredHeight: 32
                            radius: 16
                            color: refreshArea.containsMouse ? window.cardHover : "transparent"
                            border.color: window.borderColor
                            border.width: 1
                            Behavior on color { ColorAnimation { duration: 150 } }

                            Text {
                                anchors.centerIn: parent
                                text: "刷新"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textPrimary
                            }

                            MouseArea {
                                id: refreshArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.refreshDevices()
                            }
                        }
                    }

                    // 当前设备
                    Text {
                        Layout.fillWidth: true
                        text: "当前：" + (playerVM.currentDeviceName || "(无)")
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textSecondary
                        elide: Text.ElideRight
                    }

                    // 设备列表
                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Repeater {
                            model: playerVM.devices

                            delegate: Rectangle {
                                Layout.fillWidth: true
                                Layout.preferredHeight: 56
                                radius: 12
                                readonly property bool selected: modelData.id === playerVM.currentDeviceId
                                color: selected ? window.activeBg
                                     : (devArea.containsMouse ? window.hoverBg : "transparent")
                                border.color: selected ? window.brand : window.borderColor
                                border.width: 1
                                Behavior on color { ColorAnimation { duration: 150 } }

                                RowLayout {
                                    anchors.fill: parent
                                    anchors.leftMargin: 14
                                    anchors.rightMargin: 14
                                    spacing: 12

                                    Rectangle {
                                        width: 14; height: 14; radius: 7
                                        color: "transparent"
                                        border.color: parent.parent.selected ? window.brand : window.textTertiary
                                        border.width: 2

                                        Rectangle {
                                            anchors.centerIn: parent
                                            width: 6; height: 6; radius: 3
                                            color: window.brand
                                            visible: parent.parent.parent.selected
                                        }
                                    }

                                    ColumnLayout {
                                        Layout.fillWidth: true
                                        spacing: 2

                                        Text {
                                            Layout.fillWidth: true
                                            text: modelData.name
                                            font.family: window.fontFamily
                                            font.pixelSize: 14
                                            font.weight: Font.DemiBold
                                            color: window.textPrimary
                                            elide: Text.ElideRight
                                        }
                                        Text {
                                            Layout.fillWidth: true
                                            text: modelData.isDefault === true ? "系统默认" : modelData.id
                                            font.family: window.fontFamily
                                            font.pixelSize: 11
                                            color: window.textSecondary
                                            elide: Text.ElideRight
                                        }
                                    }

                                    Rectangle {
                                        visible: modelData.isDefault === true
                                        Layout.preferredWidth: 52
                                        Layout.preferredHeight: 22
                                        radius: 11
                                        color: "#DBEAFE"
                                        Text {
                                            anchors.centerIn: parent
                                            text: "默认"
                                            font.family: window.fontFamily
                                            font.pixelSize: 11
                                            color: window.brand
                                            font.weight: Font.DemiBold
                                        }
                                    }
                                }

                                MouseArea {
                                    id: devArea
                                    anchors.fill: parent
                                    hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: playerVM.setDevice(modelData.id)
                                }
                            }
                        }

                        // 空态
                        Text {
                            Layout.fillWidth: true
                            visible: !playerVM.devices || playerVM.devices.length === 0
                            text: "未发现可用设备"
                            font.family: window.fontFamily
                            font.pixelSize: 13
                            color: window.textTertiary
                            horizontalAlignment: Text.AlignHCenter
                            topPadding: 16
                            bottomPadding: 16
                        }
                    }
                }
            }

            // ---- 播放偏好 ----
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: prefCol.implicitHeight + 32
                radius: 16
                color: window.sidebarBg
                border.color: "#33FFFFFF"
                border.width: 1

                ColumnLayout {
                    id: prefCol
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 16

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        Rectangle {
                            width: 36; height: 36; radius: 10
                            color: "#FEF3C7"
                            AppIcon {
                                anchors.centerIn: parent
                                name: "settings"; size: 18
                                color: "#D97706"; strokeWidth: 2
                            }
                        }

                        Text {
                            Layout.fillWidth: true
                            text: "播放偏好"
                            font.family: window.fontFamily
                            font.pixelSize: 15
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                        }
                    }

                    // shuffle 行
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: "随机播放"
                                font.family: window.fontFamily
                                font.pixelSize: 14
                                color: window.textPrimary
                            }
                            Text {
                                text: "下一首随机选取"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        Switch {
                            checked: playerVM.shuffle
                            onToggled: playerVM.shuffle = checked
                        }
                    }

                    // repeat
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: "循环模式"
                                font.family: window.fontFamily
                                font.pixelSize: 14
                                color: window.textPrimary
                            }
                            Text {
                                text: ["关闭", "列表循环", "单曲循环"][playerVM.repeatMode]
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        RowLayout {
                            spacing: 0
                            Repeater {
                                model: [
                                    { v: 0, label: "关闭" },
                                    { v: 1, label: "列表" },
                                    { v: 2, label: "单曲" }
                                ]
                                delegate: Rectangle {
                                    Layout.preferredWidth: 56
                                    Layout.preferredHeight: 30
                                    color: playerVM.repeatMode === modelData.v ? window.brand
                                         : (segArea.containsMouse ? window.hoverBg : "transparent")
                                    border.color: window.borderColor
                                    border.width: 1
                                    radius: 0

                                    Text {
                                        anchors.centerIn: parent
                                        text: modelData.label
                                        font.family: window.fontFamily
                                        font.pixelSize: 12
                                        font.weight: Font.DemiBold
                                        color: playerVM.repeatMode === modelData.v ? "#FFFFFF" : window.textPrimary
                                    }

                                    MouseArea {
                                        id: segArea
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: playerVM.repeatMode = modelData.v
                                    }
                                }
                            }
                        }
                    }

                    // 音量
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: "默认音量"
                                font.family: window.fontFamily
                                font.pixelSize: 14
                                color: window.textPrimary
                            }
                            Text {
                                text: "应用启动时使用的音量 (" + playerVM.volume + "%)"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        Slider {
                            Layout.preferredWidth: 220
                            from: 0; to: 100
                            value: playerVM.volume
                            onMoved: playerVM.volume = Math.round(value)

                            background: Rectangle {
                                x: parent.leftPadding
                                y: parent.topPadding + parent.availableHeight / 2 - height / 2
                                width: parent.availableWidth
                                height: 4
                                radius: 2
                                color: window.borderColor

                                Rectangle {
                                    width: parent.parent.visualPosition * parent.width
                                    height: parent.height
                                    radius: parent.radius
                                    color: window.brand
                                }
                            }

                            handle: Rectangle {
                                x: parent.leftPadding + parent.visualPosition * (parent.availableWidth - width)
                                y: parent.topPadding + parent.availableHeight / 2 - height / 2
                                width: 14; height: 14; radius: 7
                                color: window.brand
                                border.color: "#FFFFFF"
                                border.width: 2
                            }
                        }
                    }
                }
            }

            // ---- 关于 ----
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: aboutCol.implicitHeight + 32
                radius: 16
                color: window.sidebarBg
                border.color: "#33FFFFFF"
                border.width: 1

                ColumnLayout {
                    id: aboutCol
                    anchors.fill: parent
                    anchors.margins: 20
                    spacing: 8

                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12

                        Rectangle {
                            width: 36; height: 36; radius: 10
                            color: "#E0E7FF"
                            AppIcon {
                                anchors.centerIn: parent
                                name: "music"; size: 18
                                color: "#4F46E5"; strokeWidth: 2
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                text: "AudioPlayerX86"
                                font.family: window.fontFamily
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                color: window.textPrimary
                            }
                            Text {
                                text: "Windows WASAPI 独占模式高保真音频播放器"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }
                    }

                    Text {
                        Layout.fillWidth: true
                        Layout.topMargin: 4
                        text: "支持 WAV(PCM/FLOAT) / FLAC,DSD 与 MP3 在后续版本中支持"
                        font.family: window.fontFamily
                        font.pixelSize: 11
                        color: window.textTertiary
                        wrapMode: Text.WordWrap
                    }

                    // 快捷键按钮
                    Rectangle {
                        Layout.topMargin: 8
                        Layout.preferredHeight: 32
                        Layout.preferredWidth: skTxt.implicitWidth + 32
                        radius: 16
                        color: skArea.containsMouse ? window.cardHover : window.sidebarBg
                        border.color: window.borderColor
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 150 } }

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: 6
                            AppIcon { name: "menu"; size: 12; color: window.textPrimary }
                            Text {
                                id: skTxt
                                text: "键盘快捷键 (F1)"
                                color: window.textPrimary
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }
                        }
                        MouseArea {
                            id: skArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: window.showShortcuts()
                        }
                    }

                    // 均衡器按钮
                    Rectangle {
                        Layout.preferredHeight: 32
                        Layout.preferredWidth: eqTxt.implicitWidth + 32
                        radius: 16
                        color: eqArea.containsMouse ? window.cardHover : window.sidebarBg
                        border.color: window.borderColor
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 150 } }

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: 6
                            AppIcon { name: "settings"; size: 12; color: window.textPrimary }
                            Text {
                                id: eqTxt
                                text: "均衡器 (Ctrl+E)"
                                color: window.textPrimary
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }
                        }
                        MouseArea {
                            id: eqArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: window.showEq()
                        }
                    }
                }
            }
        }
    }
}
