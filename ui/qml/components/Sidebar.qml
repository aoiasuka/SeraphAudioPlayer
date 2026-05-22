import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects

// 左侧导航栏 — iOS 风半透明玻璃悬浮岛 (与 MiniPlayer 同配方)
Rectangle {
    id: root
    objectName: "sidebarRoot"
    color: "transparent"                  // 由内部 glass 层负责着色
    radius: 20
    clip: true
    layer.enabled: true

    // ===== iOS 玻璃背景层 (backdrop blur + 雾化) =====
    Item {
        id: glassLayer
        anchors.fill: parent
        z: -1                             // 在所有子项之下

        // ① 抓取窗口动态背景
        ShaderEffectSource {
            id: backdropSrc
            anchors.fill: parent
            sourceItem: window.backdropItem
            sourceRect: Qt.rect(root.x, root.y, root.width, root.height)
            textureSize: Qt.size(root.width * 0.5, root.height * 0.5)
            live: true
            recursive: false
            smooth: true
            visible: false
        }

        // ② 高斯模糊
        MultiEffect {
            anchors.fill: parent
            source: backdropSrc
            blurEnabled: true
            blur: 1.0
            blurMax: 64
            blurMultiplier: 1.0
            saturation: 0.3
        }

        // ③ 半透明白色雾化层 (与 MiniPlayer 同色阶)
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.0; color: "#80FFFFFF" }
                GradientStop { position: 1.0; color: "#55FFFFFF" }
            }
        }

        // 圆角裁切
        layer.enabled: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: ShaderEffectSource {
                sourceItem: glassMask
                hideSource: true
            }
        }
    }

    // mask 用的圆角矩形 (不参与显示)
    Rectangle {
        id: glassMask
        width: root.width
        height: root.height
        radius: root.radius
        color: "black"
        antialiasing: true
        visible: false
    }

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

    // 独占式高亮滑动指示滑块 (毛玻璃悬浮风，保证全侧栏只有一个高亮，完美避免重叠或闪态，极大优化视觉效果)
    Rectangle {
        id: sharedHighlight
        x: 12
        width: parent.width - 24
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

        // 左侧 active 品牌色指示条 (跟随滑块一起平滑移动，从根本上解决切换时多个指示条并存的 bug)
        Rectangle {
            anchors.left: parent.left
            anchors.leftMargin: 6
            anchors.verticalCenter: parent.verticalCenter
            width: 4
            height: parent.height - 16
            radius: 2
            color: window.brand
        }
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
        sharedHighlight.width = activeItem.width
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

        // Logo + 折叠
        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            spacing: 10

            Rectangle {
                width: 32; height: 32; radius: 8
                color: "#2563EB"

                AppIcon {
                    anchors.centerIn: parent
                    name: "music"
                    size: 18
                    color: "#FFFFFF"
                    strokeWidth: 2
                }
            }

            Text {
                Layout.fillWidth: true
                text: "音乐播放器"
                font.family: window.fontFamily
                font.pixelSize: 16
                font.weight: Font.DemiBold
                color: window.textPrimary
                elide: Text.ElideRight
            }
        }

        // menu 按钮独占一行
        SidebarIconButton {
            Layout.fillWidth: true
            Layout.preferredHeight: 36
            iconName: "menu"
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

        // 创建的歌单 区域标题
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: 12
            Layout.rightMargin: 8
            Layout.preferredHeight: 32
            spacing: 6

            Text {
                Layout.fillWidth: true
                text: "创建的歌单"
                font.family: "Microsoft YaHei UI"
                font.pixelSize: 12
                font.weight: Font.Medium
                color: "#6B7280"
            }

            Rectangle {
                width: 24; height: 24
                radius: 12
                color: addArea.containsMouse ? window.hoverBg : "transparent"

                AppIcon {
                    anchors.centerIn: parent
                    name: "plus"
                    size: 14
                    color: "#6B7280"
                    strokeWidth: 2
                }

                MouseArea {
                    id: addArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.createPlaylistRequested()
                }
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
                    anchors.leftMargin: 12
                    anchors.rightMargin: 8
                    spacing: 10

                    // 封面渐变小块
                    Rectangle {
                        width: 28; height: 28; radius: 6
                        gradient: Gradient {
                            orientation: Gradient.Vertical
                            GradientStop { position: 0; color: "#3B82F6" }
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
                        Text {
                            Layout.fillWidth: true
                            text: modelData.name
                            font.family: "Microsoft YaHei UI"
                            font.pixelSize: 13
                            font.weight: Font.Medium
                            color: "#1F2937"
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            text: dropTarget.containsDrag ? "松开以添加" : (modelData.count + " 首")
                            font.family: "Microsoft YaHei UI"
                            font.pixelSize: 10
                            color: dropTarget.containsDrag ? window.brand : "#9CA3AF"
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

    // ④ 玻璃边缘高光描边 (1px 半透明白) — 放在最后以渲染在子项之上
    Rectangle {
        anchors.fill: parent
        radius: root.radius
        color: "transparent"
        border.color: "#66FFFFFF"
        border.width: 1
    }
}
