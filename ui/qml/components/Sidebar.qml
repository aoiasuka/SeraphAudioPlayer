import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 左侧导航 — Synapse Mica/Acrylic 风格
//
// 视觉:
//   - 半透明白 (rgba(248,250,252,0.55)) 背景 + 右侧 1px 极细分隔
//   - 顶部 Synapse 品牌 logo (cyan→indigo 渐变小方块)
//   - 主导航 Fluent 风: 8 字深底 + 18px 图标 + 左侧 3px cyan 高亮条 (active)
//   - 中部 "我的歌单" 区, 仍由 ViewModel 提供
//   - 底部用户卡片 + 设置齿轮
Rectangle {
    id: root
    objectName: "sidebarRoot"
    color: window.acrylicSidebarBg
    antialiasing: true
    clip: true

    // 右侧 1px 极细描边
    Rectangle {
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        width: 1
        color: window.borderColor
    }

    property string activeKey: "home"
    property bool busy: false
    property bool isInitialized: false
    signal navClicked(string key)

    readonly property var mainNav: [
        { key: "home",     label: "首页",     icon: "home" },
        { key: "library",  label: "本地音乐", icon: "library" },
        { key: "history",  label: "最近播放", icon: "history" },
        { key: "liked",    label: "我喜欢的", icon: "heart" },
        { key: "playlist", label: "歌单",     icon: "playlist" },
        { key: "artist",   label: "艺术家",   icon: "artist" },
        { key: "album",    label: "专辑",     icon: "album" },
        { key: "settings", label: "设置",     icon: "settings" }
    ]

    readonly property var userPlaylists: playerVM.playlists || []

    signal openPlaylistRequested(string id)
    signal createPlaylistRequested()

    // 独占式高亮: 浅胶囊 + 左侧 3px cyan 竖条
    Rectangle {
        id: sharedHighlight
        x: 8
        width: parent.width - 16
        height: 36
        radius: 8
        color: window.brandSoftBg
        visible: y > 0
        z: 0

        opacity: root.busy ? 0.0 : 1.0
        Behavior on opacity { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }
        Behavior on y {
            enabled: root.isInitialized && !root.busy
            NumberAnimation { duration: 350; easing.type: Easing.OutBack; easing.overshoot: 1.2 }
        }
        Behavior on height {
            enabled: root.isInitialized && !root.busy
            NumberAnimation { duration: 250; easing.type: Easing.OutQuad }
        }

        // 左侧 3px cyan 竖条 (active pill)
        Rectangle {
            anchors.left: parent.left
            anchors.verticalCenter: parent.verticalCenter
            anchors.leftMargin: -1
            width: 3
            height: parent.height * 0.5
            radius: 2
            color: window.brand
        }
    }

    function syncHighlight() {
        for (var i = 0; i < mainNavRepeater.count; ++i) {
            var item = mainNavRepeater.itemAt(i)
            if (item && item.active) {
                updateHighlight(item)
                return
            }
        }
        updateHighlight(null)
    }

    function updateHighlight(activeItem) {
        if (!activeItem) {
            sharedHighlight.visible = false
            return
        }
        var pos = activeItem.mapToItem(root, 0, 0)
        sharedHighlight.y = pos.y
        sharedHighlight.height = activeItem.height
        sharedHighlight.visible = true
    }

    onActiveKeyChanged: Qt.callLater(syncHighlight)
    onWidthChanged: Qt.callLater(syncHighlight)

    Component.onCompleted: {
        Qt.callLater(function() {
            syncHighlight()
            root.isInitialized = true
        })
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        anchors.rightMargin: 13
        spacing: 4

        // ===== 顶部品牌 logo (Cyan→Indigo 渐变小方块 + Synapse 文字) =====
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 40
            Layout.leftMargin: 4
            Layout.bottomMargin: 8
            spacing: 10

            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                Layout.leftMargin: window.sidebarExpanded ? 0 : (parent.width - 28) / 2
                width: 28; height: 28; radius: 7
                gradient: Gradient {
                    orientation: Gradient.Diagonal
                    GradientStop { position: 0.0; color: window.brand }
                    GradientStop { position: 1.0; color: "#6366F1" }
                }
                AppIcon {
                    anchors.centerIn: parent
                    name: "music"
                    size: 14
                    color: "#FFFFFF"
                    strokeWidth: 2
                }
            }

            Text {
                Layout.fillWidth: true
                text: "Seraph Audio"
                font.family: window.fontFamily
                font.pixelSize: 15
                font.weight: Font.Bold
                font.letterSpacing: 0.6
                color: window.textPrimary
                elide: Text.ElideRight
                visible: window.sidebarExpanded
            }
        }

        // ===== 新建歌单 (紧凑胶囊, 不再霸占整行) =====
        Rectangle {
            Layout.fillWidth: window.sidebarExpanded
            Layout.preferredWidth: window.sidebarExpanded ? -1 : 36
            Layout.alignment: window.sidebarExpanded ? Qt.AlignLeft : Qt.AlignHCenter
            Layout.preferredHeight: 34
            radius: 8
            color: newBtnArea.containsMouse ? window.acrylicCardBgHi : window.acrylicCardBg
            border.color: newBtnArea.containsMouse ? window.borderColor : "transparent"
            border.width: 1
            Behavior on color { ColorAnimation { duration: 150 } }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: window.sidebarExpanded ? 10 : 0
                anchors.rightMargin: 10
                spacing: 8

                Item {
                    Layout.preferredWidth: window.sidebarExpanded ? 18 : 34
                    Layout.preferredHeight: 18
                    AppIcon {
                        anchors.centerIn: parent
                        name: "plus"
                        size: 14
                        color: window.brand
                        strokeWidth: 2
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: "新建歌单"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    font.weight: Font.Medium
                    color: window.textPrimary
                    visible: window.sidebarExpanded
                }
            }

            MouseArea {
                id: newBtnArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.createPlaylistRequested()
            }
        }

        Item { Layout.preferredHeight: 4 }

        // ===== 主导航 =====
        Repeater {
            id: mainNavRepeater
            model: root.mainNav
            delegate: SidebarItem {
                Layout.fillWidth: true
                navKey: modelData.key
                label: modelData.label
                iconName: modelData.icon
                active: root.activeKey === modelData.key
                onClicked: root.navClicked(modelData.key)
            }
        }

        Item { Layout.preferredHeight: 12 }

        // ===== "我的歌单" 标题 (仅展开时显示) =====
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 8
            Layout.preferredHeight: 22
            spacing: 6
            visible: window.sidebarExpanded

            Text {
                Layout.fillWidth: true
                text: "我的歌单"
                font.family: window.fontFamily
                font.pixelSize: 10
                font.weight: Font.Bold
                font.capitalization: Font.AllUppercase
                font.letterSpacing: 0.8
                color: window.textTertiary
            }
        }

        // ===== 用户歌单列表 =====
        Repeater {
            model: root.userPlaylists
            delegate: Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 36
                radius: 8
                color: dropTarget.containsDrag ? window.brandSoftBg
                     : (rowArea.containsMouse ? window.hoverBg : "transparent")
                border.color: dropTarget.containsDrag ? window.brand : "transparent"
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                RowLayout {
                    anchors.fill: parent
                    spacing: 10

                    // 封面渐变小块
                    Rectangle {
                        Layout.alignment: Qt.AlignVCenter
                        Layout.leftMargin: window.sidebarExpanded ? 10 : (parent.width - 24) / 2
                        width: 24; height: 24; radius: 5
                        gradient: Gradient {
                            orientation: Gradient.Vertical
                            GradientStop { position: 0; color: window.brand }
                            GradientStop { position: 1; color: "#6366F1" }
                        }
                        AppIcon {
                            anchors.centerIn: parent
                            name: "playlist"
                            size: 12
                            color: "#FFFFFF"
                            strokeWidth: 2
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 0
                        visible: window.sidebarExpanded
                        Text {
                            Layout.fillWidth: true
                            text: modelData.name
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.Medium
                            color: window.textPrimary
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            text: dropTarget.containsDrag ? "松开以添加" : (modelData.count + " 首")
                            font.family: window.fontFamily
                            font.pixelSize: 9
                            color: dropTarget.containsDrag ? window.brand : window.textTertiary
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }
                    }
                }

                DropArea {
                    id: dropTarget
                    anchors.fill: parent
                    keys: ["application/x-apx-track"]
                    onDropped: function(drop) {
                        var p = drop.getDataAsString("application/x-apx-track")
                        if (p && p.length > 0) {
                            playerVM.addToPlaylist(modelData.id, p)
                            drop.accept(Qt.CopyAction)
                        }
                    }
                }

                MouseArea {
                    id: rowArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.openPlaylistRequested(modelData.id)
                    propagateComposedEvents: true
                    z: -1

                    ToolTip.visible: !window.sidebarExpanded && containsMouse
                    ToolTip.text: modelData.name
                }
            }
        }

        Item { Layout.fillHeight: true }


    }
}
