import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"

// 歌手详情 — 该歌手所有曲目列表
Item {
    id: root
    objectName: "artistDetailView"

    property string artistName: ""

    readonly property var tracks: artistName ? playerVM.tracksByArtist(artistName) : []

    // 监听 library 变化重新拉
    Connections {
        target: playerVM
        function onLibraryChanged() {
            root._tracks = root.artistName ? playerVM.tracksByArtist(root.artistName) : []
        }
    }
    property var _tracks: tracks

    TrackContextMenu { id: ctxMenu }

    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 132

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
            onClicked: window.navigateTo("artist")
        }

        RowLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 64
            anchors.rightMargin: 32
            spacing: 18

            Rectangle {
                Layout.preferredWidth: 80
                Layout.preferredHeight: 80
                radius: 40
                gradient: Gradient {
                    orientation: Gradient.Vertical
                    GradientStop { position: 0; color: "#EC4899" }
                    GradientStop { position: 1; color: "#8B5CF6" }
                }
                Text {
                    anchors.centerIn: parent
                    text: (root.artistName || "?").substring(0, 1).toUpperCase()
                    color: "#FFFFFF"
                    font.family: window.fontFamily
                    font.pixelSize: 34
                    font.weight: Font.Bold
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Text {
                    text: root.artistName
                    font.family: window.fontFamily
                    font.pixelSize: 24
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: (root._tracks ? root._tracks.length : 0) + " 首"
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
                        onClicked: playerVM.playArtist(root.artistName)
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
