import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import "../components"
import "../components/SearchUtil.js" as SearchUtil

// 单个歌单详情页 - 与 TrackListPage 类似但带有特定操作
Item {
    id: root
    objectName: "playlistDetailView"

    property string playlistId: ""
    readonly property var info: playlistId ? playerVM.playlistById(playlistId) : ({})
    readonly property var tracks: playlistId ? playerVM.playlistTracks(playlistId) : []

    // 监听 playlistsChanged 重新拉数据
    Connections {
        target: playerVM
        function onPlaylistsChanged() {
            root.infoBust++
            root.tracksBust++
        }
    }
    property int infoBust: 0
    property int tracksBust: 0
    // 强制重新计算
    onInfoBustChanged: refresh()
    onTracksBustChanged: refresh()
    function refresh() {
        _info = playlistId ? playerVM.playlistById(playlistId) : ({})
        _tracks = playlistId ? playerVM.playlistTracks(playlistId) : []
    }
    property var _info: info
    property var _tracks: tracks

    FileDialog {
        id: addDialog
        title: "添加曲目到歌单"
        nameFilters: ["Audio Files (*.wav *.flac *.mp3 *.dsf *.dff *.ogg)", "All Files (*)"]
        fileMode: FileDialog.OpenFiles
        onAccepted: {
            var paths = []
            for (var i = 0; i < selectedFiles.length; ++i) paths.push(selectedFiles[i])
            playerVM.addManyToPlaylist(root.playlistId, paths)
        }
    }

    TrackContextMenu {
        id: ctxMenu
    }

    // 搜索过滤
    property string searchText: ""
    property string _pendingSearch: ""
    Timer {
        id: searchDebounce
        interval: 250
        onTriggered: root.searchText = root._pendingSearch
    }
    readonly property var filteredTracks:
        SearchUtil.filter(root._tracks || [], root.searchText,
                          ["title", "artist", "album"])

    // 顶部
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 132

        // 返回按钮
        SidebarIconButton {
            id: backBtn
            anchors.top: parent.top
            anchors.left: parent.left
            anchors.topMargin: 16
            anchors.leftMargin: 24
            iconName: "chevron"
            rotation: 180
            iconColor: window.textPrimary
            implicitWidth: 32
            implicitHeight: 32
            onClicked: window.navigateTo("playlist")
        }

        RowLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 64
            anchors.rightMargin: 32
            spacing: 18

            // 封面
            Rectangle {
                Layout.preferredWidth: 80
                Layout.preferredHeight: 80
                radius: 16
                gradient: Gradient {
                    orientation: Gradient.Vertical
                    GradientStop { position: 0; color: "#3B82F6" }
                    GradientStop { position: 1; color: "#6366F1" }
                }
                AppIcon {
                    anchors.centerIn: parent
                    name: "playlist"
                    size: 36
                    color: "#FFFFFF"
                    strokeWidth: 2
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Text {
                    text: root._info && root._info.name ? root._info.name : "歌单"
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
                RowLayout {
                    spacing: 8

                    // 播放全部
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
                            onClicked: playerVM.playPlaylist(root.playlistId)
                        }
                    }

                    // 添加曲目
                    Rectangle {
                        Layout.preferredHeight: 32
                        Layout.preferredWidth: addTxt.implicitWidth + 32
                        radius: 16
                        color: addArea.containsMouse ? window.cardHover : window.sidebarBg
                        border.color: window.borderColor
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 150 } }

                        RowLayout {
                            anchors.centerIn: parent
                            spacing: 6
                            AppIcon { name: "plus"; size: 12; color: window.textPrimary }
                            Text {
                                id: addTxt
                                text: "添加"
                                color: window.textPrimary
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                            }
                        }
                        MouseArea {
                            id: addArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: addDialog.open()
                        }
                    }
                }
            }

            // 搜索框
            Rectangle {
                Layout.preferredWidth: 240
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
                        placeholderText: "在歌单中搜索 (支持 title:xxx / artist:xxx / album:xxx)"
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

    // 列表
    // 搜索状态下用 Repeater + 上下移按钮(因为搜索过滤会破坏 index 对应关系);
    // 非搜索状态下用 ReorderableTrackList 支持拖拽重排
    Item {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 8

        // 拖拽重排版本
        ReorderableTrackList {
            id: reorderList
            visible: root.searchText.length === 0
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            model: root.filteredTracks
            allowReorder: true
            showRemove: true
            contextMenu: ctxMenu

            onItemClicked: function(path) { playerVM.openFile(path) }
            onItemMoved: function(from, to) {
                playerVM.movePlaylistItem(root.playlistId, from, to)
            }
            onItemRemoved: function(index, path) {
                playerVM.removeFromPlaylist(root.playlistId, path)
            }
        }

        // 搜索时的备用渲染
        Flickable {
            visible: root.searchText.length > 0
            anchors.fill: parent
            contentWidth: width
            contentHeight: searchCol.implicitHeight + 24
            clip: true
            boundsBehavior: Flickable.StopAtBounds

            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

            ColumnLayout {
                id: searchCol
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.leftMargin: 32
                anchors.rightMargin: 32
                spacing: 2

                Item {
                    Layout.fillWidth: true
                    visible: root.filteredTracks.length === 0
                    Layout.preferredHeight: 240
                    Text {
                        anchors.centerIn: parent
                        text: "没有匹配的曲目"
                        font.family: window.fontFamily
                        font.pixelSize: 14
                        color: window.textSecondary
                    }
                }
                Repeater {
                    model: root.filteredTracks
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
                        showRemove: true
                        onClicked: playerVM.openFile(modelData.path)
                        onLikeClicked: playerVM.toggleLike(modelData.path)
                        onMoreClicked: ctxMenu.openFor(modelData.path)
                        onRemoveClicked: playerVM.removeFromPlaylist(root.playlistId, modelData.path)
                    }
                }
            }
        }

        // 空态(无搜索时,列表为空)
        Item {
            anchors.fill: parent
            visible: root.searchText.length === 0 && root.filteredTracks.length === 0
            ColumnLayout {
                anchors.centerIn: parent
                spacing: 8
                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: "歌单为空"
                    font.family: window.fontFamily
                    font.pixelSize: 14
                    color: window.textSecondary
                }
                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: "点上方「添加」加入曲目"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textTertiary
                }
            }
        }
    }
}
