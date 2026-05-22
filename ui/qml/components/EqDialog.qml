import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 10 段均衡器对话框 — 毛玻璃容器，主题化控件，无传统灰色底栏
Dialog {
    id: root
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 720, 760)
    height: 460
    padding: 0

    // 移除默认 Close 按钮 (改用右上角圆形关闭)
    standardButtons: Dialog.NoButton

    readonly property var labels: ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"]
    readonly property var gains: playerVM.eqGains || []

    // 弹窗外的暗色遮罩 — 压暗底部内容, 让焦点集中到弹窗
    Overlay.modal: Rectangle {
        color: window.modalScrim
        Behavior on opacity { NumberAnimation { duration: 180 } }
    }

    // 毛玻璃弹窗容器 (高不透明浅色, 杜绝下层文字穿透)
    background: Rectangle {
        radius: window.largeRadius
        color: window.glassBg
        border.color: window.glassBorderDark
        border.width: 1
        antialiasing: true

        // 微弱阴影描边
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

    // 隐藏默认标题与底部按钮栏 (避免出现灰色底栏)
    header: Item { visible: false; height: 0 }
    footer: Item { visible: false; height: 0 }

    contentItem: Item {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 16

            // 标题栏 + 右上角圆形关闭
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: "均衡器"
                    font.family: window.fontFamily
                    font.pixelSize: 18
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Item { Layout.fillWidth: true }

                // 圆形关闭按钮 (替代底部 Close)
                Item {
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30

                    Rectangle {
                        anchors.fill: parent
                        radius: 15
                        color: closeArea.containsMouse ? "#33000000" : "transparent"
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
                        id: closeArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.close()
                    }
                }
            }

            // 开关 + 预设 + 重置
            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                // ===== 自定义 Switch =====
                Switch {
                    id: eqSwitch
                    checked: playerVM.eqEnabled
                    onToggled: playerVM.eqEnabled = checked
                    padding: 0
                    implicitWidth: indicator.implicitWidth + label.implicitWidth + 8
                    implicitHeight: 24

                    indicator: Rectangle {
                        implicitWidth: 40
                        implicitHeight: 22
                        x: 0
                        y: (eqSwitch.height - height) / 2
                        radius: 11
                        color: eqSwitch.checked ? window.brand : "#55000000"
                        border.color: eqSwitch.checked ? window.brand : window.glassBorder
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 150 } }

                        Rectangle {
                            x: eqSwitch.checked ? parent.width - width - 2 : 2
                            y: 2
                            width: 18
                            height: 18
                            radius: 9
                            color: "#FFFFFF"
                            Behavior on x { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }
                        }
                    }
                    contentItem: Text {
                        id: label
                        leftPadding: eqSwitch.indicator.width + 8
                        text: "启用"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textPrimary
                        verticalAlignment: Text.AlignVCenter
                    }
                }

                Item { Layout.fillWidth: true }

                // ===== 自定义 ComboBox =====
                ComboBox {
                    id: presets
                    model: ["自定义", "扁平", "重低音", "舞曲", "古典", "摇滚", "人声"]
                    onActivated: applyPreset(currentIndex)
                    Layout.preferredWidth: 120
                    Layout.preferredHeight: 32

                    function applyPreset(idx) {
                        var presets = [
                            null,
                            [0,0,0,0,0,0,0,0,0,0],
                            [6,5,3,1,0,0,0,0,0,0],
                            [4,3,1,0,-1,0,2,4,4,3],
                            [3,2,0,0,0,0,-1,-1,0,2],
                            [4,3,2,0,-1,-1,0,2,3,3],
                            [-2,-1,0,2,3,3,2,1,0,-1]
                        ]
                        var p = presets[idx]
                        if (!p) return
                        for (var i = 0; i < p.length; ++i) {
                            playerVM.setEqGain(i, p[i])
                        }
                    }

                    background: Rectangle {
                        radius: window.smallRadius
                        color: presets.hovered ? "#22FFFFFF" : "#11FFFFFF"
                        border.color: window.glassBorder
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    contentItem: Text {
                        leftPadding: 12
                        rightPadding: 28
                        text: presets.displayText
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textPrimary
                        verticalAlignment: Text.AlignVCenter
                        elide: Text.ElideRight
                    }
                    indicator: AppIcon {
                        x: presets.width - width - 10
                        y: presets.topPadding + (presets.availableHeight - height) / 2
                        name: "chevron"
                        rotation: 90
                        size: 12
                        color: window.textSecondary
                        strokeWidth: 2
                    }

                    // 下拉弹层风格统一为深色毛玻璃，与右键菜单一致
                    popup: Popup {
                        y: presets.height + 4
                        width: presets.width
                        implicitHeight: contentItem.implicitHeight + 12
                        padding: 6

                        background: Rectangle {
                            radius: window.mediumRadius
                            color: window.menuBg
                            border.color: window.glassBorder
                            border.width: 1
                        }
                        contentItem: ListView {
                            clip: true
                            implicitHeight: contentHeight
                            model: presets.popup.visible ? presets.delegateModel : null
                            currentIndex: presets.highlightedIndex
                        }
                    }
                    delegate: ItemDelegate {
                        width: presets.width - 12
                        height: 30
                        contentItem: Text {
                            text: modelData
                            font.family: window.fontFamily
                            font.pixelSize: 13
                            color: window.textPrimary
                            verticalAlignment: Text.AlignVCenter
                            leftPadding: 8
                        }
                        background: Rectangle {
                            radius: window.smallRadius
                            color: highlighted ? window.menuHoverBg : "transparent"
                        }
                        highlighted: presets.highlightedIndex === index
                    }
                }

                // 重置 (主题化胶囊按钮)
                Rectangle {
                    Layout.preferredHeight: 32
                    Layout.preferredWidth: resetTxt.implicitWidth + 28
                    radius: 16
                    color: resetArea.containsMouse ? "#33EF4444" : "transparent"
                    border.color: "#66EF4444"
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    Text {
                        id: resetTxt
                        anchors.centerIn: parent
                        text: "重置"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: "#DC2626"
                    }
                    MouseArea {
                        id: resetArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            playerVM.resetEq()
                            presets.currentIndex = 1
                        }
                    }
                }
            }

            // 分隔线
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: window.glassBorder
            }

            // 频段滑块
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 0

                Repeater {
                    model: root.labels.length
                    delegate: ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        Layout.minimumWidth: dbLabelMetrics.width
                        spacing: 6

                        TextMetrics {
                            id: dbLabelMetrics
                            font.family: window.fontFamily
                            font.pixelSize: 11
                            text: "-12.0 dB"
                        }

                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            Layout.preferredWidth: dbLabelMetrics.width
                            horizontalAlignment: Text.AlignHCenter
                            text: (root.gains[index] !== undefined ? root.gains[index].toFixed(1) : "0.0") + " dB"
                            font.family: window.fontFamily
                            font.pixelSize: 11
                            color: window.textSecondary
                        }

                        Slider {
                            id: slider
                            Layout.alignment: Qt.AlignHCenter
                            Layout.fillHeight: true
                            Layout.preferredWidth: 28
                            orientation: Qt.Vertical
                            from: -12; to: 12
                            value: (root.gains[index] !== undefined ? root.gains[index] : 0)
                            stepSize: 0.5
                            onMoved: {
                                playerVM.setEqGain(index, value)
                                presets.currentIndex = 0
                            }

                            background: Item {
                                x: slider.leftPadding
                                y: slider.topPadding
                                width: slider.width - slider.leftPadding - slider.rightPadding
                                height: slider.height - slider.topPadding - slider.bottomPadding

                                // 纤细轨道
                                Rectangle {
                                    width: 3
                                    radius: 1.5
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    anchors.top: parent.top
                                    anchors.bottom: parent.bottom
                                    color: "#33000000"
                                }
                                // 已偏移量高亮 (从中心到 handle)
                                Rectangle {
                                    width: 3
                                    radius: 1.5
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    y: Math.min(parent.height / 2, slider.visualPosition * (parent.height - 0))
                                    height: Math.abs(parent.height / 2 - slider.visualPosition * parent.height)
                                    color: window.brand
                                    opacity: 0.7
                                }
                                // 0 dB 标线
                                Rectangle {
                                    width: 12; height: 1
                                    color: window.textTertiary
                                    anchors.horizontalCenter: parent.horizontalCenter
                                    anchors.verticalCenter: parent.verticalCenter
                                    opacity: 0.5
                                }
                            }

                            handle: Rectangle {
                                x: slider.leftPadding + (slider.availableWidth - width) / 2
                                y: slider.topPadding + slider.visualPosition * (slider.availableHeight - height)
                                width: 14; height: 14; radius: 7
                                color: window.brand
                                border.color: "#FFFFFF"
                                border.width: 2
                            }
                        }

                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            text: root.labels[index]
                            font.family: window.fontFamily
                            font.pixelSize: 11
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                        }
                    }
                }
            }
        }
    }
}
