import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import "../components"
import "../components/SearchUtil.js" as SearchUtil

// 通用曲目列表页 — 用于音乐库 / 最近播放 / 我喜欢的
Item {
    id: root
    objectName: "trackListPage"

    // ---- 接口 ----
    property string pageTitle: "音乐库"
    property string pageSubtitle: ""
    property string emptyHint: "暂无内容"
    property string emptyAction: ""           // 空列表时按钮文本
    property var tracks: []                   // QVariantList
    property bool showOpenFile: false
    property bool showClearAll: false
    signal clearAllRequested()
    signal emptyActionRequested()

    // 搜索过滤
    property string searchText: ""
    property string _pendingSearch: ""
    Timer {
        id: searchDebounce
        interval: 250
        onTriggered: root.searchText = root._pendingSearch
    }

    readonly property var filteredTracks:
        SearchUtil.filter(root.tracks || [], root.searchText,
                          ["title", "artist", "album"])

    FileDialog {
        id: fileDialog
        title: "选择音频文件"
        nameFilters: ["Audio Files (*.wav *.flac *.mp3 *.dsf *.dff *.ogg)", "All Files (*)"]
        fileMode: FileDialog.OpenFiles
        onAccepted: {
            var paths = []
            for (var i = 0; i < selectedFiles.length; ++i) paths.push(selectedFiles[i])
            playerVM.enqueueMany(paths)
        }
    }

    // 曲目操作菜单
    TrackContextMenu {
        id: ctxMenu
    }

    // 顶部栏: 标题 + 副标题 + 操作
    Item {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 96

        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: 32
            anchors.rightMargin: 32
            spacing: 4

            Item { Layout.preferredHeight: 8 }

            RowLayout {
                Layout.fillWidth: true
                spacing: 12

                Text {
                    text: root.pageTitle
                    font.family: window.fontFamily
                    font.pixelSize: 26
                    font.weight: Font.Bold
                    color: window.textPrimary
                }

                Item { Layout.fillWidth: true }

                // 搜索框 (胶囊)
                Rectangle {
                    Layout.preferredWidth: 320
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
                            placeholderText: "搜索曲目 (支持 title:xxx / artist:xxx / album:xxx)"
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

                // 添加文件
                Rectangle {
                    visible: root.showOpenFile
                    Layout.preferredWidth: 110
                    Layout.preferredHeight: 36
                    radius: 18
                    color: openBtnArea.pressed ? window.brandPress
                         : (openBtnArea.containsMouse ? window.brandHover : window.brand)
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        AppIcon { name: "plus"; size: 14; color: "#FFFFFF" }
                        Text {
                            text: "添加文件"
                            color: "#FFFFFF"
                            font.family: window.fontFamily
                            font.pixelSize: 13
                            font.weight: Font.DemiBold
                        }
                    }

                    MouseArea {
                        id: openBtnArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: fileDialog.open()
                    }
                }

                // 清空全部
                Rectangle {
                    visible: root.showClearAll && root.tracks.length > 0
                    Layout.preferredWidth: 90
                    Layout.preferredHeight: 36
                    radius: 18
                    color: clearArea.containsMouse ? "#FEE2E2" : "#33FECACA"
                    border.color: "#33EF4444"
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    Text {
                        anchors.centerIn: parent
                        text: "清空"
                        color: "#DC2626"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                    }

                    MouseArea {
                        id: clearArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.clearAllRequested()
                    }
                }
            }

            Text {
                visible: root.pageSubtitle.length > 0
                text: root.pageSubtitle
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textSecondary
            }

            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Layout.topMargin: 4

                Text {
                    text: "共 " + root.tracks.length + " 首"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textTertiary
                }

                Item { Layout.fillWidth: true }

                // 播放全部
                Rectangle {
                    visible: root.tracks.length > 0
                    Layout.preferredWidth: 96
                    Layout.preferredHeight: 32
                    radius: 16
                    color: playAllArea.containsMouse ? window.cardHover : window.sidebarBg
                    border.color: window.borderColor
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.centerIn: parent
                        spacing: 6
                        AppIcon { name: "play"; size: 12; color: window.textPrimary; filled: true }
                        Text {
                            text: "播放全部"
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                        }
                    }

                    MouseArea {
                        id: playAllArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            playerVM.clearQueue()
                            var paths = []
                            for (var i = 0; i < root.tracks.length; ++i) paths.push(root.tracks[i].path)
                            playerVM.enqueueMany(paths)
                        }
                    }
                }
            }
        }
    }

    // 列表区
    Flickable {
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.topMargin: 4
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

            // 空态
            Item {
                Layout.fillWidth: true
                visible: root.filteredTracks.length === 0
                Layout.preferredHeight: 320

                ColumnLayout {
                    anchors.centerIn: parent
                    spacing: 12

                    Rectangle {
                        Layout.alignment: Qt.AlignHCenter
                        width: 72; height: 72; radius: 36
                        color: window.sidebarBg
                        border.color: window.borderColor
                        border.width: 1

                        AppIcon {
                            anchors.centerIn: parent
                            name: "music"
                            size: 32
                            color: window.textTertiary
                            strokeWidth: 1.8
                        }
                    }

                    Text {
                        Layout.alignment: Qt.AlignHCenter
                        text: root.searchText.length > 0 ? "没有匹配的曲目" : root.emptyHint
                        font.family: window.fontFamily
                        font.pixelSize: 14
                        color: window.textSecondary
                    }

                    Rectangle {
                        visible: root.emptyAction.length > 0 && root.searchText.length === 0
                        Layout.alignment: Qt.AlignHCenter
                        width: actionTxt.implicitWidth + 32
                        height: 36
                        radius: 18
                        color: emptyActArea.pressed ? window.brandPress
                             : (emptyActArea.containsMouse ? window.brandHover : window.brand)
                        Behavior on color { ColorAnimation { duration: 150 } }

                        Text {
                            id: actionTxt
                            anchors.centerIn: parent
                            text: root.emptyAction
                            color: "#FFFFFF"
                            font.family: window.fontFamily
                            font.pixelSize: 13
                            font.weight: Font.DemiBold
                        }

                        MouseArea {
                            id: emptyActArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: {
                                root.emptyActionRequested()
                                if (root.showOpenFile) fileDialog.open()
                            }
                        }
                    }
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
                    onClicked: {
                        // 直接播放该文件(替换队列)
                        playerVM.openFile(modelData.path)
                    }
                    onLikeClicked: playerVM.toggleLike(modelData.path)
                    onEnqueueClicked: playerVM.enqueue(modelData.path)
                    onMoreClicked: ctxMenu.openFor(modelData.path)
                }
            }

            Item { Layout.preferredHeight: 8 }
        }
    }
}
