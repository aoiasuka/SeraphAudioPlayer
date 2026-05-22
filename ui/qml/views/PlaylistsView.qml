import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"

// 歌单列表页 — 显示所有用户歌单
Item {
    id: root
    objectName: "playlistsView"

    Dialog {
        id: createDialog
        modal: true
        anchors.centerIn: parent
        width: 360
        title: "新建歌单"
        standardButtons: Dialog.Ok | Dialog.Cancel

        contentItem: ColumnLayout {
            spacing: 8
            Text {
                text: "歌单名称"
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
            }
            TextField {
                id: nameField
                Layout.fillWidth: true
                placeholderText: "例如:晚间放松"
                font.family: window.fontFamily
                font.pixelSize: 14
                text: ""
            }
        }
        onAccepted: {
            playerVM.createPlaylist(nameField.text)
            nameField.text = ""
        }
        onRejected: nameField.text = ""
    }

    Dialog {
        id: renameDialog
        modal: true
        anchors.centerIn: parent
        width: 360
        title: "重命名歌单"
        standardButtons: Dialog.Ok | Dialog.Cancel

        property string targetId: ""

        contentItem: ColumnLayout {
            spacing: 8
            Text {
                text: "新名称"
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
            }
            TextField {
                id: renameField
                Layout.fillWidth: true
                font.family: window.fontFamily
                font.pixelSize: 14
            }
        }
        onAccepted: {
            playerVM.renamePlaylist(renameDialog.targetId, renameField.text)
        }

        function openFor(id, name) {
            targetId = id
            renameField.text = name
            open()
        }
    }

    // 顶部
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 84

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            anchors.topMargin: 16

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Text {
                    text: "歌单"
                    font.family: window.fontFamily
                    font.pixelSize: 26
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: "共 " + (playerVM.playlists ? playerVM.playlists.length : 0) + " 个"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textSecondary
                }
            }

            // 新建按钮
            Rectangle {
                Layout.preferredWidth: 110
                Layout.preferredHeight: 36
                radius: 18
                color: createArea.pressed ? window.brandPress
                     : (createArea.containsMouse ? window.brandHover : window.brand)
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.centerIn: parent
                    spacing: 6
                    AppIcon { name: "plus"; size: 14; color: "#FFFFFF" }
                    Text {
                        text: "新建歌单"
                        color: "#FFFFFF"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                    }
                }

                MouseArea {
                    id: createArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: createDialog.open()
                }
            }
        }
    }

    // 列表 / 空态
    Flickable {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 4
        contentWidth: width
        contentHeight: gridCol.implicitHeight + 32
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

        ColumnLayout {
            id: gridCol
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            spacing: 8

            // 空态
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 280
                visible: !playerVM.playlists || playerVM.playlists.length === 0
                radius: 16
                color: window.sidebarBg
                border.color: "#33FFFFFF"
                border.width: 1

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 12

                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 72; height: 72; radius: 36
                        color: "#DBEAFE"
                        AppIcon {
                            anchors.centerIn: parent
                            name: "playlist"; size: 32
                            color: window.brand
                        }
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: "还没有歌单"
                        font.family: window.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                        color: window.textPrimary
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: "点右上方「新建歌单」开始"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textSecondary
                    }
                }
            }

            Repeater {
                model: playerVM.playlists

                delegate: Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 80
                    radius: 14
                    color: rowArea.containsMouse ? window.cardHover : window.sidebarBg
                    border.color: rowArea.containsMouse ? window.brandSoft : "#33FFFFFF"
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 16
                        anchors.rightMargin: 16
                        spacing: 14

                        Rectangle {
                            Layout.preferredWidth: 56
                            Layout.preferredHeight: 56
                            radius: 12
                            gradient: Gradient {
                                orientation: Gradient.Vertical
                                GradientStop { position: 0; color: "#3B82F6" }
                                GradientStop { position: 1; color: "#6366F1" }
                            }

                            AppIcon {
                                anchors.centerIn: parent
                                name: "playlist"
                                size: 26
                                color: "#FFFFFF"
                                strokeWidth: 2
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4
                            Text {
                                Layout.fillWidth: true
                                text: modelData.name
                                font.family: window.fontFamily
                                font.pixelSize: 16
                                font.weight: Font.DemiBold
                                color: window.textPrimary
                                elide: Text.ElideRight
                            }
                            Text {
                                Layout.fillWidth: true
                                text: modelData.count + " 首"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        // 操作按钮
                        SidebarIconButton {
                            iconName: "play"; iconSize: 14
                            implicitWidth: 32; implicitHeight: 32
                            iconColor: window.brand
                            onClicked: playerVM.playPlaylist(modelData.id)
                        }
                        SidebarIconButton {
                            iconName: "more"; iconSize: 16
                            implicitWidth: 32; implicitHeight: 32
                            onClicked: playlistMenu.openFor(modelData.id, modelData.name)
                        }
                    }

                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: window.openPlaylist(modelData.id)
                        z: -1
                    }
                }
            }
        }
    }

    Menu {
        id: playlistMenu
        property string targetId: ""
        property string targetName: ""

        function openFor(id, name) {
            targetId = id
            targetName = name
            popup()
        }

        MenuItem {
            text: "打开"
            onTriggered: window.openPlaylist(playlistMenu.targetId)
        }
        MenuItem {
            text: "立即播放"
            onTriggered: playerVM.playPlaylist(playlistMenu.targetId)
        }
        MenuItem {
            text: "加入队列"
            onTriggered: playerVM.enqueuePlaylist(playlistMenu.targetId)
        }
        MenuSeparator {}
        MenuItem {
            text: "重命名..."
            onTriggered: renameDialog.openFor(playlistMenu.targetId, playlistMenu.targetName)
        }
        MenuItem {
            text: "删除"
            onTriggered: playerVM.deletePlaylist(playlistMenu.targetId)
        }
    }
}
