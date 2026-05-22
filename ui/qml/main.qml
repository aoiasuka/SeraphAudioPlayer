import QtQuick
import QtQuick.Controls
import QtQuick.Window
import QtQuick.Layouts
import "views"
import "components"

ApplicationWindow {
    id: window
    flags: Qt.Window | Qt.FramelessWindowHint | Qt.CustomizeWindowHint
    width: 1280
    height: 800
    minimumWidth: 1024
    minimumHeight: 640
    visible: true
    title: qsTr("Audio Player X86")
    color: "transparent"

    TitleBar {
        id: titleBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        targetWindow: window
        z: 9999
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
    // 全局背景改为动态背景，这里提供备用色
    readonly property color appBg: "#F0F4F8"
    // 毛玻璃侧边栏和卡片背景 (半透明)
    readonly property color sidebarBg: "#88FFFFFF"
    readonly property color surface: "#99FFFFFF"
    readonly property color hoverBg: "#33FFFFFF"
    readonly property color cardHover: "#E6FFFFFF"
    readonly property color activeBg: "#B3DBEAFE"
    readonly property color playerBg: "#DDF8F9FA"

    readonly property color textPrimary: "#111827"
    readonly property color textSecondary: "#4B5563"
    readonly property color textTertiary: "#9CA3AF"

    readonly property color brand: "#3B82F6"
    readonly property color brandHover: "#2563EB"
    readonly property color brandPress: "#1D4ED8"
    readonly property color brandSoft: "#BFDBFE"

    readonly property color heroTop: "#2563EB"
    readonly property color heroBottom: "#4F46E5"

    readonly property color borderColor: "#33E5E7EB"
    readonly property color divider: "#22000000"
    readonly property color likeRed: "#EF4444"

    // ===== Design tokens: Glass / Menu / Radii =====
    // 用于弹窗、菜单、抽屉等浮层的统一毛玻璃材质 (亮玻璃)
    // 注意: 真正的高斯模糊需要 GraphicsEffects, 这里采用"高不透明度浅色"近似毛玻璃,
    //       足以遮挡底层文字, 同时保留淡淡的色调过渡感
    readonly property color glassBg: "#E8F7F8FC"     // 浅色弹窗主体: ~91% 不透明, 避免底层文字穿透
    readonly property color glassBgSoft: "#A8FFFFFF" // 抽屉/侧边栏可继续使用的半透明
    // 深色版毛玻璃 (用于右键菜单、深色弹层)
    readonly property color menuBg: "#E81F2937"      // ~91% 不透明深色, 杜绝下层文字干扰
    readonly property color menuHoverBg: "#553B82F6"
    // 玻璃浮层统一描边
    readonly property color glassBorder: "#33FFFFFF"
    readonly property color glassBorderDark: "#22000000"
    // 模态遮罩 (压暗底层, 让焦点集中到弹窗)
    readonly property color modalScrim: "#99000000"  // 60% 黑色, 强力遮挡

    // 圆角令牌
    readonly property int smallRadius: 8
    readonly property int mediumRadius: 12
    readonly property int largeRadius: 16
    readonly property int xLargeRadius: 20

    readonly property string fontFamily: "Microsoft YaHei UI"

    // MiniPlayer 玻璃背景使用此别名抓取动态背景做 backdrop blur
    property alias backdropItem: dynamicBg

    // ===== 封面主色染窗：当前曲目主色驱动整窗渐变 (1.5s 平滑过渡) =====
    // 主色：直接来自 C++ 端 (24×24 像素平均 + HSV 调整)；
    // 辅色：QML 端从主色派生 (色相 +28°，饱和度微降，亮度微降)，形成同调对角渐变。
    property color domColor1: playerVM.currentDominantColor || "#1E40AF"
    property color domColor2: {
        var h = domColor1.hslHue
        if (h < 0) h = 0.6   // 无封面时给一个蓝紫
        h = (h + 0.078) % 1.0
        var s = Math.min(1.0, domColor1.hslSaturation * 0.85 + 0.1)
        var l = Math.max(0.20, Math.min(0.55, domColor1.hslLightness - 0.06))
        return Qt.hsla(h, s, l, 1.0)
    }
    Behavior on domColor1 { ColorAnimation { duration: 1500; easing.type: Easing.InOutQuad } }
    Behavior on domColor2 { ColorAnimation { duration: 1500; easing.type: Easing.InOutQuad } }

    // ===== 动态主色背景 =====
    // 三层叠加：① 主色对角渐变  ② 白色雾化层(保证文字对比度)  ③ 极淡浮动光晕(保留呼吸感)
    // 注: MiniPlayer 通过 window.dynamicBg 引用此项做 backdrop blur
    Rectangle {
        id: dynamicBg
        anchors.fill: parent
        radius: 16
        antialiasing: true
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0.0; color: window.domColor1 }
            GradientStop { position: 1.0; color: window.domColor2 }
        }

        // 白色雾化层 —— 上浓下淡的纵向白幕，叠出"被照亮"的对角感，
        // 同时把主色饱和度压到适合长时间观看的水平，保证文字 4.5:1 对比度
        Rectangle {
            anchors.fill: parent
            radius: parent.radius
            antialiasing: true
            gradient: Gradient {
                GradientStop { position: 0.0; color: "#A8FFFFFF" }
                GradientStop { position: 1.0; color: "#55FFFFFF" }
            }
        }

        // 浮动光晕 (保留呼吸感,变为极淡白色斑,不再抢主色风头)
        Rectangle {
            width: 800; height: 800; radius: 400
            color: "#33FFFFFF"
            x: -200; y: -200
            SequentialAnimation on x {
                loops: Animation.Infinite
                NumberAnimation { to: 200; duration: 25000; easing.type: Easing.InOutSine }
                NumberAnimation { to: -200; duration: 25000; easing.type: Easing.InOutSine }
            }
        }
        Rectangle {
            width: 700; height: 700; radius: 350
            color: "#2BFFFFFF"
            x: window.width - 400; y: window.height - 300
            SequentialAnimation on y {
                loops: Animation.Infinite
                NumberAnimation { to: window.height - 500; duration: 20000; easing.type: Easing.InOutSine }
                NumberAnimation { to: window.height - 300; duration: 20000; easing.type: Easing.InOutSine }
            }
            SequentialAnimation on x {
                loops: Animation.Infinite
                NumberAnimation { to: window.width - 600; duration: 22000; easing.type: Easing.InOutSine }
                NumberAnimation { to: window.width - 400; duration: 22000; easing.type: Easing.InOutSine }
            }
        }
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
        default:         return Qt.resolvedUrl("views/HomeView.qml")
        }
    }

    // ===== 主布局：左侧栏 + 右内容 (悬浮岛风格) =====
    RowLayout {
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.topMargin: titleBar.height + 4
        anchors.bottomMargin: 100  // 给底部悬浮播放器留位
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 16

        // 左侧栏 (悬浮)
        Sidebar {
            id: sidebar
            Layout.preferredWidth: 220
            Layout.fillHeight: true
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

        // 主内容区 (悬浮)
        Rectangle {
            Layout.fillWidth: true
            Layout.fillHeight: true
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
                    PropertyAnimation { property: "opacity"; from: 0; to: 1; duration: 200 }
                    PropertyAnimation { property: "y"; from: 16; to: 0; duration: 250; easing.type: Easing.OutQuart }
                }
                pushExit: Transition {
                    SequentialAnimation {
                        PropertyAnimation { property: "opacity"; from: 1; to: 0; duration: 150 }
                        PropertyAction { property: "visible"; value: false }
                    }
                }
                popEnter: Transition {
                    PropertyAnimation { property: "opacity"; from: 0; to: 1; duration: 200 }
                }
                popExit: Transition {
                    SequentialAnimation {
                        ParallelAnimation {
                            PropertyAnimation { property: "opacity"; from: 1; to: 0; duration: 150 }
                            PropertyAnimation { property: "y"; from: 0; to: 16; duration: 150; easing.type: Easing.InQuart }
                        }
                        PropertyAction { property: "visible"; value: false }
                    }
                }
                replaceEnter: Transition {
                    PropertyAnimation { property: "opacity"; from: 0; to: 1; duration: 200 }
                    PropertyAnimation { property: "y"; from: 16; to: 0; duration: 250; easing.type: Easing.OutQuart }
                }
                replaceExit: Transition {
                    SequentialAnimation {
                        PropertyAnimation { property: "opacity"; from: 1; to: 0; duration: 150 }
                        PropertyAction { property: "visible"; value: false }
                    }
                }
            }
        }
    }

    // 底部 MiniPlayer (悬浮岛) — 进入「正在播放」视图后自动隐藏，由页内控制栏接管
    MiniPlayer {
        id: miniPlayer
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottomMargin: 16
        anchors.leftMargin: 24
        anchors.rightMargin: 24
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
    // 播放控制
    Shortcut {
        sequences: ["Space", "Media Play"]
        onActivated: {
            if (playerVM.state === 2) playerVM.pause()
            else playerVM.play()
        }
    }
    Shortcut {
        sequence: "Right"
        onActivated: playerVM.next()
    }
    Shortcut {
        sequence: "Left"
        onActivated: playerVM.previous()
    }
    Shortcut {
        sequence: "Up"
        onActivated: playerVM.volume = Math.min(100, playerVM.volume + 5)
    }
    Shortcut {
        sequence: "Down"
        onActivated: playerVM.volume = Math.max(0,   playerVM.volume - 5)
    }
    Shortcut {
        sequence: "M"
        onActivated: playerVM.toggleMute()
    }
    Shortcut {
        sequence: "Ctrl+L"
        onActivated: playerVM.toggleLikeCurrent()
    }
    Shortcut {
        sequence: "Ctrl+R"
        onActivated: playerVM.cycleRepeatMode()
    }
    Shortcut {
        sequence: "Ctrl+S"
        onActivated: playerVM.toggleShuffle()
    }
    Shortcut {
        sequence: "Ctrl+Q"
        onActivated: queueDrawer.toggle()
    }
    Shortcut {
        sequence: "Escape"
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
    Shortcut { sequence: "1"; onActivated: window.navigateTo("home") }
    Shortcut { sequence: "2"; onActivated: window.navigateTo("library") }
    Shortcut { sequence: "3"; onActivated: window.navigateTo("playlist") }
    Shortcut { sequence: "4"; onActivated: window.navigateTo("artist") }
    Shortcut { sequence: "5"; onActivated: window.navigateTo("album") }
    Shortcut { sequence: "6"; onActivated: window.navigateTo("history") }
    Shortcut { sequence: "7"; onActivated: window.navigateTo("liked") }
    Shortcut { sequence: "Ctrl+Comma"; onActivated: window.navigateTo("settings") }
    Shortcut {
        sequence: "F11"
        onActivated: {
            if (window.visibility === Window.FullScreen) window.visibility = Window.Windowed
            else window.visibility = Window.FullScreen
        }
    }
    Shortcut {
        sequences: ["F1", "Ctrl+/"]
        onActivated: shortcutsDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+E"
        onActivated: eqDialog.open()
    }
    // 全局搜索: 跳转到搜索结果视图 (空 query 也可, 让用户在新页输入)
    Shortcut {
        sequences: ["Ctrl+F", "Ctrl+K"]
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
