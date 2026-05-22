import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects

// 底部播放条 — iOS 风半透明玻璃悬浮岛
// 设计:
//   - 真实 backdrop blur (抓取 window.backdropItem 做高斯模糊)
//   - 半透明白色雾化叠加, 保证文字对比度
//   - 1px 白色高光内边框, 玻璃边缘质感
//   - 进度条 = 1.5px 极细线, 贴卡片顶边缘, handle 仅 hover 时显现
//   - 左: 封面 + 歌名/格式 + 喜欢
//   - 中: 5 个控制按钮
//   - 右: 音量 + 队列
Rectangle {
    id: root
    color: "transparent"                  // 由内部 glass 层负责着色
    radius: 24
    clip: false                           // handle 上溢卡片顶部需保留, 玻璃形状由 glassLayer 自己 mask
    layer.enabled: true

    // ===== iOS 玻璃背景层 (backdrop blur + 雾化) =====
    Item {
        id: glassLayer
        anchors.fill: parent

        // ① 抓取窗口动态背景 (主色渐变 + 雾化 + 光晕)
        ShaderEffectSource {
            id: backdropSrc
            anchors.fill: parent
            sourceItem: window.backdropItem
            sourceRect: Qt.rect(root.x, root.y, root.width, root.height)
            textureSize: Qt.size(root.width * 0.5, root.height * 0.5)  // 半分辨率, blur 看不出差异
            live: true
            recursive: false
            smooth: true
            visible: false                // 仅供 effect 使用
        }

        // ② 高斯模糊 + 轻微提升饱和度 (iOS vibrancy 感)
        MultiEffect {
            anchors.fill: parent
            source: backdropSrc
            blurEnabled: true
            blur: 1.0
            blurMax: 64
            blurMultiplier: 1.0
            saturation: 0.3
        }

        // ③ 半透明白色雾化层 (material light) — 较透,让背景色透出更明显
        Rectangle {
            anchors.fill: parent
            gradient: Gradient {
                GradientStop { position: 0.0; color: "#80FFFFFF" }
                GradientStop { position: 1.0; color: "#55FFFFFF" }
            }
        }

        // 圆角裁切: 用 MultiEffect mask 把玻璃层切成圆角
        layer.enabled: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: ShaderEffectSource {
                sourceItem: glassMask
                hideSource: true
            }
        }
    }

    // 用作 mask 的圆角矩形 (放在裁切目标外, 不参与显示)
    Rectangle {
        id: glassMask
        width: root.width
        height: root.height
        radius: root.radius
        color: "black"
        antialiasing: true
        visible: false
    }

    // ④ 玻璃边缘高光描边 (1px 半透明白)
    Rectangle {
        anchors.fill: parent
        radius: root.radius
        color: "transparent"
        border.color: "#66FFFFFF"
        border.width: 1
    }

    // 启动时上移淡入
    opacity: 0
    transform: Translate { id: enterT; y: 24 }
    Component.onCompleted: enterAnim.start()
    ParallelAnimation {
        id: enterAnim
        NumberAnimation { target: root; property: "opacity"; from: 0; to: 1; duration: 260; easing.type: Easing.OutQuart }
        NumberAnimation { target: enterT; property: "y"; from: 24; to: 0; duration: 320; easing.type: Easing.OutQuart }
    }

    signal clicked()
    signal showQueueClicked()

    function formatTime(seconds) {
        if (!seconds || seconds < 0) return "00:00"
        var m = Math.floor(seconds / 60)
        var s = Math.floor(seconds % 60)
        return (m < 10 ? "0" + m : m) + ":" + (s < 10 ? "0" + s : s)
    }

    // ===== 进度条 =====
    // background 真贴卡片顶边 y=0 (沿卡片顶部直线段)
    // 左右 inset = root.radius, 两端正好对齐左上/右上圆角弧线端点
    // handle 中心严格对齐 background 中心 (hover 时 handle 上溢卡片顶 ~4px 形成"探出"视觉)
    Slider {
        id: progressSlider
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.topMargin: 0
        anchors.leftMargin: root.radius
        anchors.rightMargin: root.radius
        height: 14
        from: 0
        to: playerVM.duration > 0 ? playerVM.duration : 1
        value: playerVM.position

        onMoved: playerVM.seek(value)

        property bool barHovered: progressHover.hovered || pressed

        background: Rectangle {
            id: progressBg
            x: 0
            y: 0                          // 真贴卡片顶边
            width: progressSlider.availableWidth
            height: progressSlider.barHovered ? 4 : 2.5
            radius: height / 2
            color: "#1F0F172A"
            Behavior on height { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }

            // 进度填充: 品牌色渐变 (蓝 → 紫, 呼应窗口主色)
            Rectangle {
                width: progressSlider.visualPosition * parent.width
                height: parent.height
                radius: parent.radius
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0.0; color: window.brand }
                    GradientStop { position: 1.0; color: window.heroBottom }
                }
            }
        }

        handle: Rectangle {
            x: progressSlider.leftPadding + progressSlider.visualPosition * (progressSlider.availableWidth - width)
            y: progressBg.height / 2 - height / 2    // 中心对齐 background 中心 (hover 时为负, 上溢卡片顶)
            width: progressSlider.barHovered ? 12 : 0
            height: width
            radius: width / 2
            color: "#FFFFFF"
            border.color: window.brand
            border.width: 1.5
            Behavior on width { NumberAnimation { duration: 160; easing.type: Easing.OutQuart } }
        }

        HoverHandler { id: progressHover }
    }

    // 进度条下方留出 8px 后再渲染主行
    RowLayout {
        anchors.fill: parent
        anchors.topMargin: 8              // 给细进度条让出空间
        anchors.leftMargin: 20
        anchors.rightMargin: 28
        spacing: 0

        // ===== 左: 封面 + 歌名/格式 + 喜欢 =====
        RowLayout {
            Layout.preferredWidth: 320
            spacing: 14

            // 封面
            Rectangle {
                Layout.preferredWidth: 52
                Layout.preferredHeight: 52
                radius: 10
                color: "#F1F5F9"          // slate-100
                border.color: "#140F172A"
                border.width: 1
                clip: true

                Rectangle {
                    id: miniPlayerCoverImgMask
                    width: miniPlayerCoverImg.width
                    height: miniPlayerCoverImg.height
                    radius: 10
                    color: "black"
                    antialiasing: true
                }

                Image {
                    id: miniPlayerCoverImg
                    anchors.fill: parent
                    source: playerVM.currentCoverUrl
                    visible: source.toString().length > 0 && status === Image.Ready
                    fillMode: Image.PreserveAspectCrop
                    asynchronous: true
                    cache: true

                    layer.enabled: true
                    layer.effect: MultiEffect {
                        maskEnabled: true
                        maskSource: ShaderEffectSource {
                            sourceItem: miniPlayerCoverImgMask
                            hideSource: true
                        }
                    }
                }

                AppIcon {
                    anchors.centerIn: parent
                    visible: !playerVM.currentCoverUrl
                    name: "music"
                    size: 20
                    color: "#94A3B8"
                    strokeWidth: 1.5
                }

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.clicked()
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2

                Text {
                    Layout.fillWidth: true
                    text: playerVM.title && playerVM.title !== "未播放" ? playerVM.title : ""
                    font.family: window.fontFamily
                    font.pixelSize: 14
                    font.weight: Font.DemiBold
                    color: "#0F172A"
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    text: playerVM.formatInfo !== "" ? playerVM.formatInfo : ""
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: "#64748B"
                    elide: Text.ElideRight
                }
            }

            // 喜欢
            Item {
                Layout.preferredWidth: 34
                Layout.preferredHeight: 34

                Rectangle {
                    anchors.fill: parent
                    radius: 17
                    color: likeArea.containsMouse ? "#140F172A" : "transparent"
                    Behavior on color { ColorAnimation { duration: 120 } }
                }

                AppIcon {
                    anchors.centerIn: parent
                    name: "heart"
                    size: 17
                    color: playerVM.currentLiked ? window.likeRed
                         : (likeArea.containsMouse ? "#475569" : "#94A3B8")
                    filled: playerVM.currentLiked
                    strokeWidth: 1.8
                }

                MouseArea {
                    id: likeArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    enabled: playerVM.title && playerVM.title !== "未播放"
                    onClicked: playerVM.toggleLikeCurrent()
                }
            }
        }

        // ===== 中: 控制按钮 (单行) =====
        RowLayout {
            Layout.fillWidth: true
            Layout.alignment: Qt.AlignHCenter
            spacing: 22

            Item { Layout.fillWidth: true }

            // shuffle
            IconCircleBtn {
                iconName: "shuffle"; size: 32; iconSize: 16
                iconColor: playerVM.shuffle ? "#0F172A" : "#94A3B8"
                onClicked: playerVM.toggleShuffle()
            }

            // prev
            IconCircleBtn {
                iconName: "prev"; size: 36; iconSize: 18
                iconColor: "#0F172A"
                iconFilled: true
                strokeWidthOverride: 0
                onClicked: playerVM.previous()
            }

            // 主播放/暂停 (深色精细胶囊)
            Rectangle {
                Layout.preferredWidth: 46
                Layout.preferredHeight: 46
                radius: 23
                color: playArea.pressed ? "#000000"
                     : (playArea.containsMouse ? "#0F172A" : "#1E293B")
                Behavior on color { ColorAnimation { duration: 150 } }

                scale: playArea.containsMouse ? 1.04 : 1.0
                Behavior on scale { NumberAnimation { duration: 160; easing.type: Easing.OutQuad } }

                AppIcon {
                    anchors.centerIn: parent
                    anchors.horizontalCenterOffset: playerVM.state === 2 ? 0 : 1.5
                    name: playerVM.state === 2 ? "pause" : "play"
                    size: 18
                    color: "#FFFFFF"
                    filled: true
                }

                MouseArea {
                    id: playArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        if (playerVM.state === 2) playerVM.pause()
                        else playerVM.play()
                    }
                }
            }

            // next
            IconCircleBtn {
                iconName: "next"; size: 36; iconSize: 18
                iconColor: "#0F172A"
                iconFilled: true
                strokeWidthOverride: 0
                onClicked: playerVM.next()
            }

            // repeat
            IconCircleBtn {
                iconName: "repeat"; size: 32; iconSize: 16
                iconColor: playerVM.repeatMode > 0 ? "#0F172A" : "#94A3B8"
                badgeText: playerVM.repeatMode === 2 ? "1" : ""
                onClicked: playerVM.cycleRepeatMode()
            }

            Item { Layout.fillWidth: true }
        }

        // ===== 右: 时间 + 音量 + 队列 =====
        RowLayout {
            Layout.preferredWidth: 300
            Layout.alignment: Qt.AlignRight
            spacing: 12

            Item { Layout.fillWidth: true }

            // 紧凑时间显示 (取代原来中央长时间块)
            Text {
                text: root.formatTime(playerVM.position) + "  /  " + root.formatTime(playerVM.duration)
                font.family: window.fontFamily
                font.pixelSize: 11
                font.weight: Font.Medium
                color: "#94A3B8"
            }

            // 音量
            AppIcon {
                name: playerVM.muted || playerVM.volume === 0 ? "volume-mute" : "volume"
                size: 17
                color: "#64748B"
                strokeWidth: 1.8

                MouseArea {
                    anchors.fill: parent
                    cursorShape: Qt.PointingHandCursor
                    onClicked: playerVM.toggleMute()
                }
            }

            Slider {
                id: volumeSlider
                Layout.preferredWidth: 96
                from: 0; to: 100
                value: playerVM.volume

                onMoved: {
                    playerVM.volume = Math.round(value)
                    if (playerVM.muted && value > 0) playerVM.muted = false
                }

                property bool barHovered: volHover.hovered || pressed

                background: Rectangle {
                    x: volumeSlider.leftPadding
                    y: volumeSlider.topPadding + volumeSlider.availableHeight / 2 - height / 2
                    width: volumeSlider.availableWidth
                    height: 2
                    radius: 1
                    color: "#1A0F172A"

                    Rectangle {
                        width: volumeSlider.visualPosition * parent.width
                        height: parent.height
                        radius: parent.radius
                        color: "#1E293B"
                    }
                }

                handle: Rectangle {
                    x: volumeSlider.leftPadding + volumeSlider.visualPosition * (volumeSlider.availableWidth - width)
                    y: volumeSlider.topPadding + (volumeSlider.availableHeight - height) / 2
                    width: volumeSlider.barHovered ? 10 : 0
                    height: width
                    radius: width / 2
                    color: "#0F172A"
                    Behavior on width { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }
                }

                HoverHandler { id: volHover }
            }

            // 队列按钮
            Item {
                Layout.preferredWidth: 34
                Layout.preferredHeight: 34
                Rectangle {
                    anchors.fill: parent
                    radius: 17
                    color: qArea.containsMouse ? "#140F172A" : "transparent"
                    Behavior on color { ColorAnimation { duration: 120 } }
                }
                AppIcon {
                    anchors.centerIn: parent
                    name: "list"
                    size: 17
                    color: qArea.containsMouse ? "#0F172A" : "#64748B"
                    strokeWidth: 1.8
                }
                MouseArea {
                    id: qArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: root.showQueueClicked()
                }
            }
        }
    }
}
