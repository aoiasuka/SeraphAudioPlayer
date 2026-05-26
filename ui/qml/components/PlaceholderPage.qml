import QtQuick
import QtQuick.Layouts

// 即将上线占位页 — 用于歌单 / 歌手 / 专辑
Item {
    id: root
    objectName: "placeholderView"

    property string pageTitle: "即将上线"
    property string pageMessage: "此功能正在开发中"
    property string pageIcon: "playlist"

    // 顶部
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 80

        Text {
            anchors.left: parent.left
            anchors.leftMargin: 32
            anchors.verticalCenter: parent.verticalCenter
            text: root.pageTitle
            font.family: window.fontFamily
            font.pixelSize: 26
            font.weight: Font.Bold
            color: window.textPrimary
        }
    }

    // 中心占位卡片
    Rectangle {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 32
        anchors.topMargin: 0
        radius: 16
        color: window.sidebarBg
        border.color: window.borderColor
        border.width: 1

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 16

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                width: 96; height: 96; radius: 48
                color: "#DBEAFE"

                AppIcon {
                    anchors.centerIn: parent
                    name: root.pageIcon
                    size: 44
                    color: window.brand
                    strokeWidth: 1.6
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "即将上线"
                font.family: window.fontFamily
                font.pixelSize: 22
                font.weight: Font.Bold
                color: window.textPrimary
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                Layout.maximumWidth: 400
                text: root.pageMessage
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
            }
        }
    }
}
