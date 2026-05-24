import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects

// 底部播放条 — 极简纯色悬浮胶囊
// 设计:
//   - 纯白卡片 + 柔和投影, 在米色主背景上"悬空"
//   - 1px 极细描边 (10% 黑), 让边缘清晰
//   - 极简单色进度条 (深灰), handle 仅 hover 时显现
//   - 左: 封面 + 歌名/格式 + 喜欢
//   - 中: 5 个控制按钮
//   - 右: 音量 + 队列
Item {
    id: root
    clip: false

    // 通过 alias 让外部继续用 .radius 调整
    property real radius: 0

    signal clicked()
    signal showQueueClicked()

    // ===== 投影层 (放在面板之下) =====
    Rectangle {
        id: shadowSrc
        anchors.fill: parent
        radius: root.radius
        color: window.surface
        visible: false
        layer.enabled: true
        layer.smooth: true
    }
    MultiEffect {
        anchors.fill: shadowSrc
        source: shadowSrc
        shadowEnabled: true
        shadowColor: window.shadowColor
        shadowBlur: 1.0
        shadowVerticalOffset: 6
        shadowHorizontalOffset: 0
    }

    // ===== 主面板: 纯白胶囊 + 极细描边 =====
    Rectangle {
        id: panel
        anchors.fill: parent
        radius: root.radius
        color: window.surface
        border.color: window.borderColor
        border.width: 1
        antialiasing: true
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

    function formatTime(seconds) {
        if (!seconds || seconds < 0) return "00:00"
        var m = Math.floor(seconds / 60)
        var s = Math.floor(seconds % 60)
        return (m < 10 ? "0" + m : m) + ":" + (s < 10 ? "0" + s : s)
    }

    // ===== 进度条 (极简单色细线) =====
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
        Binding {
            target: progressSlider
            property: "value"
            value: playerVM.position
            when: !progressSlider.pressed
        }

        onPressedChanged: {
            if (!pressed) playerVM.seek(value)
        }

        property bool barHovered: progressHover.hovered || pressed

        background: Rectangle {
            id: progressBg
            x: 0
            y: 0
            width: progressSlider.availableWidth
            height: progressSlider.barHovered ? 3 : 2
            radius: height / 2
            color: "#1A000000"          // 极细灰线
            Behavior on height { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }

            // 进度填充: 单色深灰, 与"极简"基调一致
            Rectangle {
                width: progressSlider.visualPosition * parent.width
                height: parent.height
                radius: parent.radius
                color: window.textPrimary
            }
        }

        handle: Rectangle {
            x: progressSlider.leftPadding + progressSlider.visualPosition * (progressSlider.availableWidth - width)
            y: progressBg.height / 2 - height / 2
            width: progressSlider.barHovered ? 10 : 0
            height: width
            radius: width / 2
            color: window.textPrimary
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
                color: window.surfaceAlt
                border.color: window.borderColor
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
                    color: window.textTertiary
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
                    color: window.textPrimary
                    elide: Text.ElideRight
                }
                Text {
                    Layout.fillWidth: true
                    text: playerVM.formatInfo !== "" ? playerVM.formatInfo : ""
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: window.textSecondary
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
                    color: likeArea.containsMouse ? window.hoverBg : "transparent"
                    Behavior on color { ColorAnimation { duration: 120 } }
                }

                AppIcon {
                    anchors.centerIn: parent
                    name: "heart"
                    size: 17
                    color: playerVM.currentLiked ? window.likeRed
                         : (likeArea.containsMouse ? window.textSecondary : window.textTertiary)
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
                iconColor: playerVM.shuffle ? window.textPrimary : window.textTertiary
                onClicked: playerVM.toggleShuffle()
            }

            // prev
            IconCircleBtn {
                iconName: "prev"; size: 36; iconSize: 18
                iconColor: window.textPrimary
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
                     : (playArea.containsMouse ? "#0F0F11" : window.textPrimary)
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
                iconColor: window.textPrimary
                iconFilled: true
                strokeWidthOverride: 0
                onClicked: playerVM.next()
            }

            // repeat
            IconCircleBtn {
                iconName: "repeat"; size: 32; iconSize: 16
                iconColor: playerVM.repeatMode > 0 ? window.textPrimary : window.textTertiary
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
                color: window.textTertiary
            }

            // 音量
            AppIcon {
                name: playerVM.muted || playerVM.volume === 0 ? "volume-mute" : "volume"
                size: 17
                color: window.textSecondary
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
                    color: "#1A000000"

                    Rectangle {
                        width: volumeSlider.visualPosition * parent.width
                        height: parent.height
                        radius: parent.radius
                        color: window.textPrimary
                    }
                }

                handle: Rectangle {
                    x: volumeSlider.leftPadding + volumeSlider.visualPosition * (volumeSlider.availableWidth - width)
                    y: volumeSlider.topPadding + (volumeSlider.availableHeight - height) / 2
                    width: volumeSlider.barHovered ? 10 : 0
                    height: width
                    radius: width / 2
                    color: window.textPrimary
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
                    color: qArea.containsMouse ? window.hoverBg : "transparent"
                    Behavior on color { ColorAnimation { duration: 120 } }
                }
                AppIcon {
                    anchors.centerIn: parent
                    name: "list"
                    size: 17
                    color: qArea.containsMouse ? window.textPrimary : window.textSecondary
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
