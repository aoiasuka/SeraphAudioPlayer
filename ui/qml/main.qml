import QtQuick
import QtQuick.Controls
import QtQuick.Window
import QtQuick.Layouts
import QtQuick.Effects
import "views"
import "components"

ApplicationWindow {
    id: window
    flags: Qt.Window | Qt.FramelessWindowHint | Qt.CustomizeWindowHint | Qt.WindowMinimizeButtonHint | Qt.WindowMaximizeButtonHint | Qt.WindowSystemMenuHint
    width: 1280
    height: 800
    minimumWidth: 1024
    minimumHeight: 640
    visible: true
    title: qsTr("Audio Player X86")
    color: "transparent"

    property bool sidebarExpanded: true
    function toggleSidebar() { sidebarExpanded = !sidebarExpanded }

    TitleBar {
        id: titleBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        targetWindow: window
        z: 9999
        onHamburgerClicked: window.toggleSidebar()
    }

    // ===== 边缘 / 四角 resize 热区 (frameless 模式自管) =====
    // 用 8 个不可见 MouseArea 覆盖窗口边缘,触发原生 startSystemResize.
    // 厚度 5px (鼠标精度足够,又不抢内部控件命中).
    Item {
        anchors.fill: parent
        z: 99998   // 在 titleBar (9999) 之下,但在大多数内容之上
        visible: window.visibility !== Window.Maximized && window.visibility !== Window.FullScreen

        readonly property int edge: 5
        readonly property int corner: 10

        // ---- 四边 ----
        MouseArea {
            anchors { left: parent.left; right: parent.right; top: parent.top }
            anchors.leftMargin: parent.corner
            anchors.rightMargin: parent.corner
            height: parent.edge
            cursorShape: Qt.SizeVerCursor
            onPressed: window.startSystemResize(Qt.TopEdge)
        }
        MouseArea {
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
            anchors.leftMargin: parent.corner
            anchors.rightMargin: parent.corner
            height: parent.edge
            cursorShape: Qt.SizeVerCursor
            onPressed: window.startSystemResize(Qt.BottomEdge)
        }
        MouseArea {
            anchors { top: parent.top; bottom: parent.bottom; left: parent.left }
            anchors.topMargin: parent.corner
            anchors.bottomMargin: parent.corner
            width: parent.edge
            cursorShape: Qt.SizeHorCursor
            onPressed: window.startSystemResize(Qt.LeftEdge)
        }
        MouseArea {
            anchors { top: parent.top; bottom: parent.bottom; right: parent.right }
            anchors.topMargin: parent.corner
            anchors.bottomMargin: parent.corner
            width: parent.edge
            cursorShape: Qt.SizeHorCursor
            onPressed: window.startSystemResize(Qt.RightEdge)
        }
        // ---- 四角 ----
        MouseArea {
            anchors { left: parent.left; top: parent.top }
            width: parent.corner; height: parent.corner
            cursorShape: Qt.SizeFDiagCursor
            onPressed: window.startSystemResize(Qt.LeftEdge | Qt.TopEdge)
        }
        MouseArea {
            anchors { right: parent.right; top: parent.top }
            width: parent.corner; height: parent.corner
            cursorShape: Qt.SizeBDiagCursor
            onPressed: window.startSystemResize(Qt.RightEdge | Qt.TopEdge)
        }
        MouseArea {
            anchors { left: parent.left; bottom: parent.bottom }
            width: parent.corner; height: parent.corner
            cursorShape: Qt.SizeBDiagCursor
            onPressed: window.startSystemResize(Qt.LeftEdge | Qt.BottomEdge)
        }
        MouseArea {
            anchors { right: parent.right; bottom: parent.bottom }
            width: parent.corner; height: parent.corner
            cursorShape: Qt.SizeFDiagCursor
            onPressed: window.startSystemResize(Qt.RightEdge | Qt.BottomEdge)
        }
    }

    // ---- 主题 token (子组件通过 window.xxx 访问) ----
    // 极简护眼主题:
    //   • 全局背景使用低饱和度复古纸张色, 长时间使用减少疲劳
    //   • 不再依靠透明叠加表达层级, 改用纯色面板 + 柔和投影
    //   • 文字使用接近纯黑的深灰提高对比度
    readonly property color appBg: "#FDFBF7"             // 主背景: 米白纸张色
    readonly property color appBgSubtle: "#F4F0EA"       // 次级背景: 浅麦色 (备用)
    readonly property color surface: "#FFFFFF"           // 卡片/弹窗面板: 纯白
    readonly property color surfaceAlt: "#FAFAFA"        // 备用/分区背景
    readonly property color surfaceHover: "#F2EFE8"      // 行 hover 浅麦色
    readonly property color sidebarBg: "#FFFFFF"         // 侧边/抽屉面板
    readonly property color hoverBg: "#0A000000"         // 通用 hover 4% 黑
    readonly property color cardHover: "#F2EFE8"
    readonly property color activeBg: "#FFE8E6FF"        // 选中态: 极淡品牌冷调
    readonly property color playerBg: "#FFFFFF"

    readonly property color textPrimary: "#1C1C1E"       // iOS 风深灰, 接近纯黑
    readonly property color textSecondary: "#6E6E73"     // 次级
    readonly property color textTertiary: "#8E8E93"      // 占位/弱化

    readonly property color brand: "#3B82F6"
    readonly property color brandHover: "#2563EB"
    readonly property color brandPress: "#1D4ED8"
    readonly property color brandSoft: "#DBEAFE"

    readonly property color heroTop: "#2563EB"
    readonly property color heroBottom: "#4F46E5"

    readonly property color borderColor: "#1A000000"     // 1px 描边: 10% 黑
    readonly property color hairline: "#0F000000"        // 极细分隔
    readonly property color divider: "#14000000"
    readonly property color likeRed: "#EF4444"

    // ===== Design tokens: 纯色面板 + 阴影 + 圆角 =====
    readonly property color surfaceMenu: "#FFFFFF"       // 菜单/弹窗: 纯白
    readonly property color menuHoverBg: "#F2EFE8"       // 菜单项 hover: 浅麦色
    readonly property color modalScrim: "#66000000"      // 模态遮罩: 40% 黑

    // 阴影色 (用于 MultiEffect shadowColor)
    readonly property color shadowColor: "#26000000"     // 大阴影 (浮窗)
    readonly property color shadowColorSoft: "#14000000" // 卡片阴影
    readonly property color shadowColorHairline: "#0A000000"

    // 圆角令牌
    readonly property int smallRadius: 8
    readonly property int mediumRadius: 12
    readonly property int largeRadius: 16
    readonly property int xLargeRadius: 20

    // ===== 向后兼容别名 (将在弹窗/菜单重构完成后逐步移除) =====
    // 让之前用 glassBg / menuBg / glassBorder 的组件仍能编译,
    // 实际值映射到新的纯色面板与极细描边
    readonly property color glassBg: surface
    readonly property color glassBgSoft: surface
    readonly property color menuBg: surfaceMenu
    readonly property color glassBorder: borderColor
    readonly property color glassBorderDark: borderColor

    readonly property string fontFamily: "Microsoft YaHei UI"

    // MiniPlayer 玻璃背景使用此别名抓取动态背景做 backdrop blur
    property alias backdropItem: dynamicBg

    // ===== 封面主色 (现仅用于 ColorImageProvider / 个别强调元素, 不再用于全屏背景) =====
    property color domColor1: playerVM.currentDominantColor || window.brand
    property color domColor2: window.brand
    Behavior on domColor1 { ColorAnimation { duration: 1500; easing.type: Easing.InOutQuad } }

    // ===== 护眼纯色背景 =====
    // 弃用先前的紫色对角渐变 + 白雾化 + 浮动光晕,
    // 采用低饱和度米白纸张色作为主背景, 长时间观看更舒适
    //
    // radius: 8 与 DwmSetWindowAttribute(DWMWA_WINDOW_CORNER_PREFERENCE=DWMWCP_ROUND)
    // 在 Win11 给出的系统圆角(约 8px)对齐,避免 DWM 圆角剪裁后露出方角"破角"。
    // 最大化时窗口贴满工作区,Win11 自动取消圆角剪裁,此时圆角同样不会影响视觉。
    Rectangle {
        id: dynamicBg
        anchors.fill: parent
        radius: window.visibility === Window.Maximized || window.visibility === Window.FullScreen ? 0 : 8
        antialiasing: true
        color: window.appBg
    }

    // 当前选中的侧栏菜单 (用于 active 高亮)
    property string currentNav: "home"

    // 切换主区域到指定导航 key
    function navigateTo(key) {
        if (stackView.busy) return
        if (currentNav === key && stackView.depth <= 1) return
        currentNav = key
        // 清掉 NowPlaying 等顶层视图栈,回到根层并替换
        while (stackView.depth > 1) stackView.pop(StackView.Immediate)
        stackView.replace(viewFor(key))
    }

    // 打开某个用户歌单详情
    function openPlaylist(id) {
        if (stackView.busy) return
        currentNav = "playlist"
        while (stackView.depth > 1) stackView.pop(StackView.Immediate)
        stackView.replace(Qt.resolvedUrl("views/PlaylistDetailView.qml"), { playlistId: id })
    }

    // 打开歌手详情
    function openArtist(name) {
        if (stackView.busy) return
        currentNav = "artist"
        while (stackView.depth > 1) stackView.pop(StackView.Immediate)
        stackView.replace(Qt.resolvedUrl("views/ArtistDetailView.qml"), { artistName: name })
    }

    // 打开专辑详情
    function openAlbum(name, artist) {
        if (stackView.busy) return
        currentNav = "album"
        while (stackView.depth > 1) stackView.pop(StackView.Immediate)
        stackView.replace(Qt.resolvedUrl("views/AlbumDetailView.qml"),
                          { albumName: name, artistName: artist || "" })
    }

    // 打开全局搜索结果视图
    function openSearch(query) {
        if (stackView.busy) return
        var q = (query || "").toString()
        currentNav = "search"
        while (stackView.depth > 1) stackView.pop(StackView.Immediate)
        stackView.replace(Qt.resolvedUrl("views/SearchResultsView.qml"), { query: q })
    }

    function viewFor(key) {
        switch (key) {
        case "home":     return Qt.resolvedUrl("views/HomeView.qml")
        case "library":  return Qt.resolvedUrl("views/LibraryView.qml")
        case "playlist": return Qt.resolvedUrl("views/PlaylistsView.qml")
        case "artist":   return Qt.resolvedUrl("views/ArtistView.qml")
        case "album":    return Qt.resolvedUrl("views/AlbumView.qml")
        case "history":  return Qt.resolvedUrl("views/HistoryView.qml")
        case "liked":    return Qt.resolvedUrl("views/LikedView.qml")
        case "settings": return Qt.resolvedUrl("views/SettingsView.qml")
        case "viz_settings": return Qt.resolvedUrl("views/VizSettingsView.qml")
        case "queue":    return Qt.resolvedUrl("views/PlaylistView.qml")
        default:         return Qt.resolvedUrl("views/HomeView.qml")
        }
    }

    // ===== 主布局: 左侧侧边栏 + 右侧主内容区 =====
    Sidebar {
        id: drawerSidebar
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.topMargin: titleBar.height
        anchors.bottomMargin: 80
        width: window.sidebarExpanded ? 260 : 72
        Behavior on width { NumberAnimation { duration: 250; easing.type: Easing.OutQuart } }

        activeKey: window.currentNav
        busy: stackView.busy
        onNavClicked: function(key) {
            window.navigateTo(key)
        }
        onOpenPlaylistRequested: function(id) {
            window.openPlaylist(id)
        }
        onCreatePlaylistRequested: {
            window.navigateTo("playlist")
        }
    }

    Rectangle {
        id: mainContent
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: drawerSidebar.right
        anchors.right: parent.right
        anchors.topMargin: titleBar.height
        anchors.bottomMargin: 80        // 给底栏让出空间
        color: "transparent"

        StackView {
            id: stackView
            anchors.fill: parent

            // 不使用 initialItem (URL 方式在构造期加载会导致子组件绑定失败),
            // 改为在 onCompleted 中推入首页,确保所有上下文属性已就绪
            Component.onCompleted: {
                stackView.push(Qt.resolvedUrl("views/HomeView.qml"), StackView.Immediate)
            }

            pushEnter: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 250; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "y"; from: 30; to: 0; duration: 400; easing.type: Easing.OutExpo }
                }
            }
            pushExit: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 200; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "scale"; from: 1; to: 0.96; duration: 300; easing.type: Easing.OutCubic }
                }
            }
            popEnter: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 250; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: 400; easing.type: Easing.OutExpo }
                }
            }
            popExit: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 200; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "y"; from: 0; to: 30; duration: 300; easing.type: Easing.InCubic }
                }
            }
            replaceEnter: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 250; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "y"; from: 15; to: 0; duration: 350; easing.type: Easing.OutExpo }
                    NumberAnimation { property: "scale"; from: 0.98; to: 1; duration: 350; easing.type: Easing.OutExpo }
                }
            }
            replaceExit: Transition {
                ParallelAnimation {
                    NumberAnimation { property: "opacity"; from: 1; to: 0; duration: 200; easing.type: Easing.OutCubic }
                    NumberAnimation { property: "scale"; from: 1; to: 0.98; duration: 250; easing.type: Easing.OutCubic }
                }
            }
        }
    }

    // 移除了抽屉导航和 scrim

    // 底部 MiniPlayer (悬浮胶囊) — 进入「正在播放」视图后自动隐藏，由页内控制栏接管
    MiniPlayer {
        id: miniPlayer
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottomMargin: 0
        height: 80

        // 进入「正在播放」沉浸视图时隐藏，避免与页内控制栏功能/视觉冗余
        property bool onNowPlaying: stackView.currentItem
                                    && stackView.currentItem.objectName === "nowPlayingView"
        opacity: onNowPlaying ? 0 : 1
        visible: opacity > 0.01
        enabled: !onNowPlaying
        Behavior on opacity { NumberAnimation { duration: 220; easing.type: Easing.OutQuad } }

        onClicked: {
            if (stackView.currentItem.objectName !== "nowPlayingView") {
                stackView.push(Qt.resolvedUrl("views/NowPlayingView.qml"))
            }
        }
        onShowQueueClicked: queueDrawer.toggle()
    }

    // 队列抽屉(从右侧滑入)
    QueueDrawer {
        id: queueDrawer
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.topMargin: 16
        anchors.bottomMargin: 100 + 16  // miniPlayer 高度 + 边距
        width: 380
    }

    // ===== 拖拽导入 =====
    DropArea {
        id: dropZone
        anchors.fill: parent
        keys: ["text/uri-list"]

        onEntered: function(drag) {
            // 只接受文件
            drag.accept(Qt.CopyAction)
            dropOverlay.opacity = 1
        }
        onExited: dropOverlay.opacity = 0
        onDropped: function(drop) {
            dropOverlay.opacity = 0
            if (!drop.hasUrls) return
            var paths = []
            for (var i = 0; i < drop.urls.length; ++i) {
                var u = drop.urls[i].toString()
                if (u.match(/\.(wav|flac|mp3|dsf|dff)$/i)) paths.push(u)
            }
            if (paths.length === 0) return
            playerVM.enqueueMany(paths)
            drop.accept(Qt.CopyAction)
        }
    }

    // 拖拽视觉提示
    Rectangle {
        id: dropOverlay
        anchors.fill: parent
        anchors.margins: 8
        radius: 24
        color: "#332563EB"
        border.color: window.brand
        border.width: 2
        visible: opacity > 0
        opacity: 0
        z: 99
        Behavior on opacity { NumberAnimation { duration: 180 } }

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 16

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                width: 96; height: 96; radius: 48
                color: "#EFF6FF"

                AppIcon {
                    anchors.centerIn: parent
                    name: "plus"
                    size: 42
                    color: window.brand
                    strokeWidth: 2
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "松开以加入队列"
                font.family: window.fontFamily
                font.pixelSize: 22
                font.weight: Font.Bold
                color: window.brand
            }
            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "支持 .wav / .flac"
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.brandHover
            }
        }
    }

    // ===== 键盘快捷键 =====
    // 全部 sequence 都来自 shortcutsVM (可在设置中改键).
    // Media Play / Ctrl+/ 等"次要绑定"保留为单独的 Shortcut, 不暴露给改键 UI.

    Shortcut {
        sequence: shortcutsVM.keymap["play_pause"] || "Space"
        onActivated: {
            if (playerVM.state === 2) playerVM.pause()
            else playerVM.play()
        }
    }
    Shortcut {
        sequence: "Media Play"
        onActivated: {
            if (playerVM.state === 2) playerVM.pause()
            else playerVM.play()
        }
    }
    Shortcut {
        sequence: shortcutsVM.keymap["next"] || "Right"
        onActivated: playerVM.next()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["prev"] || "Left"
        onActivated: playerVM.previous()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["vol_up"] || "Up"
        onActivated: playerVM.volume = Math.min(100, playerVM.volume + 5)
    }
    Shortcut {
        sequence: shortcutsVM.keymap["vol_down"] || "Down"
        onActivated: playerVM.volume = Math.max(0,   playerVM.volume - 5)
    }
    Shortcut {
        sequence: shortcutsVM.keymap["mute"] || "M"
        onActivated: playerVM.toggleMute()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["like_current"] || "Ctrl+L"
        onActivated: playerVM.toggleLikeCurrent()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["cycle_repeat"] || "Ctrl+R"
        onActivated: playerVM.cycleRepeatMode()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["toggle_shuffle"] || "Ctrl+S"
        onActivated: playerVM.toggleShuffle()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["toggle_queue"] || "Ctrl+Q"
        onActivated: queueDrawer.toggle()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["open_queue_page"] || "Ctrl+Shift+Q"
        onActivated: window.navigateTo("queue")
    }
    Shortcut {
        sequence: shortcutsVM.keymap["escape"] || "Escape"
        onActivated: {
            if (queueDrawer.open) {
                queueDrawer.hide()
                return
            }
            if (stackView.currentItem && stackView.currentItem.objectName === "nowPlayingView") {
                stackView.pop()
            }
        }
    }
    // 数字键切换主导航 (1=首页 ... 7=喜欢)
    Shortcut { sequence: shortcutsVM.keymap["nav_home"]     || "1"; onActivated: window.navigateTo("home") }
    Shortcut { sequence: shortcutsVM.keymap["nav_library"]  || "2"; onActivated: window.navigateTo("library") }
    Shortcut { sequence: shortcutsVM.keymap["nav_playlist"] || "3"; onActivated: window.navigateTo("playlist") }
    Shortcut { sequence: shortcutsVM.keymap["nav_artist"]   || "4"; onActivated: window.navigateTo("artist") }
    Shortcut { sequence: shortcutsVM.keymap["nav_album"]    || "5"; onActivated: window.navigateTo("album") }
    Shortcut { sequence: shortcutsVM.keymap["nav_history"]  || "6"; onActivated: window.navigateTo("history") }
    Shortcut { sequence: shortcutsVM.keymap["nav_liked"]    || "7"; onActivated: window.navigateTo("liked") }
    Shortcut {
        sequence: shortcutsVM.keymap["nav_settings"] || "Ctrl+,"
        onActivated: window.navigateTo("settings")
    }
    Shortcut {
        sequence: shortcutsVM.keymap["toggle_fullscreen"] || "F11"
        onActivated: {
            if (window.visibility === Window.FullScreen) window.visibility = Window.Windowed
            else window.visibility = Window.FullScreen
        }
    }
    Shortcut {
        sequence: shortcutsVM.keymap["show_shortcuts"] || "F1"
        onActivated: shortcutsDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+/"
        onActivated: shortcutsDialog.open()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["toggle_eq"] || "Ctrl+E"
        onActivated: eqDialog.open()
    }
    Shortcut {
        sequence: shortcutsVM.keymap["open_search"] || "Ctrl+F"
        onActivated: window.openSearch("")
    }
    Shortcut {
        sequence: "Ctrl+K"
        onActivated: window.openSearch("")
    }

    // 快捷键帮助对话框
    ShortcutsDialog {
        id: shortcutsDialog
    }
    function showShortcuts() { shortcutsDialog.open() }

    // 均衡器
    EqDialog {
        id: eqDialog
    }
    function showEq() { eqDialog.open() }

    // 错误 Toast
    Rectangle {
        id: toast
        property string message: ""
        visible: opacity > 0
        opacity: 0
        anchors.top: parent.top
        anchors.topMargin: 24
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(560, parent.width - 64)
        height: 44
        radius: 22
        color: "#F87171"
        border.color: "#FECACA"
        border.width: 1

        Behavior on opacity { NumberAnimation { duration: 220 } }

        Text {
            anchors.centerIn: parent
            anchors.margins: 16
            text: toast.message
            color: "#FFFFFF"
            font.family: window.fontFamily
            font.pixelSize: 13
            elide: Text.ElideRight
            width: parent.width - 32
            horizontalAlignment: Text.AlignHCenter
        }

        Timer {
            id: toastTimer
            interval: 2800
            onTriggered: toast.opacity = 0
        }
    }

    Connections {
        target: playerVM
        function onErrorOccurred(msg) {
            if (!msg) return
            toast.message = msg
            toast.opacity = 1
            toastTimer.restart()
        }
    }
}
