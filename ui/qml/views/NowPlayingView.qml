import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import "../components"
import QtQuick.Effects

Item {
    id: root
    objectName: "nowPlayingView"

    // 是否有正在播放/可播放的曲目: 队列已加载到某一首即视为"有曲目"
    // 这是整个视图条件渲染的核心开关
    readonly property bool hasTrack: playerVM.currentIndex >= 0

    function formatTime(seconds) {
        if (seconds < 0) return "00:00"
        var m = Math.floor(seconds / 60)
        var s = Math.floor(seconds % 60)
        return (m < 10 ? "0" + m : m) + ":" + (s < 10 ? "0" + s : s)
    }

    TrackContextMenu {
        id: ctxMenu
    }

    // 视图模式: "cover" | "lyrics"
    property string viewMode: "cover"

    // 背景透明, 主窗口的护眼米白背景自然透过
    // (旧版叠加白雾化层是为压低紫色渐变的饱和度, 新版主背景已是浅色, 无需再加层)

    // 顶部栏
    RowLayout {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 20
        height: 44
        spacing: 8

        SidebarIconButton {
            iconName: "chevron"
            iconSize: 18
            iconColor: window.textPrimary
            implicitWidth: 36
            implicitHeight: 36
            onClicked: root.StackView.view.pop()
            // 反向显示 chevron
            rotation: 180
        }

        Item { Layout.fillWidth: true }

        // 视图切换按钮(封面 / 歌词)
        Rectangle {
            visible: root.hasTrack
            Layout.preferredHeight: 32
            radius: 16
            color: window.sidebarBg
            border.color: window.borderColor
            border.width: 1
            Layout.preferredWidth: segRow.implicitWidth + 8

            RowLayout {
                id: segRow
                anchors.centerIn: parent
                spacing: 0
                Repeater {
                    model: [
                        { v: "cover",  label: "封面" },
                        { v: "lyrics", label: "歌词" },
                        { v: "viz",    label: "频谱" }
                    ]
                    delegate: Rectangle {
                        Layout.preferredWidth: 50
                        Layout.preferredHeight: 26
                        radius: 13
                        color: root.viewMode === modelData.v ? window.brand : "transparent"
                        Behavior on color { ColorAnimation { duration: 150 } }

                        Text {
                            anchors.centerIn: parent
                            text: modelData.label
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                            color: root.viewMode === modelData.v ? "#FFFFFF" : window.textPrimary
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.viewMode = modelData.v
                        }
                    }
                }
            }
        }

        SidebarIconButton {
            iconName: "more"
            iconSize: 18
            iconColor: window.textPrimary
            implicitWidth: 36
            implicitHeight: 36
            onClicked: {
                if (playerVM.title && playerVM.title !== "未播放" && playerVM.queue.length > 0) {
                    var cur = playerVM.queue[playerVM.currentIndex]
                    if (cur) ctxMenu.openFor(cur.path)
                }
            }
        }
    }

    // 封面
    Rectangle {
        id: coverArt
        visible: root.hasTrack && root.viewMode === "cover"
        anchors.top: topBar.bottom
        anchors.topMargin: 40
        anchors.horizontalCenter: parent.horizontalCenter
        width: Math.min(parent.width * 0.6, parent.height * 0.5)
        height: width
        radius: 16
        color: window.textPrimary
        clip: true

        // 内嵌封面图(若有)
        Rectangle {
            id: coverImgMask
            width: coverImg.width
            height: coverImg.height
            radius: 16
            color: "black"
            antialiasing: true
        }

        Image {
            id: coverImg
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
                    sourceItem: coverImgMask
                    hideSource: true
                }
            }
        }

        // 黑胶纹理(无封面时显示)
        Rectangle {
            id: vinyl
            visible: !coverImg.visible
            anchors.centerIn: parent
            width: parent.width * 0.75
            height: width
            radius: width / 2
            color: "#0B0F14"

            // 播放时旋转
            RotationAnimation on rotation {
                id: rot
                from: 0; to: 360
                duration: 8000
                loops: Animation.Infinite
                running: playerVM.state === 2 && !coverImg.visible
            }

            Repeater {
                model: 5
                Rectangle {
                    anchors.centerIn: parent
                    width: parent.width - 16 - index * 22
                    height: width
                    radius: width / 2
                    color: "transparent"
                    border.color: "#16202C"
                    border.width: 1
                }
            }

            Rectangle {
                anchors.centerIn: parent
                width: parent.width * 0.3
                height: width
                radius: width / 2
                color: "#2563EB"
            }
        }

        // 软阴影
        Rectangle {
            anchors.fill: parent
            anchors.margins: -8
            anchors.verticalCenterOffset: 6
            z: -1
            radius: 24
            color: "#000000"
            opacity: 0.06
        }
    }

    // 歌词视图(占据封面同位置)
    LyricsView {
        id: lyricsArea
        visible: root.hasTrack && root.viewMode === "lyrics"
        anchors.top: topBar.bottom
        anchors.topMargin: 24
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 32
        anchors.rightMargin: 32
        height: Math.min(parent.width * 0.6, parent.height * 0.5)
    }

    // 频谱视图(占据封面同位置)
    SpectrumView {
        id: vizArea
        visible: root.hasTrack && root.viewMode === "viz"
        anchors.top: topBar.bottom
        anchors.topMargin: 40
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 48
        anchors.rightMargin: 48
        height: Math.min(parent.width * 0.6, parent.height * 0.5)
    }

    // 歌曲信息
    ColumnLayout {
        id: songInfoCol
        visible: root.hasTrack
        anchors.top: coverArt.bottom
        anchors.topMargin: 32
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 32
        anchors.rightMargin: 32
        spacing: 6

        RowLayout {
            Layout.fillWidth: true
            spacing: 8

            Item { Layout.fillWidth: true }

            Text {
                text: playerVM.title
                font.family: "Microsoft YaHei UI"
                font.pixelSize: 24
                font.weight: Font.Bold
                color: window.textPrimary
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignHCenter
            }

            // 喜欢按钮
            Item {
                Layout.preferredWidth: 36
                Layout.preferredHeight: 36

                Rectangle {
                    anchors.fill: parent
                    radius: 18
                    color: likeArea.containsMouse ? window.hoverBg : "transparent"
                    Behavior on color { ColorAnimation { duration: 120 } }
                }

                AppIcon {
                    anchors.centerIn: parent
                    name: "heart"
                    size: 22
                    color: playerVM.currentLiked ? window.likeRed : window.textSecondary
                    filled: playerVM.currentLiked
                    strokeWidth: 1.8
                }

                MouseArea {
                    id: likeArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: playerVM.toggleLikeCurrent()
                }
            }

            Item { Layout.fillWidth: true }
        }

        Text {
            text: playerVM.formatInfo
            font.family: "Microsoft YaHei UI"
            font.pixelSize: 14
            color: window.textSecondary
            elide: Text.ElideRight
            Layout.fillWidth: true
            horizontalAlignment: Text.AlignHCenter
        }
    }

    // 上下首预览(位于进度条上方)
    RowLayout {
        id: prevNextRow
        visible: root.hasTrack
        anchors.bottom: progressCol.top
        anchors.bottomMargin: 12
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 48
        anchors.rightMargin: 48
        spacing: 12

        // 上一首
        Item {
            Layout.preferredHeight: 44
            Layout.preferredWidth: 220
            visible: prevItem !== null
            property var prevItem: {
                var q = playerVM.queue || []
                var i = playerVM.currentIndex - 1
                return (i >= 0 && i < q.length) ? q[i] : null
            }

            Rectangle {
                anchors.fill: parent
                radius: 22
                color: prevArea.containsMouse ? window.cardHover : window.sidebarBg
                border.color: window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }
            }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 6
                anchors.rightMargin: 12
                spacing: 8

                Rectangle {
                    Layout.preferredWidth: 32
                    Layout.preferredHeight: 32
                    radius: 16
                    color: window.borderColor
                    clip: true
                    Rectangle {
                        id: prevCoverImgMask
                        width: prevCoverImg.width
                        height: prevCoverImg.height
                        radius: 16
                        color: "black"
                        antialiasing: true
                    }

                    Image {
                        id: prevCoverImg
                        anchors.fill: parent
                        source: parent.parent.parent.prevItem ? (parent.parent.parent.prevItem.coverUrl || "") : ""
                        visible: source.toString().length > 0 && status === Image.Ready
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: true

                        layer.enabled: true
                        layer.effect: MultiEffect {
                            maskEnabled: true
                            maskSource: ShaderEffectSource {
                                sourceItem: prevCoverImgMask
                                hideSource: true
                            }
                        }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        visible: !prevCoverImg.visible
                        name: "prev"; size: 14; color: window.textPrimary; filled: true
                    }
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        text: "上一首"
                        font.family: window.fontFamily
                        font.pixelSize: 10
                        color: window.textTertiary
                    }
                    Text {
                        Layout.fillWidth: true
                        text: parent.parent.parent.prevItem ? (parent.parent.parent.prevItem.title || "") : ""
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                        color: window.textPrimary
                        elide: Text.ElideRight
                    }
                }
            }

            MouseArea {
                id: prevArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: playerVM.previous()
            }
        }

        Item { Layout.fillWidth: true }

        // 下一首
        Item {
            Layout.preferredHeight: 44
            Layout.preferredWidth: 220
            visible: nextItem !== null
            property var nextItem: {
                var q = playerVM.queue || []
                var i = playerVM.currentIndex + 1
                return (i >= 0 && i < q.length) ? q[i] : null
            }

            Rectangle {
                anchors.fill: parent
                radius: 22
                color: nextArea.containsMouse ? window.cardHover : window.sidebarBg
                border.color: window.borderColor
                border.width: 1
                Behavior on color { ColorAnimation { duration: 150 } }
            }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 6
                spacing: 8

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Text {
                        Layout.alignment: Qt.AlignRight
                        text: "下一首"
                        font.family: window.fontFamily
                        font.pixelSize: 10
                        color: window.textTertiary
                    }
                    Text {
                        Layout.fillWidth: true
                        text: parent.parent.parent.nextItem ? (parent.parent.parent.nextItem.title || "") : ""
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        font.weight: Font.DemiBold
                        color: window.textPrimary
                        elide: Text.ElideRight
                        horizontalAlignment: Text.AlignRight
                    }
                }
                Rectangle {
                    Layout.preferredWidth: 32
                    Layout.preferredHeight: 32
                    radius: 16
                    color: window.borderColor
                    clip: true
                    Rectangle {
                        id: nextCoverImgMask
                        width: nextCoverImg.width
                        height: nextCoverImg.height
                        radius: 16
                        color: "black"
                        antialiasing: true
                    }

                    Image {
                        id: nextCoverImg
                        anchors.fill: parent
                        source: parent.parent.parent.nextItem ? (parent.parent.parent.nextItem.coverUrl || "") : ""
                        visible: source.toString().length > 0 && status === Image.Ready
                        fillMode: Image.PreserveAspectCrop
                        asynchronous: true
                        cache: true

                        layer.enabled: true
                        layer.effect: MultiEffect {
                            maskEnabled: true
                            maskSource: ShaderEffectSource {
                                sourceItem: nextCoverImgMask
                                hideSource: true
                            }
                        }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        visible: !nextCoverImg.visible
                        name: "next"; size: 14; color: window.textPrimary; filled: true
                    }
                }
            }

            MouseArea {
                id: nextArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: playerVM.next()
            }
        }
    }

    // 进度条
    ColumnLayout {
        id: progressCol
        visible: root.hasTrack
        anchors.bottom: controlsRow.top
        anchors.bottomMargin: 32
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 48
        anchors.rightMargin: 48
        spacing: 6

        Slider {
            id: progressSlider
            Layout.fillWidth: true
            from: 0
            to: playerVM.duration > 0 ? playerVM.duration : 1
            value: playerVM.position

            onMoved: playerVM.seek(value)

            property bool barHovered: progressHover.hovered || pressed

            // 极简单色细线: 与 MiniPlayer 风格统一, hover 时略微变粗
            background: Rectangle {
                x: progressSlider.leftPadding
                y: progressSlider.topPadding + progressSlider.availableHeight / 2 - height / 2
                width: progressSlider.availableWidth
                height: progressSlider.barHovered ? 3 : 2
                radius: height / 2
                color: "#1A000000"
                Behavior on height { NumberAnimation { duration: 150; easing.type: Easing.OutQuad } }

                Rectangle {
                    width: progressSlider.visualPosition * parent.width
                    height: parent.height
                    radius: parent.radius
                    color: window.textPrimary
                }
            }

            // handle: 仅 hover 时显现的小圆点
            handle: Rectangle {
                x: progressSlider.leftPadding + progressSlider.visualPosition * (progressSlider.availableWidth - width)
                y: progressSlider.topPadding + progressSlider.availableHeight / 2 - height / 2
                width: progressSlider.barHovered ? 12 : 0
                height: width
                radius: width / 2
                color: window.textPrimary
                Behavior on width { NumberAnimation { duration: 160; easing.type: Easing.OutQuart } }
            }

            HoverHandler { id: progressHover }
        }

        RowLayout {
            Layout.fillWidth: true
            Text {
                text: root.formatTime(playerVM.position)
                font.family: "Microsoft YaHei UI"
                font.pixelSize: 12
                color: window.textTertiary
            }
            Item { Layout.fillWidth: true }
            Text {
                text: root.formatTime(playerVM.duration)
                font.family: "Microsoft YaHei UI"
                font.pixelSize: 12
                color: window.textTertiary
            }
        }
    }

    // 控制按钮
    RowLayout {
        id: controlsRow
        visible: root.hasTrack
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 56
        anchors.horizontalCenter: parent.horizontalCenter
        spacing: 28

        IconCircleBtn {
            iconName: "shuffle"; size: 40; iconSize: 18
            iconColor: playerVM.shuffle ? window.textPrimary : window.textTertiary
            onClicked: playerVM.toggleShuffle()
        }

        IconCircleBtn {
            iconName: "prev"; size: 44; iconSize: 22
            iconColor: window.textPrimary
            iconFilled: true
            strokeWidthOverride: 0
            onClicked: playerVM.previous()
        }

        // 主播放按钮 (深色实心胶囊, 与 MiniPlayer 风格一致)
        Rectangle {
            Layout.preferredWidth: 64
            Layout.preferredHeight: 64
            radius: 32
            color: mainPlayArea.pressed ? "#000000"
                 : (mainPlayArea.containsMouse ? "#0F0F11" : window.textPrimary)
            Behavior on color { ColorAnimation { duration: 120 } }

            AppIcon {
                anchors.centerIn: parent
                anchors.horizontalCenterOffset: playerVM.state === 2 ? 0 : 2
                name: playerVM.state === 2 ? "pause" : "play"
                size: 26
                color: "#FFFFFF"
                filled: true
            }

            MouseArea {
                id: mainPlayArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (playerVM.state === 2) playerVM.pause()
                    else playerVM.play()
                }
            }
        }

        IconCircleBtn {
            iconName: "next"; size: 44; iconSize: 22
            iconColor: window.textPrimary
            iconFilled: true
            strokeWidthOverride: 0
            onClicked: playerVM.next()
        }

        IconCircleBtn {
            iconName: "repeat"; size: 40; iconSize: 18
            iconColor: playerVM.repeatMode > 0 ? window.textPrimary : window.textTertiary
            badgeText: playerVM.repeatMode === 2 ? "1" : ""
            onClicked: playerVM.cycleRepeatMode()
        }
    }

    // ===== 空状态: 没有任何曲目时显示提示, 与上面所有"播放态"元素互斥 =====
    Item {
        id: emptyState
        visible: !root.hasTrack
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 18

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                width: 112; height: 112; radius: 56
                color: window.sidebarBg
                border.color: window.borderColor
                border.width: 1

                AppIcon {
                    anchors.centerIn: parent
                    name: "music"
                    size: 48
                    color: window.textTertiary
                    strokeWidth: 1.6
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: 4
                text: "暂无播放歌曲"
                font.family: window.fontFamily
                font.pixelSize: 20
                font.weight: Font.DemiBold
                color: window.textSecondary
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "请从左侧「音乐库」选择曲目,或拖拽音频文件到窗口"
                font.family: window.fontFamily
                font.pixelSize: 13
                color: window.textTertiary
            }

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: 12
                width: gotoRow.implicitWidth + 36
                height: 38
                radius: 19
                color: gotoArea.pressed ? window.brandPress
                     : (gotoArea.containsMouse ? window.brandHover : window.brand)
                Behavior on color { ColorAnimation { duration: 150 } }

                RowLayout {
                    id: gotoRow
                    anchors.centerIn: parent
                    spacing: 8
                    AppIcon { name: "library"; size: 14; color: "#FFFFFF"; strokeWidth: 2 }
                    Text {
                        text: "打开音乐库"
                        color: "#FFFFFF"
                        font.family: window.fontFamily
                        font.pixelSize: 13
                        font.weight: Font.DemiBold
                    }
                }

                MouseArea {
                    id: gotoArea
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: window.navigateTo("library")
                }
            }
        }
    }
}
