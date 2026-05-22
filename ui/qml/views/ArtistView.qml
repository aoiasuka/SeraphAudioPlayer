import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import "../components/SearchUtil.js" as SearchUtil
import QtQuick.Effects

// 歌手列表 — 按 ARTIST 聚合
Item {
    id: root
    objectName: "artistView"

    property string searchText: ""
    property string _pendingSearch: ""
    Timer {
        id: searchDebounce
        interval: 250
        onTriggered: root.searchText = root._pendingSearch
    }

    readonly property var artists: playerVM.artists || []
    readonly property var filteredArtists:
        SearchUtil.filter(root.artists, root.searchText, ["name"])

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
                    text: "歌手"
                    font.family: window.fontFamily
                    font.pixelSize: 26
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: "共 " + root.artists.length + " 位"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textSecondary
                }
            }

            // 搜索框
            Rectangle {
                Layout.preferredWidth: 280
                Layout.preferredHeight: 36
                radius: 18
                color: searchBox.activeFocus ? window.surface : window.sidebarBg
                border.color: searchBox.activeFocus ? window.brand : window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 14
                    anchors.rightMargin: 14
                    spacing: 8

                    AppIcon { name: "search"; size: 14; color: window.textSecondary }

                    TextField {
                        id: searchBox
                        Layout.fillWidth: true
                        placeholderText: "搜索歌手"
                        placeholderTextColor: window.textTertiary
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textPrimary
                        background: null
                        verticalAlignment: TextInput.AlignVCenter
                        onTextChanged: {
                            root._pendingSearch = text
                            searchDebounce.restart()
                        }
                    }
                }
            }
        }
    }

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
                Layout.preferredHeight: 240
                visible: root.filteredArtists.length === 0
                radius: 16
                color: window.sidebarBg
                border.color: window.borderColor
                border.width: 1

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 8
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 64; height: 64; radius: 32
                        color: "#FCE7F3"
                        AppIcon { anchors.centerIn: parent; name: "artist"; size: 28; color: "#DB2777" }
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: root.searchText.length > 0 ? "没有匹配的歌手" : "音乐库为空"
                        font.family: window.fontFamily
                        font.pixelSize: 14
                        color: window.textSecondary
                    }
                }
            }

            Repeater {
                model: root.filteredArtists

                delegate: Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 72
                    radius: 14
                    color: rowArea.containsMouse ? window.cardHover : window.sidebarBg
                    border.color: rowArea.containsMouse ? window.brandSoft : window.borderColor
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 16
                        anchors.rightMargin: 16
                        spacing: 14

                        Rectangle {
                            Layout.preferredWidth: 48
                            Layout.preferredHeight: 48
                            radius: 24
                            clip: true
                            gradient: Gradient {
                                orientation: Gradient.Vertical
                                GradientStop { position: 0; color: "#EC4899" }
                                GradientStop { position: 1; color: "#8B5CF6" }
                            }

                            Rectangle {
                                id: artistCoverMask
                                width: artistCover.width
                                height: artistCover.height
                                radius: 24
                                color: "black"
                                antialiasing: true
                            }

                            Image {
                                id: artistCover
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
                                        sourceItem: artistCoverMask
                                        hideSource: true
                                    }
                                }
                            }

                            Text {
                                anchors.centerIn: parent
                                visible: !artistCover.visible
                                text: (modelData.name || "?").substring(0, 1).toUpperCase()
                                color: "#FFFFFF"
                                font.family: window.fontFamily
                                font.pixelSize: 22
                                font.weight: Font.Bold
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 4
                            Text {
                                Layout.fillWidth: true
                                text: modelData.name
                                font.family: window.fontFamily
                                font.pixelSize: 15
                                font.weight: Font.DemiBold
                                color: window.textPrimary
                                elide: Text.ElideRight
                            }
                            Text {
                                text: modelData.count + " 首"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textSecondary
                            }
                        }

                        SidebarIconButton {
                            iconName: "play"; iconSize: 14
                            implicitWidth: 32; implicitHeight: 32
                            iconColor: window.brand
                            onClicked: playerVM.playArtist(modelData.name)
                        }
                    }

                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: window.openArtist(modelData.name)
                        z: -1
                    }
                }
            }
        }
    }
}
