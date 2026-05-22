import QtQuick
import QtQuick.Layouts

// 推荐歌单卡片：封面 + 圆形播放按钮 hover + 标题 + 数量
Item {
    id: root

    property color coverColor: "transparent"   // 封面占位色 (留白)
    property string title: ""
    property string subtitle: ""

    signal clicked()

    ColumnLayout {
        anchors.fill: parent
        spacing: 12

        // 封面容器 (悬浮放大效果)
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: width

            Rectangle {
                id: cover
                anchors.centerIn: parent
                width: coverArea.containsMouse ? parent.width * 1.04 : parent.width
                height: width
                radius: 12
                color: root.coverColor
                border.color: "#33000000"
                border.width: 1
                clip: true

                Behavior on width { NumberAnimation { duration: 250; easing.type: Easing.OutQuart } }

                // 渐变叠加增加层次
                Rectangle {
                    anchors.fill: parent
                    radius: parent.radius
                    gradient: Gradient {
                        GradientStop { position: 0; color: "transparent" }
                        GradientStop { position: 1; color: "#66000000" }
                    }
                }

                // hover 时浮出的播放按钮
                Rectangle {
                    id: playBtn
                    width: 44; height: 44; radius: 22
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom
                    anchors.rightMargin: 12
                    anchors.bottomMargin: 12
                    color: window.brand
                    opacity: coverArea.containsMouse ? 1 : 0
                    scale: coverArea.containsMouse ? 1 : 0.8
                    Behavior on opacity { NumberAnimation { duration: 200 } }
                    Behavior on scale { NumberAnimation { duration: 250; easing.type: Easing.OutBack } }

                    AppIcon {
                        anchors.centerIn: parent
                        anchors.horizontalCenterOffset: 1.5   // 视觉补偿三角形
                        name: "play"
                        size: 16
                        color: "#FFFFFF"
                        filled: true
                    }
                }

                MouseArea {
                    id: coverArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.clicked()
                }
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            
            Text {
                Layout.fillWidth: true
                text: root.title
                font.family: window.fontFamily
                font.pixelSize: 15
                font.weight: Font.DemiBold
                color: window.textPrimary
                elide: Text.ElideRight
            }

            Text {
                Layout.fillWidth: true
                text: root.subtitle
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
                elide: Text.ElideRight
            }
        }
    }
}
