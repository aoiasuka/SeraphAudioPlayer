import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 左侧导航 — 极简纯色面板 (现作为 Drawer 内容使用)
// 注: 自身透明 + radius+clip; Drawer 的 background 负责底色和边框, 圆角由二者共同维持
Rectangle {
    id: root
    objectName: "sidebarRoot"
    color: "transparent"
    radius: window.largeRadius
    antialiasing: true
    clip: true

    property string activeKey: "home"
    property bool busy: false
    property bool isInitialized: false
    signal navClicked(string key)

    // 主导航项
    readonly property var mainNav: [
        { key: "home",     label: "首页",     icon: "home" },
        { key: "library",  label: "音乐库",   icon: "library" },
        { key: "playlist", label: "歌单",     icon: "playlist" },
        { key: "artist",   label: "歌手",     icon: "artist" },
        { key: "album",    label: "专辑",     icon: "album" },
        { key: "history",  label: "最近播放", icon: "history" },
        { key: "liked",    label: "我喜欢的", icon: "heart" }
    ]

    // 创建的歌单 — 改用 ViewModel 提供的真实数据
    readonly property var userPlaylists: playerVM.playlists || []

    // 信号通知主窗口打开特定歌单或新建
    signal openPlaylistRequested(string id)
    signal createPlaylistRequested()

    // 独占式高亮滑动指示滑块
    Rectangle {
        id: sharedHighlight
        x: window.sidebarExpanded ? 12 : (root.width - 40) / 2
        width: window.sidebarExpanded ? parent.width - 24 : 40
        height: 40
        radius: 20
        color: window.activeBg
        visible: y > 0
        z: 0

        // 切换时淡出，完成后淡入，防止切换过程中显示两个提示
        opacity: root.busy ? 0.0 : 1.0
        Behavior on opacity {
            NumberAnimation { duration: 150; easing.type: Easing.OutQuad }
        }

        // Y轴滑动平滑阻尼动画 (切换时临时禁用，使其隐形定位到新元素位置)
        Behavior on y {
            id: sharedHighlightYBehavior
            enabled: root.isInitialized && !root.busy
            NumberAnimation { duration: 220; easing.type: Easing.OutCubic }
        }

        // 移除了左侧竖向品牌色指示条，改为纯粹的背景高亮
    }

    // 动态同步高亮滑块坐标
    function syncHighlight() {
        var activeFound = false
        for (var i = 0; i < mainNavRepeater.count; ++i) {
            var item = mainNavRepeater.itemAt(i)
            if (item && item.active) {
                updateHighlight(item)
                activeFound = true
                break
            }
        }
        if (!activeFound) {
            if (settingsItem && settingsItem.active) {
                updateHighlight(settingsItem)
            } else {
                updateHighlight(null)
            }
        }
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

    onActiveKeyChanged: {
        Qt.callLater(syncHighlight)
    }

    onWidthChanged: {
        Qt.callLater(syncHighlight)
    }

    Component.onCompleted: {
        Qt.callLater(function() {
            syncHighlight()
            // 首次定位完成后启用滑动动画，防止冷启动时滑块从顶部飞入
            root.isInitialized = true
        })
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: 12
        spacing: 4

        // Logo 区域
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            spacing: 12

            Rectangle {
                Layout.alignment: Qt.AlignVCenter
                Layout.leftMargin: window.sidebarExpanded ? 4 : (parent.width - 32) / 2
                width: 32; height: 32; radius: 16
                color: window.brand
                AppIcon {
                    anchors.centerIn: parent
                    name: "music"
                    size: 16
                    color: "#FFFFFF"
                    strokeWidth: 2
                }
            }

            Text {
                Layout.fillWidth: true
                text: "AudioPlayer"
                font.family: window.fontFamily
                font.pixelSize: 18
                font.weight: Font.DemiBold
                color: window.textPrimary
                elide: Text.ElideRight
                visible: window.sidebarExpanded
            }
        }

        Item { Layout.preferredHeight: 12 }

        // 醒目的“新建歌单”按钮 (类似 Gemini 的发起新对话)
        Rectangle {
            Layout.preferredWidth: window.sidebarExpanded ? parent.width : 44
            Layout.alignment: window.sidebarExpanded ? Qt.AlignLeft : Qt.AlignHCenter
            Layout.preferredHeight: 44
            radius: 22
            color: newBtnArea.containsMouse ? window.cardHover : window.surfaceAlt
            Behavior on color { ColorAnimation { duration: 150 } }

            RowLayout {
                anchors.fill: parent
                spacing: 12
                
                Item {
                    Layout.preferredWidth: 44
                    Layout.preferredHeight: 44
                    AppIcon {
                        anchors.centerIn: parent
                        name: "plus"
                        size: 18
                        color: window.textPrimary
                        strokeWidth: 2
                    }
                }

                Text {
                    Layout.fillWidth: true
                    text: "新建歌单"
                    font.family: window.fontFamily
                    font.pixelSize: 14
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

        Item { Layout.preferredHeight: 8 }

        // 主导航
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

        Item { Layout.preferredHeight: 16 }

        // 创建的歌单 区域标题 (仅展开时显示)
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 8
            Layout.preferredHeight: 32
            spacing: 6
            visible: window.sidebarExpanded

            Text {
                Layout.fillWidth: true
                text: "我的歌单"
                font.family: window.fontFamily
                font.pixelSize: 12
                font.weight: Font.Medium
                color: window.textSecondary
            }
        }

        // 用户歌单列表
        Repeater {
            model: root.userPlaylists
            delegate: Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 44
                radius: 12
                color: dropTarget.containsDrag ? window.brandSoft
                     : (rowArea.containsMouse ? window.hoverBg : "transparent")
                border.color: dropTarget.containsDrag ? window.brand : "transparent"
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                RowLayout {
                    anchors.fill: parent
                    spacing: 12

                    // 封面渐变小块
                    Rectangle {
                        Layout.alignment: Qt.AlignVCenter
                        Layout.leftMargin: window.sidebarExpanded ? 12 : (parent.width - 28) / 2
                        width: 28; height: 28; radius: 6
                        gradient: Gradient {
                            orientation: Gradient.Vertical
                            GradientStop { position: 0; color: window.brand }
                            GradientStop { position: 1; color: "#6366F1" }
                        }
                        AppIcon {
                            anchors.centerIn: parent
                            name: "playlist"
                            size: 14
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
                            font.pixelSize: 13
                            font.weight: Font.Medium
                            color: window.textPrimary
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            text: dropTarget.containsDrag ? "松开以添加" : (modelData.count + " 首")
                            font.family: window.fontFamily
                            font.pixelSize: 10
                            color: dropTarget.containsDrag ? window.brand : window.textTertiary
                            Behavior on color { ColorAnimation { duration: 120 } }
                        }
                    }
                }

                // 接收拖入的曲目
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
                    // 不要阻塞 DropArea
                    propagateComposedEvents: true
                    z: -1
                    
                    ToolTip.visible: !window.sidebarExpanded && containsMouse
                    ToolTip.text: modelData.name
                }
            }
        }

        Item { Layout.fillHeight: true }

        // 分隔线
        Rectangle {
            Layout.fillWidth: true
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.preferredHeight: 1
            color: window.divider
        }

        // 设置
        SidebarItem {
            id: settingsItem
            Layout.fillWidth: true
            navKey: "settings"
            label: "设置"
            iconName: "settings"
            active: root.activeKey === "settings"
            onClicked: root.navClicked("settings")
        }
    }
}
