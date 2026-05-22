import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import QtQuick.Dialogs
import "../components"
import "../components/SearchUtil.js" as SearchUtil

Item {
    id: root
    objectName: "homeView"

    FileDialog {
        id: fileDialog
        title: "选择音频文件"
        nameFilters: ["Audio Files (*.wav *.flac *.mp3 *.dsf *.dff *.ogg)", "All Files (*)"]
        onAccepted: {
            playerVM.openFile(selectedFile)
            playerVM.play()
        }
    }

    TrackContextMenu {
        id: ctxMenu
    }

    readonly property var recommendedPlaylists: []

    // 搜索关键字 (经过防抖, 用于实际过滤)
    property string searchText: ""
    // 搜索输入实时值 (由 TextField 写入, 防抖后同步到 searchText)
    property string _pendingSearch: ""
    Timer {
        id: searchDebounce
        interval: 250
        onTriggered: root.searchText = root._pendingSearch
    }

    // 过滤后的最近播放: 支持 title / artist / album, 空格分词, 字段前缀, 字段权重排序
    readonly property var filteredRecent:
        SearchUtil.filter(playerVM.recent || [], root.searchText,
                          ["title", "artist", "album"])

    // 顶部操作栏
    Item {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 64

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 24
            spacing: 12

            // 搜索框 (毛玻璃胶囊)
            Rectangle {
                Layout.preferredWidth: 460
                Layout.preferredHeight: 44
                Layout.alignment: Qt.AlignVCenter
                radius: 22
                color: searchBox.activeFocus ? window.surface : window.sidebarBg
                border.color: searchBox.activeFocus ? window.brand : "#33FFFFFF"
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 16
                    anchors.rightMargin: 16
                    spacing: 8

                    TextField {
                        id: searchBox
                        Layout.fillWidth: true
                        placeholderText: "搜索歌曲、歌手或专辑 (Enter 在全部库中搜索)"
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
                        // Enter 提交 → 跳转全局搜索结果页
                        Keys.onReturnPressed: {
                            var q = (text || "").trim()
                            if (q.length > 0) window.openSearch(q)
                        }
                        Keys.onEnterPressed: {
                            var q = (text || "").trim()
                            if (q.length > 0) window.openSearch(q)
                        }
                    }

                    AppIcon {
                        name: "search"
                        size: 16
                        color: window.textSecondary
                        strokeWidth: 2
                    }
                }
            }

            Item { Layout.fillWidth: true }
        }

        // 移除了底部分隔线，保持悬浮感
    }

    // 主滚动区
    Flickable {
        anchors.top: topBar.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        contentWidth: width
        contentHeight: content.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
            width: 8
        }

        ColumnLayout {
            id: content
            width: parent.width
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.leftMargin: 36
            anchors.rightMargin: 36
            spacing: 28

            Item { Layout.preferredHeight: 4 }

            // Hero 横幅 (沉浸式单首"最近播放")
            HeroBanner {
                Layout.fillWidth: true
                Layout.preferredHeight: 220
                currentItem: {
                    var src = playerVM.recent || []
                    return src.length > 0 ? src[0] : null
                }
                fallbackTitle: "Audio Player X86"
                fallbackSubtitle: "拖拽 .wav / .flac 到窗口或点击下方按钮选择文件"
                onPlayClicked: fileDialog.open()
                onItemClicked: function(path) { playerVM.openFile(path) }
            }



            // ========== 最近播放 ==========
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 4
                spacing: 8

                Text {
                    Layout.fillWidth: true
                    text: "最近播放"
                    font.family: window.fontFamily
                    font.pixelSize: 18
                    font.weight: Font.Bold
                    font.letterSpacing: -0.2
                    color: "#0F172A"
                }

                Item {
                    Layout.preferredHeight: moreRow.implicitHeight
                    Layout.preferredWidth: moreRow.implicitWidth

                    RowLayout {
                        id: moreRow
                        anchors.fill: parent
                        spacing: 4

                        Text {
                            text: "更多"
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.Medium
                            color: moreArea.containsMouse ? "#0F172A" : "#475569"
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }
                        AppIcon {
                            name: "chevron"
                            size: 12
                            color: moreArea.containsMouse ? "#0F172A" : "#475569"
                            strokeWidth: 1.8
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }
                    }

                    MouseArea {
                        id: moreArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: window.navigateTo("history")
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2                  // 行间留极细缝, 行内 padding 已通过 TrackRow 高度 64 体现呼吸感

                Repeater {
                    model: root.filteredRecent
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

            Item { Layout.preferredHeight: 32 }
        }
    }
}
