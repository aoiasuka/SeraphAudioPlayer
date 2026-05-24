import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects
import "../components"
import "../components/SearchUtil.js" as SearchUtil

// 全局搜索结果视图 — 三段式: 歌曲 / 专辑 / 歌手
// 由 window.openSearch(query) 触发, 也允许用户在顶部框继续修改 query
Item {
    id: root
    objectName: "searchResultsView"

    // 入参: 初始 query (在 onCompleted 时同步到 searchBox.text)
    property string query: ""

    // 实际生效的搜索关键字 (经防抖)
    property string activeQuery: query
    property string _pendingSearch: query
    Timer {
        id: searchDebounce
        interval: 200
        onTriggered: root.activeQuery = root._pendingSearch
    }

    Component.onCompleted: {
        // 同步 query → 输入框 + 立即触发
        searchBox.text = root.query || ""
        searchBox.forceActiveFocus()
        searchBox.cursorPosition = searchBox.text.length
    }

    TrackContextMenu { id: ctxMenu }

    // 派生: 歌曲 (C++ 后端, 支持权重排序, 跨越整个 library)
    readonly property var trackHits:
        playerVM.searchTracks(root.activeQuery, 200)

    // 派生: 专辑 / 歌手 (复用 JS SearchUtil 在已聚合的派生集合上过滤)
    readonly property var albumHits:
        SearchUtil.filter(playerVM.albums || [], root.activeQuery, ["album", "artist"])
    readonly property var artistHits:
        SearchUtil.filter(playerVM.artists || [], root.activeQuery, ["name"])

    readonly property bool hasAny:
        trackHits.length > 0 || albumHits.length > 0 || artistHits.length > 0

    // ===== 顶部: 返回 + 搜索输入 =====
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 84

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 24
            anchors.rightMargin: 32
            anchors.topMargin: 16
            anchors.bottomMargin: 16
            spacing: 12

            SidebarIconButton {
                iconName: "chevron"
                iconSize: 16
                iconColor: window.textPrimary
                implicitWidth: 34
                implicitHeight: 34
                rotation: 180
                onClicked: window.navigateTo("home")
            }

            // 搜索框 (与 HomeView 同款胶囊毛玻璃)
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                Layout.maximumWidth: 560
                radius: 22
                color: searchBox.activeFocus ? window.surface : window.sidebarBg
                border.color: searchBox.activeFocus ? window.brand : window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 16
                    anchors.rightMargin: 16
                    spacing: 8

                    AppIcon { name: "search"; size: 16; color: window.textSecondary; strokeWidth: 2 }

                    TextField {
                        id: searchBox
                        Layout.fillWidth: true
                        placeholderText: "搜索歌曲、歌手或专辑 (artist:xxx / 七里香 周杰伦)"
                        placeholderTextColor: window.textTertiary
                        font.family: window.fontFamily
                        font.pixelSize: 14
                        color: window.textPrimary
                        background: null
                        verticalAlignment: TextInput.AlignVCenter
                        onTextChanged: {
                            root._pendingSearch = text
                            searchDebounce.restart()
                        }
                        Keys.onEscapePressed: window.navigateTo("home")
                    }

                    Item {
                        Layout.preferredWidth: 24
                        Layout.preferredHeight: 24
                        visible: searchBox.text.length > 0
                        AppIcon {
                            anchors.centerIn: parent
                            name: "close"; size: 12
                            color: clearArea.containsMouse ? window.textPrimary : window.textTertiary
                            strokeWidth: 2
                        }
                        MouseArea {
                            id: clearArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: { searchBox.text = ""; searchBox.forceActiveFocus() }
                        }
                    }
                }
            }

            Item { Layout.fillWidth: true }

            Text {
                text: root.activeQuery.length === 0 ? ""
                     : (root.trackHits.length + root.albumHits.length + root.artistHits.length) + " 项结果"
                font.family: window.fontFamily
                font.pixelSize: 12
                color: window.textTertiary
            }
        }
    }

    // ===== 主体 =====
    ScrollView {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        clip: true

        ColumnLayout {
            width: root.width
            spacing: 24

            // ---- 空态 ----
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: 240
                visible: root.activeQuery.length > 0 && !root.hasAny
                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 10
                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 72; height: 72; radius: 36
                        color: window.sidebarBg
                        border.color: window.borderColor
                        border.width: 1
                        AppIcon {
                            anchors.centerIn: parent
                            name: "search"; size: 28
                            color: window.textTertiary; strokeWidth: 1.6
                        }
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: "没有匹配的结果"
                        font.family: window.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.DemiBold
                        color: window.textSecondary
                    }
                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: "试试更短的关键字, 或使用 artist:xxx / album:xxx 精确搜"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textTertiary
                    }
                }
            }

            // ---- 段: 歌手 ----
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 32
                Layout.rightMargin: 32
                Layout.topMargin: 12
                spacing: 12
                visible: root.artistHits.length > 0

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "歌手"
                        font.family: window.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.Bold
                        color: window.textPrimary
                    }
                    Text {
                        text: "(" + root.artistHits.length + ")"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textTertiary
                    }
                    Item { Layout.fillWidth: true }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    Repeater {
                        model: Math.min(root.artistHits.length, 6)
                        delegate: Item {
                            implicitWidth: 110
                            implicitHeight: 140

                            property var item: root.artistHits[index]

                            Rectangle {
                                anchors.fill: parent
                                radius: window.mediumRadius
                                color: artistArea.containsMouse ? window.hoverBg : "transparent"
                                Behavior on color { ColorAnimation { duration: 150 } }
                            }

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 8
                                spacing: 6

                                Rectangle {
                                    Layout.alignment: Qt.AlignHCenter
                                    Layout.preferredWidth: 80
                                    Layout.preferredHeight: 80
                                    radius: 40
                                    color: window.brandSoft
                                    AppIcon {
                                        anchors.centerIn: parent
                                        name: "artist"; size: 32
                                        color: window.brand; strokeWidth: 1.6
                                    }
                                }
                                Text {
                                    Layout.alignment: Qt.AlignHCenter
                                    Layout.fillWidth: true
                                    horizontalAlignment: Text.AlignHCenter
                                    text: parent.parent.item ? (parent.parent.item.name || "") : ""
                                    font.family: window.fontFamily
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    color: window.textPrimary
                                    elide: Text.ElideRight
                                }
                                Text {
                                    Layout.alignment: Qt.AlignHCenter
                                    Layout.fillWidth: true
                                    horizontalAlignment: Text.AlignHCenter
                                    text: parent.parent.item ? (parent.parent.item.count + " 首") : ""
                                    font.family: window.fontFamily
                                    font.pixelSize: 11
                                    color: window.textTertiary
                                }
                            }

                            MouseArea {
                                id: artistArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    if (parent.item) window.openArtist(parent.item.name)
                                }
                            }
                        }
                    }
                    Item { Layout.fillWidth: true }
                }
            }

            // ---- 段: 专辑 ----
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 32
                Layout.rightMargin: 32
                spacing: 12
                visible: root.albumHits.length > 0

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "专辑"
                        font.family: window.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.Bold
                        color: window.textPrimary
                    }
                    Text {
                        text: "(" + root.albumHits.length + ")"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textTertiary
                    }
                    Item { Layout.fillWidth: true }
                }

                RowLayout {
                    Layout.fillWidth: true
                    spacing: 16
                    Repeater {
                        model: Math.min(root.albumHits.length, 6)
                        delegate: Item {
                            implicitWidth: 140
                            implicitHeight: 180

                            property var item: root.albumHits[index]

                            Rectangle {
                                anchors.fill: parent
                                radius: window.mediumRadius
                                color: albumArea.containsMouse ? window.hoverBg : "transparent"
                                Behavior on color { ColorAnimation { duration: 150 } }
                            }

                            ColumnLayout {
                                anchors.fill: parent
                                anchors.margins: 8
                                spacing: 6

                                // 封面
                                Item {
                                    Layout.preferredWidth: 124
                                    Layout.preferredHeight: 124
                                    Layout.alignment: Qt.AlignHCenter
                                    clip: true

                                    Rectangle {
                                        anchors.fill: parent
                                        radius: window.smallRadius
                                        gradient: Gradient {
                                            orientation: Gradient.Vertical
                                            GradientStop { position: 0; color: index % 3 === 0 ? window.brand
                                                                       : index % 3 === 1 ? "#10B981" : "#F59E0B" }
                                            GradientStop { position: 1; color: index % 3 === 0 ? "#6366F1"
                                                                       : index % 3 === 1 ? "#0EA5E9" : "#EF4444" }
                                        }
                                    }

                                    Rectangle {
                                        id: alCoverMask
                                        width: alCover.width
                                        height: alCover.height
                                        radius: window.smallRadius
                                        color: "black"
                                        antialiasing: true
                                    }

                                    Image {
                                        id: alCover
                                        anchors.fill: parent
                                        source: parent.parent.parent.item ? (parent.parent.parent.item.coverUrl || "") : ""
                                        visible: source.toString().length > 0 && status === Image.Ready
                                        fillMode: Image.PreserveAspectCrop
                                        asynchronous: true
                                        cache: true

                                        layer.enabled: true
                                        layer.effect: MultiEffect {
                                            maskEnabled: true
                                            maskSource: ShaderEffectSource {
                                                sourceItem: alCoverMask
                                                hideSource: true
                                            }
                                        }
                                    }

                                    AppIcon {
                                        anchors.centerIn: parent
                                        visible: !alCover.visible
                                        name: "album"; size: 40
                                        color: "#FFFFFF"; strokeWidth: 1.6
                                        opacity: 0.92
                                    }
                                }

                                Text {
                                    Layout.fillWidth: true
                                    text: parent.parent.item ? (parent.parent.item.album || "") : ""
                                    font.family: window.fontFamily
                                    font.pixelSize: 12
                                    font.weight: Font.DemiBold
                                    color: window.textPrimary
                                    elide: Text.ElideRight
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: parent.parent.item ? (parent.parent.item.artist || "") : ""
                                    font.family: window.fontFamily
                                    font.pixelSize: 11
                                    color: window.textTertiary
                                    elide: Text.ElideRight
                                }
                            }

                            MouseArea {
                                id: albumArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    if (parent.item) window.openAlbum(parent.item.album, parent.item.artist)
                                }
                            }
                        }
                    }
                    Item { Layout.fillWidth: true }
                }
            }

            // ---- 段: 歌曲 ----
            ColumnLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 32
                Layout.rightMargin: 32
                Layout.bottomMargin: 24
                spacing: 8
                visible: root.trackHits.length > 0

                RowLayout {
                    Layout.fillWidth: true
                    Text {
                        text: "歌曲"
                        font.family: window.fontFamily
                        font.pixelSize: 16
                        font.weight: Font.Bold
                        color: window.textPrimary
                    }
                    Text {
                        text: "(" + root.trackHits.length + ")"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textTertiary
                    }
                    Item { Layout.fillWidth: true }
                    // 播放全部
                    Rectangle {
                        Layout.preferredHeight: 30
                        Layout.preferredWidth: playAllRow.implicitWidth + 24
                        radius: 15
                        color: playAllArea.containsMouse ? window.brandHover : window.brand
                        Behavior on color { ColorAnimation { duration: 150 } }
                        RowLayout {
                            id: playAllRow
                            anchors.centerIn: parent
                            spacing: 6
                            AppIcon { name: "play"; size: 12; color: "#FFFFFF"; filled: true }
                            Text {
                                text: "播放全部"
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                                color: "#FFFFFF"
                            }
                        }
                        MouseArea {
                            id: playAllArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                if (root.trackHits.length === 0) return
                                var paths = []
                                for (var i = 0; i < root.trackHits.length; ++i)
                                    paths.push(root.trackHits[i].path)
                                playerVM.clearQueue()
                                playerVM.enqueueMany(paths)
                            }
                        }
                    }
                }

                Repeater {
                    model: root.trackHits
                    delegate: TrackRow {
                        Layout.fillWidth: true
                        title:    modelData.title
                        artist:   modelData.artist
                        album:    modelData.album
                        duration: modelData.duration
                        liked:    modelData.liked
                        isCurrent: modelData.isCurrent
                        coverUrl: modelData.coverUrl
                        path:     modelData.path
                        onClicked:        playerVM.openFile(modelData.path)
                        onLikeClicked:    playerVM.toggleLike(modelData.path)
                        onEnqueueClicked: ctxMenu.openPlaylistMenuFor(modelData.path)
                        onMoreClicked:    ctxMenu.openFor(modelData.path)
                    }
                }
            }

            Item { Layout.preferredHeight: 16 }
        }
    }
}
