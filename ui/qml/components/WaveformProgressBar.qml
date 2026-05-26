import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Synapse 风格波形进度条 — Canvas 自绘 70 根对称包络条带
//
// 视觉:
//   - 已播放部分: Cyan-600 渐变 alpha (越靠右越深, 模拟"已播放饱和度递增")
//   - 未播放部分: 12% Slate-900
//   - 暂停时整条做缓慢"呼吸"扰动 (12s 周期)
//
// 交互:
//   - 点击任意位置 seek 到对应进度
//   - hover 时整条提亮一档 + cursor pointer
Item {
    id: root
    implicitHeight: 40

    // 由外部填充
    property real position: 0
    property real duration: 0
    property bool playing: false
    // 当前曲目 ID, 用于切歌时生成不同的波形种子
    property string trackKey: ""

    signal seekRequested(real seconds)

    // 音频响应数据
    readonly property var spectrumBands: playerVM.spectrum || []
    readonly property double vuLeft: playerVM.vuLeft || 0
    readonly property double vuRight: playerVM.vuRight || 0
    readonly property int effectType: playerVM.visualizerType

    Connections {
        target: playerVM
        function onVisualUpdated() {
            if (root.playing || root.effectType !== -1) {
                wave.requestPaint()
            }
        }
    }

    // 时间格式化
    function _fmt(sec) {
        if (!sec || sec < 0) return "00:00"
        var m = Math.floor(sec / 60)
        var s = Math.floor(sec % 60)
        return (m < 10 ? "0" + m : m) + ":" + (s < 10 ? "0" + s : s)
    }

    readonly property real _progressRatio: duration > 0 ? Math.max(0, Math.min(1, position / duration)) : 0

    // 当前时间
    Text {
        id: timeCurrent
        anchors.left: parent.left
        anchors.leftMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        text: root._fmt(root.position)
        font.family: window.fontFamily
        font.pixelSize: 10
        font.weight: Font.Medium
        color: window.textSecondary
        z: 2
    }

    // 总时长
    Text {
        id: timeTotal
        anchors.right: parent.right
        anchors.rightMargin: 4
        anchors.verticalCenter: parent.verticalCenter
        text: root._fmt(root.duration)
        font.family: window.fontFamily
        font.pixelSize: 10
        font.weight: Font.Medium
        color: window.textSecondary
        z: 2
    }

    // 主交互区
    Rectangle {
        id: bg
        anchors.fill: parent
        anchors.leftMargin: 36
        anchors.rightMargin: 36
        radius: 8
        color: scrubArea.containsMouse ? "#08000000" : "#04000000"
        Behavior on color { ColorAnimation { duration: 150 } }
    }

    // Canvas 波形
    Canvas {
        id: wave
        anchors.fill: bg
        anchors.leftMargin: 6
        anchors.rightMargin: 6
        antialiasing: true
        renderTarget: Canvas.FramebufferObject
        renderStrategy: Canvas.Cooperative

        readonly property int barCount: 70
        property real anim: 0    // 呼吸相位
        property var envelope: []
        property string lastTrackKey: ""

        function buildBaseline() {
            if (root.trackKey === lastTrackKey && envelope && envelope.length > 0) {
                return
            }
            lastTrackKey = root.trackKey || ""
            var arr = []
            var seed = 1.0
            if (root.trackKey && root.trackKey.length > 0) {
                var s = 0
                for (var k = 0; k < root.trackKey.length; ++k) s += root.trackKey.charCodeAt(k)
                seed = 0.6 + (s % 100) / 60.0
            }
            for (var i = 0; i < barCount; ++i) {
                var env = Math.sin((i / barCount) * Math.PI)
                var p1 = Math.sin(i * 0.15 * seed) * 0.28
                var p2 = Math.cos(i * 0.35 + seed) * 0.14
                var hf = Math.sin(i * 0.85) * 0.05
                var amp = (0.35 + p1 + p2 + hf) * env
                amp = Math.max(0.12, amp)
                arr.push(amp)
            }
            envelope = arr
        }

        Component.onCompleted: buildBaseline()
        Connections {
            target: root
            function onTrackKeyChanged() { wave.buildBaseline(); wave.requestPaint() }
        }

        onPaint: {
            var c = getContext('2d')
            var w = width
            var h = height
            c.clearRect(0, 0, w, h)
            if (!envelope || envelope.length === 0) return

            var ratio = root._progressRatio
            var barW = w / barCount
            
            // Create a vibrant linear gradient for the played portion
            var gradient = c.createLinearGradient(0, 0, w, 0)
            gradient.addColorStop(0.0, "#06b6d4") // Cyan-500
            gradient.addColorStop(1.0, "#3b82f6") // Blue-500

            // Find the index of the exact playhead position
            var playheadIndex = -1
            if (root.duration > 0 && ratio > 0) {
                playheadIndex = Math.floor(ratio * barCount)
                playheadIndex = Math.max(0, Math.min(barCount - 1, playheadIndex))
            }

            // Audio data
            var bands = root.spectrumBands
            var numBands = bands ? bands.length : 0
            var globalBass = (root.vuLeft + root.vuRight) / 2.0

            for (var i = 0; i < barCount; ++i) {
                var breath = root.playing
                    ? Math.sin(anim * 6.28 - i * 0.18) * 0.03
                    : Math.sin(anim * 6.28 + i * 0.12) * 0.05
                
                var baseAmp = envelope[i]
                var extraAmp = 0
                
                // Apply visualizer effect
                if (root.playing) {
                    if (root.effectType === 0) {
                        // Effect 0: Spectrum Mapping
                        if (numBands > 0) {
                            var bandIndex = Math.floor((i / barCount) * numBands)
                            var bandVal = Math.max(0, Math.min(1, bands[bandIndex]))
                            // 使用加法，保证无论原生柱子高低，跳动幅度都绝对明显
                            extraAmp = bandVal * 0.6
                        }
                    } else if (root.effectType === 1) {
                        // Effect 1: Global Bass Pulse
                        extraAmp = globalBass * 0.5
                    } else if (root.effectType === 2) {
                        // Effect 2: Playhead Ripple
                        if (playheadIndex >= 0) {
                            var dist = Math.abs(i - playheadIndex)
                            if (dist < 8) {
                                extraAmp = globalBass * (1.0 - dist / 8.0) * 0.75
                            }
                        }
                    }
                }

                // 改用加法，极大增强视觉波动感
                var amp = baseAmp + extraAmp + breath
                if (amp < 0.08) amp = 0.08
                
                var isPlayhead = (i === playheadIndex)
                if (isPlayhead) {
                    amp = Math.min(1.0, amp + 0.15) // Playhead 始终更高一点
                }

                // Max cap
                if (amp > 1.0) amp = 1.0

                var barH = h * 0.22 + amp * h * 0.7
                var x = i * barW
                var y = (h - barH) / 2
                var rWidth = Math.max(1.5, barW - 3)

                var barRatio = i / barCount

                // Draw rounded bar
                var rr = Math.min(1.6, rWidth / 2)
                c.beginPath()
                c.moveTo(x + 1.5 + rr, y)
                c.lineTo(x + 1.5 + rWidth - rr, y)
                c.quadraticCurveTo(x + 1.5 + rWidth, y, x + 1.5 + rWidth, y + rr)
                c.lineTo(x + 1.5 + rWidth, y + barH - rr)
                c.quadraticCurveTo(x + 1.5 + rWidth, y + barH, x + 1.5 + rWidth - rr, y + barH)
                c.lineTo(x + 1.5 + rr, y + barH)
                c.quadraticCurveTo(x + 1.5, y + barH, x + 1.5, y + barH - rr)
                c.lineTo(x + 1.5, y + rr)
                c.quadraticCurveTo(x + 1.5, y, x + 1.5 + rr, y)
                c.closePath()

                if (barRatio <= ratio) {
                    // Played portion
                    if (isPlayhead) {
                        c.fillStyle = "#60a5fa" // Bright blue for playhead
                        c.shadowColor = "#60a5fa"
                        c.shadowBlur = 6
                    } else {
                        c.fillStyle = gradient
                        c.shadowColor = "transparent"
                        c.shadowBlur = 0
                    }
                } else {
                    // Unplayed portion: a modern frosted look
                    c.fillStyle = "rgba(148, 163, 184, 0.25)"
                    c.shadowColor = "transparent"
                    c.shadowBlur = 0
                }
                c.fill()
            }
        }
    }

    // 驱动重绘: 播放时交给 playerVM 的 onVisualUpdated，暂停时仍保留缓慢呼吸
    Timer {
        interval: 250
        running: !root.playing
        repeat: true
        onTriggered: {
            wave.anim = (wave.anim + 0.004) % 1.0
            wave.requestPaint()
        }
    }
    
    // 动画相位持续步进
    Timer {
        interval: 50
        running: root.playing
        repeat: true
        onTriggered: {
            wave.anim = (wave.anim + 0.012) % 1.0
            // 重绘交由 onVisualUpdated 触发，若无音频信号也强制重绘以保证动画
            if (root.vuLeft < 0.01 && root.vuRight < 0.01) {
                wave.requestPaint()
            }
        }
    }

    onPositionChanged: wave.requestPaint()
    onDurationChanged: wave.requestPaint()
    onEffectTypeChanged: wave.requestPaint()

    MouseArea {
        id: scrubArea
        anchors.fill: bg
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: function(mouse) {
            if (root.duration <= 0) return
            var pct = Math.max(0, Math.min(1, (mouse.x - 6) / (width - 12)))
            root.seekRequested(pct * root.duration)
        }
    }
}
