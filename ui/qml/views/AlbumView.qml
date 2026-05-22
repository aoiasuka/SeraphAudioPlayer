import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import "../components/SearchUtil.js" as SearchUtil
import QtQuick.Effects

// 专辑视图 — 按 ALBUM(+ARTIST) 聚合,卡片网格
Item {
    id: root
    objectName: "albumView"

    property string searchText: ""
    property string _pendingSearch: ""
    Timer {
        id: searchDebounce
        interval: 250
        onTriggered: root.searchText = root._pendingSearch
    }

    readonly property var allAlbums: playerVM.albums || []
    readonly property var filteredAlbums:
        SearchUtil.filter(root.allAlbums, root.searchText, ["album", "artist"])

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
                    text: "专辑"
                    font.family: window.fontFamily
                    font.pixelSize: 26
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: "共 " + root.allAlbums.length + " 张"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textSecondary
                }
            }

            Rectangle {
                Layout.preferredWidth: 280
                Layout.preferredHeight: 36
                radius: 18
                color: searchBox.activeFocus ? window.surface : window.sidebarBg
                border.color: searchBox.activeFocus ? window.brand : "#33FFFFFF"
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
                        placeholderText: "搜索专辑或歌手 (支持 album:xxx / artist:xxx)"
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

    // 卡片网格
    ScrollView {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 4
        clip: true

        GridView {
            id: grid
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            anchors.topMargin: 8
            anchors.bottomMargin: 16
            model: root.filteredAlbums

            readonly property int columns: Math.max(2, Math.floor(width / 200))
            cellWidth: Math.floor(width / columns)
            cellHeight: cellWidth + 56  // 封面方形 + 文字区

            // 空态
            Item {
                anchors.centerIn: parent
                visible: grid.count === 0
                width: 240; height: 160
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 6
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 56; height: 56; radius: 28
                        color: "#E0E7FF"
                        AppIcon { anchors.centerIn: parent; name: "album"; size: 24; color: "#4F46E5" }
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: root.searchText.length > 0 ? "没有匹配的专辑" : "音乐库为空"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textSecondary
                    }
                }
            }

            delegate: Item {
                width: grid.cellWidth
                height: grid.cellHeight

                Item {
                    anchors.fill: parent
                    anchors.margins: 8

                    Rectangle {
                        id: cover
                        anchors.top: parent.top
                        anchors.left: parent.left
                        anchors.right: parent.right
                        height: width
                        radius: 12
                        clip: true
                        gradient: Gradient {
                            orientation: Gradient.Vertical
                            GradientStop { position: 0; color: index % 3 === 0 ? "#3B82F6"
                                                       : index % 3 === 1 ? "#10B981" : "#F59E0B" }
                            GradientStop { position: 1; color: index % 3 === 0 ? "#6366F1"
                                                       : index % 3 === 1 ? "#0EA5E9" : "#EF4444" }
                        }

                        Rectangle {
                            id: realCoverMask
                            width: realCover.width
                            height: realCover.height
                            radius: 12
                            color: "black"
                            antialiasing: true
                        }

                        // 真实封面(若有)
                        Image {
                            id: realCover
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
                                    sourceItem: realCoverMask
                                    hideSource: true
                                }
                            }
                        }

                        AppIcon {
                            anchors.centerIn: parent
                            visible: !realCover.visible
                            name: "album"; size: cover.width * 0.32
                            color: "#FFFFFF"; strokeWidth: 1.6
                            opacity: 0.92
                        }

                        // hover 浮出播放按钮
                        Rectangle {
                            id: playBtn
                            width: 44; height: 44; radius: 22
                            anchors.right: parent.right
                            anchors.bottom: parent.bottom
                            anchors.rightMargin: 10
                            anchors.bottomMargin: 10
                            color: window.brand
                            opacity: hover.containsMouse ? 1 : 0
                            scale: hover.containsMouse ? 1 : 0.8
                            Behavior on opacity { NumberAnimation { duration: 180 } }
                            Behavior on scale { NumberAnimation { duration: 220; easing.type: Easing.OutBack } }

                            AppIcon {
                                anchors.centerIn: parent
                                anchors.horizontalCenterOffset: 1.5
                                name: "play"; size: 16
                                color: "#FFFFFF"; filled: true
                            }

                            MouseArea {
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.playAlbum(modelData.album, modelData.artist)
                            }
                        }

                        MouseArea {
                            id: hover
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: window.openAlbum(modelData.album, modelData.artist)
                            z: -1
                        }
                    }

                    ColumnLayout {
                        anchors.top: cover.bottom
                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.topMargin: 8
                        spacing: 2

                        Text {
                            Layout.fillWidth: true
                            text: modelData.album
                            font.family: window.fontFamily
                            font.pixelSize: 13
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            text: modelData.artist || "未知歌手"
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            color: window.textSecondary
                            elide: Text.ElideRight
                        }
                    }
                }
            }
        }
    }
}
