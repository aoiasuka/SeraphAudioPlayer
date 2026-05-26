import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Effects
import "../components"

// Synapse HiFi 正在播放视图 — 中央播放区 + 右侧 320px 信息栏
//
// 中央列:
//   - Cover (呼吸光晕 + 边框)
//   - Title / Artist / Album
//   - Hi-Res 金色徽章
//   - Output 状态行 (左 "轻量化"  右 "WASAPI - DAC")
//   - Canvas 波形进度条
//   - 控制行 (shuffle/prev/PLAY/next/loop/like + volume)
//
// 右侧列:
//   - 下一首播放 卡片
//   - Lyrics 歌词区
//   - Audio Info 音频信息卡 (Format / Bitrate / SampleRate / Channels / Size)
Item {
    id: root
    objectName: "nowPlayingView"

    readonly property bool hasTrack: playerVM.currentIndex >= 0

    function formatTime(seconds) {
        if (seconds < 0) return "00:00"
        var m = Math.floor(seconds / 60)
        var s = Math.floor(seconds % 60)
        return (m < 10 ? "0" + m : m) + ":" + (s < 10 ? "0" + s : s)
    }

    // 当前曲目摘要 (用于波形 trackKey)
    readonly property string trackKey: {
        if (!hasTrack || !playerVM.queue || playerVM.queue.length === 0) return ""
        var cur = playerVM.queue[playerVM.currentIndex]
        return cur ? (cur.path || cur.title || "") : ""
    }

    TrackContextMenu { id: ctxMenu }

    // ============ 顶部小工具栏 (轻量化 — 仅返回 + more) ============
    RowLayout {
        id: topBar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.margins: 16
        height: 32
        spacing: 8

        SidebarIconButton {
            iconName: "chevron"
            iconSize: 16
            iconColor: window.textSecondary
            implicitWidth: 32
            implicitHeight: 32
            onClicked: root.StackView.view.pop()
            rotation: 180
        }

        Item { Layout.fillWidth: true }

        SidebarIconButton {
            iconName: "more"
            iconSize: 16
            iconColor: window.textSecondary
            implicitWidth: 32
            implicitHeight: 32
            onClicked: {
                if (playerVM.title && playerVM.title !== "未播放" && playerVM.queue.length > 0) {
                    var cur = playerVM.queue[playerVM.currentIndex]
                    if (cur) ctxMenu.openFor(cur.path)
                }
            }
        }
    }

    // ============ 右侧 320 信息栏 ============
    Rectangle {
        id: rightPanel
        visible: root.hasTrack
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.right: parent.right
        anchors.topMargin: 8
        anchors.bottomMargin: 16
        anchors.rightMargin: 16
        width: 320
        radius: 14
        color: window.acrylicRightBg
        border.color: window.borderColor
        border.width: 1
        antialiasing: true

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 14
            spacing: 12

            // ----- 下一首播放 -----
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6

                Text {
                    text: "下一首播放"
                    font.family: window.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.Bold
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.8
                    color: window.textTertiary
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: 56
                    radius: 10
                    color: nextArea.containsMouse ? window.acrylicCardBgHi : window.acrylicCardBg
                    border.color: window.borderColor
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 150 } }

                    RowLayout {
                        anchors.fill: parent
                        anchors.margins: 8
                        spacing: 10

                        // 下一首封面
                        Item {
                            Layout.preferredWidth: 40
                            Layout.preferredHeight: 40

                            Rectangle {
                                anchors.fill: parent
                                radius: 6
                                color: window.surfaceAlt
                                border.color: window.borderColor
                                border.width: 1
                            }

                            Rectangle {
                                id: nextCoverMask
                                anchors.fill: parent
                                anchors.margins: 1
                                radius: 5
                                color: "black"
                                antialiasing: true
                            }

                            Image {
                                id: nextCoverImg
                                anchors.fill: parent
                                anchors.margins: 1
                                source: {
                                    if (!playerVM.queue || playerVM.queue.length === 0) return ""
                                    var nextIdx = (playerVM.currentIndex + 1) % playerVM.queue.length
                                    var nxt = playerVM.queue[nextIdx]
                                    return nxt ? (nxt.coverUrl || "") : ""
                                }
                                visible: source.toString().length > 0 && status === Image.Ready
                                fillMode: Image.PreserveAspectCrop
                                asynchronous: true
                                cache: true

                                layer.enabled: true
                                layer.effect: MultiEffect {
                                    maskEnabled: true
                                    maskSource: ShaderEffectSource {
                                        sourceItem: nextCoverMask
                                        hideSource: true
                                    }
                                }
                            }

                            AppIcon {
                                anchors.centerIn: parent
                                visible: !nextCoverImg.visible
                                name: "music"
                                size: 16
                                color: window.textTertiary
                                strokeWidth: 1.6
                            }
                        }

                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2

                            Text {
                                Layout.fillWidth: true
                                text: {
                                    if (!playerVM.queue || playerVM.queue.length === 0) return "—"
                                    var nextIdx = (playerVM.currentIndex + 1) % playerVM.queue.length
                                    var nxt = playerVM.queue[nextIdx]
                                    return nxt ? (nxt.title || "未知曲目") : "—"
                                }
                                font.family: window.fontFamily
                                font.pixelSize: 12
                                font.weight: Font.DemiBold
                                color: window.textPrimary
                                elide: Text.ElideRight
                            }
                            Text {
                                Layout.fillWidth: true
                                text: {
                                    if (!playerVM.queue || playerVM.queue.length === 0) return ""
                                    var nextIdx = (playerVM.currentIndex + 1) % playerVM.queue.length
                                    var nxt = playerVM.queue[nextIdx]
                                    return nxt ? (nxt.artist || "") : ""
                                }
                                font.family: window.fontFamily
                                font.pixelSize: 10
                                color: window.textTertiary
                                elide: Text.ElideRight
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

            // ----- 歌词面板 -----
            ColumnLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 6

                Text {
                    text: "LYRICS 歌词区域"
                    font.family: window.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.Bold
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.8
                    color: window.textTertiary
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.fillHeight: true
                    radius: 10
                    color: window.acrylicCardBg
                    border.color: window.borderColor
                    border.width: 1
                    clip: true

                    LyricsView {
                        anchors.fill: parent
                        anchors.margins: 6
                    }
                }
            }

            // ----- 音频信息 -----
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 6

                Text {
                    text: "AUDIO INFO 音频信息"
                    font.family: window.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.Bold
                    font.capitalization: Font.AllUppercase
                    font.letterSpacing: 0.8
                    color: window.textTertiary
                }

                Rectangle {
                    Layout.fillWidth: true
                    Layout.preferredHeight: audioInfoCol.implicitHeight + 16
                    radius: 10
                    color: window.acrylicCardBg
                    border.color: window.borderColor
                    border.width: 1

                    ColumnLayout {
                        id: audioInfoCol
                        anchors.fill: parent
                        anchors.margins: 10
                        spacing: 4

                        function parseFormat() {
                            var f = playerVM.formatInfo || ""
                            var parts = { format: "—", rate: "—", bits: "—" }
                            if (f.length === 0) return parts
                            var m = f.match(/^(\w+)/)
                            if (m) parts.format = m[1]
                            var r = f.match(/(\d+(\.\d+)?)\s*kHz/i)
                            if (r) parts.rate = r[1] + " kHz"
                            var b = f.match(/(\d+)\s*bit/i)
                            if (b) parts.bits = b[1] + " bit"
                            return parts
                        }

                        readonly property var info: parseFormat()

                        Repeater {
                            model: [
                                { k: "Format",      v: audioInfoCol.info.format },
                                { k: "Bit Depth",   v: audioInfoCol.info.bits },
                                { k: "Sample Rate", v: audioInfoCol.info.rate },
                                { k: "Device",      v: playerVM.currentDeviceName || "—" }
                            ]
                            delegate: RowLayout {
                                Layout.fillWidth: true
                                spacing: 8
                                Text {
                                    text: modelData.k + ":"
                                    font.family: window.fontFamily
                                    font.pixelSize: 10
                                    color: window.textTertiary
                                    Layout.preferredWidth: 80
                                }
                                Text {
                                    Layout.fillWidth: true
                                    text: modelData.v
                                    font.family: window.fontFamily
                                    font.pixelSize: 10
                                    font.weight: Font.Medium
                                    color: window.textPrimary
                                    elide: Text.ElideRight
                                    horizontalAlignment: Text.AlignRight
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // ============ 中央播放区 ============
    Item {
        id: centerCol
        visible: root.hasTrack
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: rightPanel.left
        anchors.topMargin: 8
        anchors.bottomMargin: 16
        anchors.leftMargin: 24
        anchors.rightMargin: 24

        ColumnLayout {
            anchors.fill: parent
            anchors.leftMargin: Math.max(0, (parent.width - 560) / 2)
            anchors.rightMargin: Math.max(0, (parent.width - 560) / 2)
            spacing: 12

            // 顶部弹性占位，将封面和标题组推向垂直中间偏上区域
            Item {
                Layout.fillHeight: true
                Layout.minimumHeight: 12
            }

            // ----- 封面 (呼吸 + 光晕) -----
            Item {
                id: coverWrap
                Layout.alignment: Qt.AlignHCenter
                Layout.preferredWidth: 240
                Layout.preferredHeight: 240

                // 后置光晕 (品牌色, 缓慢呼吸)
                Rectangle {
                    id: coverGlow
                    anchors.centerIn: parent
                    width: parent.width * 1.15
                    height: parent.height * 1.15
                    radius: width / 2
                    color: window.brandLite
                    opacity: 0.20
                    layer.enabled: true
                    layer.smooth: true
                    layer.effect: MultiEffect {
                        blurEnabled: true
                        blurMax: 64
                        blur: 1.0
                    }

                    SequentialAnimation on opacity {
                        loops: Animation.Infinite
                        running: playerVM.state === 2
                        NumberAnimation { from: 0.12; to: 0.28; duration: 6000; easing.type: Easing.InOutSine }
                        NumberAnimation { from: 0.28; to: 0.12; duration: 6000; easing.type: Easing.InOutSine }
                    }
                }

                // 封面卡片
                Rectangle {
                    id: coverCard
                    anchors.fill: parent
                    radius: 16
                    color: window.surface
                    border.color: "#80FFFFFF"
                    border.width: 1
                    clip: true
                    antialiasing: true

                    // 呼吸缩放
                    SequentialAnimation on scale {
                        loops: Animation.Infinite
                        running: playerVM.state === 2
                        NumberAnimation { from: 1.0;   to: 1.012; duration: 6000; easing.type: Easing.InOutSine }
                        NumberAnimation { from: 1.012; to: 1.0;   duration: 6000; easing.type: Easing.InOutSine }
                    }

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

                    // 无封面占位
                    Rectangle {
                        visible: !coverImg.visible
                        anchors.fill: parent
                        gradient: Gradient {
                            orientation: Gradient.Diagonal
                            GradientStop { position: 0.0; color: "#E2E8F0" }
                            GradientStop { position: 1.0; color: "#CBD5E1" }
                        }
                        AppIcon {
                            anchors.centerIn: parent
                            name: "music"
                            size: 56
                            color: window.brand
                            strokeWidth: 1.6
                        }
                    }
                }
            }

            // ----- 标题 / 艺术家 / 专辑 -----
            ColumnLayout {
                Layout.alignment: Qt.AlignHCenter
                Layout.fillWidth: true
                spacing: 4

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.fillWidth: true
                    text: playerVM.title || "未播放"
                    font.family: window.fontFamily
                    font.pixelSize: 22
                    font.weight: Font.Bold
                    color: window.textPrimary
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignHCenter
                }

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.fillWidth: true
                    text: {
                        var cur = playerVM.queue && playerVM.queue.length > 0
                            ? playerVM.queue[playerVM.currentIndex] : null
                        return "Artist · " + (cur && cur.artist ? cur.artist : "—")
                    }
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    font.weight: Font.DemiBold
                    color: window.textSecondary
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignHCenter
                }

                Text {
                    Layout.alignment: Qt.AlignHCenter
                    Layout.fillWidth: true
                    text: {
                        var cur = playerVM.queue && playerVM.queue.length > 0
                            ? playerVM.queue[playerVM.currentIndex] : null
                        return "💿 Album · " + (cur && cur.album ? cur.album : "—")
                    }
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    color: window.textTertiary
                    elide: Text.ElideRight
                    horizontalAlignment: Text.AlignHCenter
                }
            }

            // ----- Hi-Res 徽章 -----
            HiResBadge {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: 2
                formatText: playerVM.formatInfo
            }

            // 中部弹性占位，在 EQ 展开时能够弹性压缩，防止封面和标题被过度向上挤压
            Item {
                Layout.fillHeight: true
                Layout.minimumHeight: 8
            }

            // ----- Output 标签行 (左右内边距对齐波形进度条背景) -----
            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 36
                Layout.rightMargin: 36
                Layout.topMargin: 2
                spacing: 0

                Text {
                    Layout.fillWidth: true
                    text: "轻量化 · 独占模式"
                    font.family: window.fontFamily
                    font.pixelSize: 10
                    color: window.textTertiary
                }

                Item {
                    Layout.alignment: Qt.AlignVCenter
                    implicitWidth: deviceRow.implicitWidth + 16
                    implicitHeight: 24

                    Rectangle {
                        anchors.fill: parent
                        radius: 4
                        color: deviceMouseArea.containsMouse || deviceMenu.visible ? window.hoverBg : "transparent"
                        Behavior on color { ColorAnimation { duration: 150 } }
                    }

                    RowLayout {
                        id: deviceRow
                        anchors.centerIn: parent
                        spacing: 4
                        AppIcon {
                            name: "volume"
                            size: 11
                            color: window.textSecondary
                            strokeWidth: 1.6
                        }
                        Text {
                            text: (playerVM.currentDeviceName || "默认设备")
                            font.family: window.fontFamily
                            font.pixelSize: 10
                            font.weight: Font.DemiBold
                            color: window.textSecondary
                            elide: Text.ElideRight
                            Layout.maximumWidth: 200
                        }
                        AppIcon {
                            name: "chevron"
                            size: 10
                            rotation: -90
                            color: window.textTertiary
                        }
                    }

                    MouseArea {
                        id: deviceMouseArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: deviceMenu.popup(0, parent.height + 4)
                    }

                    Menu {
                        id: deviceMenu

                        padding: 6
                        topPadding: 8
                        bottomPadding: 8
                        leftPadding: 6
                        rightPadding: 6

                        background: Rectangle {
                            implicitWidth: 280
                            radius: window.mediumRadius
                            color: window.surfaceMenu
                            border.color: window.borderColor
                            border.width: 1
                            antialiasing: true
                        }

                        delegate: MenuItem {
                            id: mItemDelegate
                            implicitHeight: 32
                            leftPadding: 12
                            rightPadding: 12

                            // 移除默认打勾图标，让内容居左
                            indicator: Item {}

                            contentItem: Text {
                                text: mItemDelegate.text
                                font.family: window.fontFamily
                                font.pixelSize: 11
                                font.weight: mItemDelegate.checked ? Font.Bold : Font.Medium
                                color: mItemDelegate.checked ? window.brand : window.textPrimary
                                verticalAlignment: Text.AlignVCenter
                                elide: Text.ElideRight
                            }
                            background: Rectangle {
                                implicitHeight: 32
                                radius: window.smallRadius
                                color: mItemDelegate.highlighted ? window.menuHoverBg : "transparent"
                                Behavior on color { ColorAnimation { duration: 100 } }
                            }
                        }

                        onAboutToShow: {
                            while (count > 0) {
                                removeItem(itemAt(0))
                            }
                            var devs = playerVM.devices
                            for (var i = 0; i < devs.length; ++i) {
                                var dev = devs[i]
                                var title = dev.name + (dev.isDefault ? " (默认)" : "")
                                var mItem = addItem(title)
                                
                                // 高亮当前所选设备
                                if (dev.id === playerVM.currentDeviceId) {
                                    mItem.checkable = true
                                    mItem.checked = true
                                }

                                mItem.triggered.connect((function(id) {
                                    return function() { playerVM.setDevice(id) }
                                })(dev.id))
                            }
                        }
                    }
                }
            }

            // ----- 波形进度条 -----
            WaveformProgressBar {
                Layout.fillWidth: true
                Layout.preferredHeight: 40
                position: playerVM.position
                duration: playerVM.duration > 0 ? playerVM.duration : 1
                playing: playerVM.state === 2
                trackKey: root.trackKey
                onSeekRequested: function(t) { playerVM.seek(t) }
            }

            // ----- 控制行 -----
            Item {
                Layout.fillWidth: true
                Layout.topMargin: 4
                Layout.preferredHeight: 56

                Rectangle {
                    anchors.top: parent.top
                    anchors.left: parent.left
                    anchors.right: parent.right
                    height: 1
                    color: window.borderColor
                }

                RowLayout {
                    anchors.fill: parent
                    anchors.topMargin: 10
                    spacing: 0

                    // 左侧控制 (EQ开关等)
                    RowLayout {
                        Layout.preferredWidth: 140
                        spacing: 12
                        
                        IconCircleBtn {
                            iconName: "sliders"
                            size: 30
                            iconSize: 14
                            iconColor: eqPopup.opened ? window.brand : window.textTertiary
                            onClicked: eqPopup.opened ? eqPopup.close() : eqPopup.open()
                        }
                    }

                    // 中部主控按钮
                    RowLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignHCenter
                        spacing: 18

                        Item { Layout.fillWidth: true }

                        IconCircleBtn {
                            iconName: "shuffle"; size: 30; iconSize: 14
                            iconColor: playerVM.shuffle ? window.brand : window.textTertiary
                            onClicked: playerVM.toggleShuffle()
                        }

                        IconCircleBtn {
                            iconName: "prev"; size: 32; iconSize: 16
                            iconColor: window.textSecondary
                            iconFilled: true
                            strokeWidthOverride: 0
                            onClicked: playerVM.previous()
                        }

                        // 主播放按钮 (Cyan-600 圆形 + 阴影)
                        Item {
                            Layout.preferredWidth: 44
                            Layout.preferredHeight: 44

                            // 软阴影
                            Rectangle {
                                anchors.fill: parent
                                anchors.margins: -3
                                anchors.verticalCenterOffset: 4
                                radius: width / 2
                                color: window.brand
                                opacity: mainPlayArea.containsMouse ? 0.42 : 0.28
                                z: -1
                                Behavior on opacity { NumberAnimation { duration: 180 } }
                                layer.enabled: true
                                layer.smooth: true
                                layer.effect: MultiEffect {
                                    blurEnabled: true
                                    blurMax: 24
                                    blur: 1.0
                                }
                            }

                            Rectangle {
                                anchors.fill: parent
                                radius: width / 2
                                color: mainPlayArea.pressed ? window.brandPress
                                     : (mainPlayArea.containsMouse ? window.brandHover : window.brand)
                                Behavior on color { ColorAnimation { duration: 120 } }
                                scale: mainPlayArea.containsMouse ? 1.05 : 1.0
                                Behavior on scale { NumberAnimation { duration: 160; easing.type: Easing.OutQuad } }

                                AppIcon {
                                    anchors.centerIn: parent
                                    anchors.horizontalCenterOffset: playerVM.state === 2 ? 0 : 1.5
                                    name: playerVM.state === 2 ? "pause" : "play"
                                    size: 18
                                    color: "#FFFFFF"
                                    filled: true
                                }
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
                            iconName: "next"; size: 32; iconSize: 16
                            iconColor: window.textSecondary
                            iconFilled: true
                            strokeWidthOverride: 0
                            onClicked: playerVM.next()
                        }

                        IconCircleBtn {
                            iconName: "repeat"; size: 30; iconSize: 14
                            iconColor: playerVM.repeatMode > 0 ? window.brand : window.textTertiary
                            badgeText: playerVM.repeatMode === 2 ? "1" : ""
                            onClicked: playerVM.cycleRepeatMode()
                        }

                        // Like
                        Item {
                            Layout.preferredWidth: 30
                            Layout.preferredHeight: 30

                            Rectangle {
                                anchors.fill: parent
                                radius: width / 2
                                color: likeArea.containsMouse ? window.hoverBg : "transparent"
                                Behavior on color { ColorAnimation { duration: 120 } }
                            }

                            AppIcon {
                                anchors.centerIn: parent
                                name: "heart"
                                size: 14
                                color: playerVM.currentLiked ? window.likeRed : window.textTertiary
                                filled: playerVM.currentLiked
                                strokeWidth: 1.6
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

                    // 右侧音量
                    RowLayout {
                        Layout.preferredWidth: 140
                        spacing: 6

                        Item { Layout.fillWidth: true }

                        AppIcon {
                            name: playerVM.muted || playerVM.volume === 0 ? "volume-mute" : "volume"
                            size: 13
                            color: window.textSecondary
                            strokeWidth: 1.6

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: playerVM.toggleMute()
                            }
                        }

                        Slider {
                            id: volSlider
                            Layout.preferredWidth: 72
                            from: 0; to: 100
                            value: playerVM.volume
                            onMoved: {
                                playerVM.volume = Math.round(value)
                                if (playerVM.muted && value > 0) playerVM.muted = false
                            }
                            background: Rectangle {
                                x: volSlider.leftPadding
                                y: volSlider.topPadding + volSlider.availableHeight / 2 - 1
                                width: volSlider.availableWidth
                                height: 2
                                radius: 1
                                color: window.hairline
                                Rectangle {
                                    width: volSlider.visualPosition * parent.width
                                    height: parent.height
                                    radius: parent.radius
                                    color: window.brand
                                }
                            }
                            handle: Rectangle {
                                x: volSlider.leftPadding + volSlider.visualPosition * (volSlider.availableWidth - width)
                                y: volSlider.topPadding + (volSlider.availableHeight - height) / 2
                                width: 8; height: 8; radius: 4
                                color: window.brand
                            }
                        }
                    }
                }
            }

        }
    }

    // ============ 空状态 ============
    Item {
        id: emptyState
        visible: !root.hasTrack
        anchors.top: topBar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 16

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                width: 96; height: 96; radius: 48
                color: window.acrylicCardBg
                border.color: window.borderColor
                border.width: 1

                AppIcon {
                    anchors.centerIn: parent
                    name: "music"
                    size: 40
                    color: window.brand
                    strokeWidth: 1.6
                }
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "暂无播放歌曲"
                font.family: window.fontFamily
                font.pixelSize: 18
                font.weight: Font.DemiBold
                color: window.textPrimary
            }

            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "请从左侧「音乐库」选择曲目, 或拖拽音频文件到窗口"
                font.family: window.fontFamily
                font.pixelSize: 12
                color: window.textTertiary
            }

            Rectangle {
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: 8
                width: gotoRow.implicitWidth + 32
                height: 34
                radius: 17
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
                        font.pixelSize: 12
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

    // ============ EQ 美化版弹窗 ============
    Popup {
        id: eqPopup
        parent: Overlay.overlay
        anchors.centerIn: parent
        width: 760
        height: 460
        modal: true
        focus: true
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        enter: Transition {
            NumberAnimation { property: "opacity"; from: 0.0; to: 1.0; duration: 250; easing.type: Easing.OutQuart }
            NumberAnimation { property: "scale"; from: 0.95; to: 1.0; duration: 250; easing.type: Easing.OutBack }
        }
        exit: Transition {
            NumberAnimation { property: "opacity"; from: 1.0; to: 0.0; duration: 200; easing.type: Easing.InQuart }
            NumberAnimation { property: "scale"; from: 1.0; to: 0.95; duration: 200; easing.type: Easing.InQuart }
        }

        background: Rectangle {
            radius: 16
            color: window.surface
            border.color: window.borderColor
            border.width: 1
        }

        contentItem: ColumnLayout {
            anchors.fill: parent
            anchors.margins: 28
            spacing: 24

            // 头部: 标题与开关 / 预设 / 重置
            RowLayout {
                Layout.fillWidth: true
                spacing: 16

                Text {
                    text: "均衡器"
                    font.family: window.fontFamily
                    font.pixelSize: 20
                    font.weight: Font.DemiBold
                    color: window.textPrimary
                }

                Item { Layout.preferredWidth: 8 }

                Switch {
                    id: eqSwitch
                    checked: playerVM.eqEnabled
                    onCheckedChanged: {
                        if (playerVM.eqEnabled !== checked) {
                            playerVM.eqEnabled = checked
                        }
                    }
                }

                Text {
                    text: "启用"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    color: window.textSecondary
                }

                Item { Layout.fillWidth: true }

                ComboBox {
                    id: presetCombo
                    Layout.preferredWidth: 120
                    Layout.preferredHeight: 34
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    model: ["自定义", "扁平", "重低音", "舞曲", "古典", "摇滚", "人声"]
                    onActivated: function(index) {
                        if (index === 0) return;
                        var p = [
                            [],
                            [0,0,0,0,0,0,0,0,0,0], // 扁平
                            [6,5,3,1,0,0,0,0,0,0], // 重低音
                            [4,3,1,0,-1,0,2,4,4,3], // 舞曲
                            [3,2,0,0,0,0,-1,-1,0,2], // 古典
                            [4,3,2,0,-1,-1,0,2,3,3], // 摇滚
                            [-2,-1,0,2,3,3,2,1,0,-1] // 人声
                        ]
                        var target = p[index]
                        for (var i = 0; i < 10; ++i) {
                            playerVM.setEqGain(i, target[i])
                        }
                    }
                    background: Rectangle {
                        radius: 8
                        color: window.surfaceMenu
                        border.color: window.borderColor
                        border.width: 1
                    }
                }

                Button {
                    text: "重置"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    Layout.preferredHeight: 34
                    Layout.preferredWidth: 64
                    background: Rectangle {
                        radius: 17
                        color: "transparent"
                        border.color: "#FF5A5A"
                        border.width: 1
                        opacity: parent.pressed ? 0.6 : (parent.hovered ? 0.8 : 1.0)
                        Behavior on opacity { NumberAnimation { duration: 100 } }
                    }
                    contentItem: Text {
                        text: parent.text
                        color: "#FF5A5A"
                        horizontalAlignment: Text.AlignHCenter
                        verticalAlignment: Text.AlignVCenter
                        font: parent.font
                    }
                    onClicked: {
                        playerVM.resetEq()
                        presetCombo.currentIndex = 0
                    }
                }

                Item { Layout.preferredWidth: 8 }

                IconCircleBtn {
                    iconName: "x"
                    size: 32
                    iconSize: 14
                    iconColor: window.textSecondary
                    onClicked: eqPopup.close()
                }
            }

            // 分割线
            Rectangle {
                Layout.fillWidth: true
                Layout.preferredHeight: 1
                color: window.divider
            }

            // EQ 调节区 (10 Band)
            RowLayout {
                Layout.fillWidth: true
                Layout.fillHeight: true
                spacing: 8

                property var freqs: ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"]

                Repeater {
                    model: 10
                    delegate: ColumnLayout {
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        spacing: 12

                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            Layout.preferredWidth: 48
                            horizontalAlignment: Text.AlignHCenter
                            text: (playerVM.eqGains[index] > 0 ? "+" : "") + playerVM.eqGains[index].toFixed(1) + " dB"
                            font.family: window.fontFamily
                            font.pixelSize: 11
                            color: window.textSecondary
                        }

                        Slider {
                            id: eqSlider
                            Layout.fillHeight: true
                            Layout.alignment: Qt.AlignHCenter
                            orientation: Qt.Vertical
                            from: -12.0
                            to: 12.0
                            value: playerVM.eqGains[index]
                            
                            onMoved: {
                                playerVM.setEqGain(index, value)
                                presetCombo.currentIndex = 0 // 设置为自定义
                            }

                            background: Rectangle {
                                x: eqSlider.leftPadding + (eqSlider.availableWidth - width) / 2
                                y: eqSlider.topPadding
                                implicitWidth: 4
                                implicitHeight: 220
                                width: 4
                                height: eqSlider.availableHeight
                                radius: 2
                                color: window.divider
                                
                                Rectangle {
                                    width: 4
                                    radius: 2
                                    color: playerVM.eqEnabled ? window.brand : window.textTertiary
                                    opacity: 0.6
                                    y: Math.min(eqSlider.visualPosition, 0.5) * eqSlider.availableHeight
                                    height: Math.abs(eqSlider.visualPosition - 0.5) * eqSlider.availableHeight
                                }
                            }
                            handle: Rectangle {
                                x: eqSlider.leftPadding + (eqSlider.availableWidth - width) / 2
                                y: eqSlider.topPadding + eqSlider.visualPosition * (eqSlider.availableHeight - height)
                                width: 14
                                height: 14
                                radius: 7
                                color: playerVM.eqEnabled ? window.brand : window.textTertiary
                                border.color: window.surface
                                border.width: 2
                            }
                        }

                        Text {
                            Layout.alignment: Qt.AlignHCenter
                            text: parent.parent.freqs[index]
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            font.weight: Font.DemiBold
                            color: window.textPrimary
                        }
                    }
                }
            }
        }
    }
}
