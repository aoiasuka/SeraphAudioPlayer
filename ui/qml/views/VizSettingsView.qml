import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"

Item {
    id: root
    objectName: "vizSettingsView"

    // 顶部标题
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 80

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            anchors.topMargin: 16
            spacing: 16

            Rectangle {
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36
                radius: 18
                color: backArea.containsMouse ? window.cardHover : "transparent"
                border.color: window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                Text {
                    anchors.centerIn: parent
                    text: "←"
                    font.family: window.fontFamily
                    font.pixelSize: 16
                    color: window.textPrimary
                }

                MouseArea {
                    id: backArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: window.navigateTo("settings")
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4

                Text {
                    text: "频谱效果设置"
                    font.family: window.fontFamily
                    font.pixelSize: 26
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: "预览并选择你喜欢的可视化风格"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    color: window.textSecondary
                }
            }
        }
    }

    // 预览区域
    Rectangle {
        id: previewArea
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 32
        height: 240
        radius: 16
        color: window.surfaceAlt
        border.color: window.borderColor
        border.width: 1

        // 复用 SpectrumView
        SpectrumView {
            anchors.fill: parent
            anchors.margins: 20
        }
        
        Text {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.margins: 16
            text: "实时预览"
            font.family: window.fontFamily
            font.pixelSize: 12
            font.weight: Font.DemiBold
            color: window.textTertiary
        }
    }

    // 选项列表
    GridLayout {
        anchors.top: previewArea.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 32
        anchors.topMargin: 24
        columns: 2
        columnSpacing: 16
        rowSpacing: 16

        Repeater {
            model: [
                { v: 0, label: "经典柱状", desc: "16段向上的能量柱" },
                { v: 1, label: "镜像对称", desc: "从中心向两端生长的能量柱" },
                { v: 2, label: "平滑曲线", desc: "丝滑连续的单向波形" },
                { v: 3, label: "对称波浪", desc: "上下起伏的连续声波" }
            ]

            delegate: Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 80
                radius: 12
                color: playerVM.visualizerType === modelData.v ? window.activeBg
                     : (optArea.containsMouse ? window.hoverBg : "transparent")
                border.color: playerVM.visualizerType === modelData.v ? window.brand : window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.fill: parent
                    anchors.margins: 16
                    spacing: 16

                    Rectangle {
                        width: 18; height: 18; radius: 9
                        color: "transparent"
                        border.color: playerVM.visualizerType === modelData.v ? window.brand : window.textTertiary
                        border.width: 2

                        Rectangle {
                            anchors.centerIn: parent
                            width: 8; height: 8; radius: 4
                            color: window.brand
                            visible: playerVM.visualizerType === modelData.v
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 4

                        Text {
                            text: modelData.label
                            font.family: window.fontFamily
                            font.pixelSize: 15
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                        }
                        Text {
                            text: modelData.desc
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            color: window.textSecondary
                        }
                    }
                }

                MouseArea {
                    id: optArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: playerVM.visualizerType = modelData.v
                }
            }
        }
    }
}
