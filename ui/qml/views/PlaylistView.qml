// =============================================================================
//  ui/qml/views/PlaylistView.qml
//
//  "当前队列"主视图。和 QueueDrawer 是两套布局,但底层都绑定 playerVM.playlistModel。
//
//  能力:
//    - 列表 ListView 用 QAbstractListModel 绑定 (role: title/artist/album/duration/coverUrl/isCurrent/path)
//    - 模式切换 + 清空 + 上下移动 + 删除 + 双击播放
//    - 导入/导出 M3U / JSON / CUE
//    - 顶部统计: 项数 / 总时长 / 当前序号
//    - 搜索过滤 (本页内,不影响 model 自身)
// =============================================================================
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import "../components"

Item {
    id: root
    objectName: "playlistView"

    // 搜索关键字 (实时, 直接过滤显示)
    property string searchText: ""

    // 工具方法: 把秒数格式化成 mm:ss 或 hh:mm:ss
    function formatTime(sec) {
        sec = Math.max(0, Math.floor(sec || 0))
        var h = Math.floor(sec / 3600)
        var m = Math.floor((sec % 3600) / 60)
        var s = sec % 60
        function pad(n) { return n < 10 ? "0" + n : "" + n }
        if (h > 0) return h + ":" + pad(m) + ":" + pad(s)
        return pad(m) + ":" + pad(s)
    }

    // ---- FileDialog (导入 / 导出 / CUE) ----
    FileDialog {
        id: importM3UDialog
        title: "导入 M3U 播放列表"
        nameFilters: ["M3U/M3U8 (*.m3u *.m3u8)", "All Files (*)"]
        fileMode: FileDialog.OpenFile
        onAccepted: {
            var err = playerVM.importPlaylistM3U(selectedFile)
            if (err.length > 0) playerVM.lastError, console.warn("[Playlist] import M3U:", err)
        }
    }
    FileDialog {
        id: exportM3UDialog
        title: "导出 M3U 播放列表"
        nameFilters: ["M3U8 (*.m3u8)"]
        fileMode: FileDialog.SaveFile
        defaultSuffix: "m3u8"
        onAccepted: {
            var err = playerVM.exportPlaylistM3U(selectedFile)
            if (err.length > 0) console.warn("[Playlist] export M3U:", err)
        }
    }
    FileDialog {
        id: importJsonDialog
        title: "导入 JSON 播放列表"
        nameFilters: ["JSON (*.json)", "All Files (*)"]
        fileMode: FileDialog.OpenFile
        onAccepted: {
            var err = playerVM.importPlaylistJson(selectedFile)
            if (err.length > 0) console.warn("[Playlist] import JSON:", err)
        }
    }
    FileDialog {
        id: exportJsonDialog
        title: "导出 JSON 播放列表"
        nameFilters: ["JSON (*.json)"]
        fileMode: FileDialog.SaveFile
        defaultSuffix: "json"
        onAccepted: {
            var err = playerVM.exportPlaylistJson(selectedFile)
            if (err.length > 0) console.warn("[Playlist] export JSON:", err)
        }
    }
    FileDialog {
        id: importCueDialog
        title: "导入 CUE 文件"
        nameFilters: ["Cue Sheet (*.cue)", "All Files (*)"]
        fileMode: FileDialog.OpenFile
        onAccepted: {
            var n = playerVM.importCueSheet(selectedFile)
            console.info("[Playlist] cue tracks added:", n)
        }
    }

    // ===== Header =====
    Rectangle {
        id: header
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 20
        height: 96
        radius: window.largeRadius
        color: window.surface
        border.color: window.borderColor
        border.width: 1

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 20
            anchors.rightMargin: 16
            spacing: 16

            // 左: 标题 + 统计
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                RowLayout {
                    spacing: 8
                    Text {
                        text: "当前队列"
                        font.family: window.fontFamily
                        font.pixelSize: 22
                        font.weight: Font.Bold
                        color: window.textPrimary
                    }
                    Rectangle {
                        Layout.preferredHeight: 22
                        Layout.preferredWidth: countTxt.implicitWidth + 14
                        radius: 11
                        color: window.brandSoft
                        Text {
                            id: countTxt
                            anchors.centerIn: parent
                            text: playerVM.playlistModel.count + " 首"
                            font.family: window.fontFamily
                            font.pixelSize: 11
                            font.weight: Font.DemiBold
                            color: window.brand
                        }
                    }
                }
                Text {
                    text: {
                        var idx = playerVM.playlistModel.currentIndex
                        var total = playerVM.playlistModel.count
                        if (total === 0) return "空队列 · 拖入文件或从首页打开开始播放"
                        var cur = (idx >= 0 ? (idx + 1) + " / " : "")
                        return cur + total + " 首 · " +
                               ["顺序", "列表循环", "单曲循环"][playerVM.repeatMode] +
                               (playerVM.shuffle ? " · 随机开" : "")
                    }
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textSecondary
                }
            }

            Item { Layout.fillWidth: true }

            // 中: 搜索
            Rectangle {
                Layout.preferredHeight: 36
                Layout.preferredWidth: 220
                radius: 18
                color: window.surfaceAlt
                border.color: searchField.activeFocus ? window.brand : window.borderColor
                border.width: 1
                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 12
                    anchors.rightMargin: 8
                    spacing: 6
                    AppIcon { name: "search"; size: 14; color: window.textTertiary; strokeWidth: 2 }
                    TextField {
                        id: searchField
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        placeholderText: "搜索队列..."
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        color: window.textPrimary
                        background: Item {}
                        verticalAlignment: TextInput.AlignVCenter
                        onTextChanged: root.searchText = text
                    }
                }
            }

            // 右: 模式
            Rectangle {
                Layout.preferredHeight: 34
                Layout.preferredWidth: modeBtnTxt.implicitWidth + 22
                radius: 17
                color: modeBtnArea.containsMouse ? window.surfaceAlt : "transparent"
                border.color: window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }
                Text {
                    id: modeBtnTxt
                    anchors.centerIn: parent
                    text: ["顺序", "列表循环", "单曲循环"][playerVM.repeatMode] +
                          (playerVM.shuffle ? " · 随机" : "")
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textPrimary
                }
                MouseArea {
                    id: modeBtnArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: playerVM.cycleRepeatMode()
                    onDoubleClicked: playerVM.toggleShuffle()
                }
            }

            // 导入 / 导出 菜单
            Rectangle {
                Layout.preferredHeight: 34
                Layout.preferredWidth: ioBtnTxt.implicitWidth + 22
                radius: 17
                color: ioArea.containsMouse ? window.surfaceAlt : "transparent"
                border.color: window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }
                Text {
                    id: ioBtnTxt
                    anchors.centerIn: parent
                    text: "导入/导出"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textPrimary
                }
                MouseArea {
                    id: ioArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: ioMenu.popup()
                }
                Menu {
                    id: ioMenu
                    MenuItem { text: "导入 M3U…";  onTriggered: importM3UDialog.open() }
                    MenuItem { text: "导入 JSON…"; onTriggered: importJsonDialog.open() }
                    MenuItem { text: "导入 CUE…";  onTriggered: importCueDialog.open() }
                    MenuSeparator {}
                    MenuItem {
                        text: "导出 M3U…";  enabled: playerVM.playlistModel.count > 0
                        onTriggered: exportM3UDialog.open()
                    }
                    MenuItem {
                        text: "导出 JSON…"; enabled: playerVM.playlistModel.count > 0
                        onTriggered: exportJsonDialog.open()
                    }
                }
            }

            // 清空
            Rectangle {
                Layout.preferredHeight: 34
                Layout.preferredWidth: clearBtnTxt.implicitWidth + 22
                radius: 17
                color: clearArea.containsMouse ? "#FEE2E2" : "transparent"
                border.color: "#33EF4444"
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }
                Text {
                    id: clearBtnTxt
                    anchors.centerIn: parent
                    text: "清空"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: "#DC2626"
                }
                MouseArea {
                    id: clearArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    enabled: playerVM.playlistModel.count > 0
                    onClicked: playerVM.clearQueue()
                }
            }
        }
    }

    // ===== 列表 =====
    Rectangle {
        id: listCard
        anchors.top: header.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.leftMargin: 20
        anchors.rightMargin: 20
        anchors.topMargin: 12
        anchors.bottomMargin: 20
        radius: window.largeRadius
        color: window.surface
        border.color: window.borderColor
        border.width: 1

        // 空态
        Item {
            anchors.centerIn: parent
            visible: playerVM.playlistModel.count === 0
            width: 380
            height: 180
            ColumnLayout {
                anchors.centerIn: parent
                spacing: 10
                AppIcon {
                    Layout.alignment: Qt.AlignHCenter
                    name: "playlist"; size: 48; color: window.textTertiary; strokeWidth: 1.6
                }
                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: "队列为空"
                    font.family: window.fontFamily
                    font.pixelSize: 16
                    font.weight: Font.DemiBold
                    color: window.textSecondary
                }
                Text {
                    Layout.alignment: Qt.AlignHCenter
                    text: "拖入音频文件、导入 M3U / JSON / CUE,或从首页打开"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textTertiary
                }
            }
        }

        ScrollView {
            anchors.fill: parent
            anchors.margins: 8
            clip: true
            visible: playerVM.playlistModel.count > 0

            ListView {
                id: list
                anchors.fill: parent
                spacing: 2
                model: playerVM.playlistModel
                currentIndex: playerVM.playlistModel.currentIndex
                highlightFollowsCurrentItem: true

                delegate: Item {
                    width: list.width
                    height: 58

                    required property int index
                    required property string title
                    required property string artist
                    required property string album
                    required property string suffix
                    required property string duration
                    required property bool   isCurrent
                    required property bool   liked
                    required property string coverUrl
                    required property string path

                    // 搜索过滤 (页内过滤,不重排;不匹配项整体隐藏)
                    readonly property bool matches:
                        root.searchText.length === 0 ||
                        title.toLowerCase().indexOf(root.searchText.toLowerCase())  >= 0 ||
                        artist.toLowerCase().indexOf(root.searchText.toLowerCase()) >= 0 ||
                        album.toLowerCase().indexOf(root.searchText.toLowerCase())  >= 0

                    visible: matches
                    height: visible ? 58 : 0

                    Rectangle {
                        anchors.fill: parent
                        anchors.margins: 2
                        radius: 10
                        color: isCurrent
                               ? window.activeBg
                               : (rowArea.containsMouse ? window.hoverBg : "transparent")
                        border.color: isCurrent ? window.brand : "transparent"
                        border.width: 1
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }

                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 14
                        anchors.rightMargin: 12
                        spacing: 12

                        // 序号 / 喇叭
                        Item {
                            Layout.preferredWidth: 28
                            Layout.preferredHeight: 28
                            Text {
                                anchors.centerIn: parent
                                visible: !isCurrent
                                text: (index + 1)
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                color: window.textTertiary
                            }
                            AppIcon {
                                anchors.centerIn: parent
                                visible: isCurrent
                                name: "volume"; size: 14
                                color: window.brand
                                strokeWidth: 2
                            }
                        }

                        // 缩略图
                        Rectangle {
                            Layout.preferredWidth: 40
                            Layout.preferredHeight: 40
                            radius: 6
                            color: window.surfaceAlt
                            border.color: window.borderColor
                            border.width: 1
                            clip: true

                            Image {
                                anchors.fill: parent
                                source: coverUrl
                                visible: coverUrl.length > 0 && status === Image.Ready
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                                cache: true
                            }
                            AppIcon {
                                anchors.centerIn: parent
                                visible: coverUrl.length === 0
                                name: "music"; size: 18
                                color: window.textTertiary
                                strokeWidth: 1.6
                            }
                        }

                        // 标题 / 副标题
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Text {
                                Layout.fillWidth: true
                                text: title
                                font.family: window.fontFamily
                                font.pixelSize: 13
                                font.weight: isCurrent ? Font.DemiBold : Font.Medium
                                color: isCurrent ? window.brand : window.textPrimary
                                elide: Text.ElideRight
                            }
                            Text {
                                Layout.fillWidth: true
                                text: (artist || suffix) + (album ? " · " + album : "")
                                font.family: window.fontFamily
                                font.pixelSize: 11
                                color: window.textTertiary
                                elide: Text.ElideRight
                            }
                        }

                        Text {
                            visible: duration.length > 0
                            text: duration
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            color: window.textSecondary
                        }

                        // 喜欢
                        Item {
                            Layout.preferredWidth: 28
                            Layout.preferredHeight: 28
                            AppIcon {
                                anchors.centerIn: parent
                                name: "heart"
                                size: 14
                                color: liked ? window.likeRed
                                       : (likeArea.containsMouse ? window.textPrimary : window.textTertiary)
                                strokeWidth: 2
                            }
                            MouseArea {
                                id: likeArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.toggleLike(path)
                            }
                        }

                        // 上移
                        Item {
                            Layout.preferredWidth: 22
                            Layout.preferredHeight: 22
                            visible: rowArea.containsMouse
                            AppIcon {
                                anchors.centerIn: parent
                                name: "chevron"; rotation: -90; size: 12
                                color: upArea.containsMouse ? window.brand : window.textTertiary
                                strokeWidth: 2
                            }
                            MouseArea {
                                id: upArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.moveQueueItem(index, index - 1)
                            }
                        }

                        // 下移
                        Item {
                            Layout.preferredWidth: 22
                            Layout.preferredHeight: 22
                            visible: rowArea.containsMouse
                            AppIcon {
                                anchors.centerIn: parent
                                name: "chevron"; rotation: 90; size: 12
                                color: dnArea.containsMouse ? window.brand : window.textTertiary
                                strokeWidth: 2
                            }
                            MouseArea {
                                id: dnArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.moveQueueItem(index, index + 1)
                            }
                        }

                        // 删除
                        Item {
                            Layout.preferredWidth: 26
                            Layout.preferredHeight: 26
                            visible: rowArea.containsMouse
                            Rectangle {
                                anchors.fill: parent
                                radius: 13
                                color: delArea.containsMouse ? "#FEE2E2" : "transparent"
                                Behavior on color { ColorAnimation { duration: 120 } }
                            }
                            AppIcon {
                                anchors.centerIn: parent
                                name: "close"; size: 12
                                color: delArea.containsMouse ? "#DC2626" : window.textTertiary
                                strokeWidth: 2
                            }
                            MouseArea {
                                id: delArea
                                anchors.fill: parent
                                hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.removeAt(index)
                            }
                        }
                    }

                    MouseArea {
                        id: rowArea
                        anchors.fill: parent
                        hoverEnabled: true
                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                        cursorShape: Qt.PointingHandCursor
                        onClicked: function(mouse) {
                            if (mouse.button === Qt.RightButton) {
                                rowMenu.popup()
                            }
                        }
                        onDoubleClicked: playerVM.playIndex(index)
                        z: -1
                    }

                    Menu {
                        id: rowMenu
                        MenuItem { text: "播放";       onTriggered: playerVM.playIndex(index) }
                        MenuItem { text: "置顶";       enabled: index > 0
                                   onTriggered: playerVM.moveQueueItem(index, 0) }
                        MenuItem { text: "上移一行";   enabled: index > 0
                                   onTriggered: playerVM.moveQueueItem(index, index - 1) }
                        MenuItem { text: "下移一行";   enabled: index < playerVM.playlistModel.count - 1
                                   onTriggered: playerVM.moveQueueItem(index, index + 1) }
                        MenuSeparator {}
                        MenuItem { text: liked ? "取消喜欢" : "标记为喜欢"
                                   onTriggered: playerVM.toggleLike(path) }
                        MenuItem { text: "从队列移除"
                                   onTriggered: playerVM.removeAt(index) }
                    }
                }
            }
        }
    }
}
