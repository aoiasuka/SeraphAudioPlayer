import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"

// 专辑详情
Item {
    id: root
    objectName: "albumDetailView"

    property string albumName: ""
    property string artistName: ""

    readonly property var tracks: albumName ? playerVM.tracksByAlbum(albumName, artistName) : []

    Connections {
        target: playerVM
        function onLibraryChanged() {
            root._tracks = root.albumName ? playerVM.tracksByAlbum(root.albumName, root.artistName) : []
        }
    }
    property var _tracks: tracks

    TrackContextMenu { id: ctxMenu }

    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 152

        SidebarIconButton {
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.topMargin: 16
            anchors.leftMargin: 24
            iconName: "chevron"
            rotation: 180
            iconColor: window.textPrimary
            implicitWidth: 32
            implicitHeight: 32
            onClicked: window.navigateTo("album")
        }

        RowLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 64
            anchors.rightMargin: 32
            spacing: 20

            Rectangle {
                Layout.preferredWidth: 100
                Layout.preferredHeight: 100
                radius: 16
                gradient: Gradient {
                    orientation: Gradient.Vertical
                    GradientStop { position: 0; color: "#3B82F6" }
                    GradientStop { position: 1; color: "#6366F1" }
                }
                AppIcon {
                    anchors.centerIn: parent
                    name: "album"; size: 44
                    color: "#FFFFFF"; strokeWidth: 1.6
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6
                Text {
                    text: "专辑"
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    font.weight: Font.DemiBold
                    color: window.textTertiary
                }
                Text {
                    text: root.albumName
                    font.family: window.fontFamily
                    font.pixelSize: 24
                    font.weight: Font.Bold
                    color: window.textPrimary
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                Text {
                    text: (root.artistName || "未知歌手") + " · " + (root._tracks ? root._tracks.length : 0) + " 首"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    color: window.textSecondary
                }
                Item { Layout.preferredHeight: 4 }
                Rectangle {
                    Layout.preferredHeight: 32
                    Layout.preferredWidth: playTxt.implicitWidth + 32
                    radius: 16
                    color: playArea.pressed ? window.brandPress
                         : (playArea.containsMouse ? window.brandHover : window.brand)
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        AppIcon { name: "play"; size: 12; color: "#FFFFFF"; filled: true }
                        Text {
                            id: playTxt
                            text: "播放全部"
                            color: "#FFFFFF"
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                        }
                    }
                    MouseArea {
                        id: playArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: playerVM.playAlbum(root.albumName, root.artistName)
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
        anchors.topMargin: 8
        contentWidth: width
        contentHeight: bodyCol.implicitHeight + 24
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

        ColumnLayout {
            id: bodyCol
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            spacing: 2

            Repeater {
                model: root._tracks
                delegate: TrackRow {
                    Layout.fillWidth: true
                    title: modelData.title
                    artist: modelData.artist
                    album: modelData.album
                    duration: modelData.duration || ""
                    liked: modelData.liked === true
                    isCurrent: modelData.isCurrent === true
                    coverUrl: modelData.coverUrl || ""
                    path: modelData.path
                    onClicked: playerVM.openFile(modelData.path)
                    onLikeClicked: playerVM.toggleLike(modelData.path)
                    onEnqueueClicked: playerVM.enqueue(modelData.path)
                    onMoreClicked: ctxMenu.openFor(modelData.path)
                }
            }
        }
    }
}
