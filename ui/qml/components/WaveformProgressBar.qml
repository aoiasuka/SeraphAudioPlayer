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

    // 主交互区: hover 时背景轻微提亮 (类似 acrylic 卡片)
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

        // 用 trackKey 生成确定性"高保真"包络
        function buildBaseline() {
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
            for (var i = 0; i < barCount; ++i) {
                var breath = root.playing
                    ? Math.sin(anim * 6.28 - i * 0.18) * 0.03
                    : Math.sin(anim * 6.28 + i * 0.12) * 0.05
                var amp = envelope[i] + breath
                if (amp < 0.08) amp = 0.08
                var barH = h * 0.22 + amp * h * 0.7
                var x = i * barW
                var y = (h - barH) / 2
                var rWidth = Math.max(1.5, barW - 3)

                var barRatio = i / barCount
                if (barRatio <= ratio) {
                    // 已播放: Cyan-700 alpha 渐进 0.55→0.95
                    var a = 0.55 + barRatio * 0.4
                    c.fillStyle = "rgba(14, 116, 144, " + a.toFixed(2) + ")"
                } else {
                    // 未播放: 8% Slate-900
                    c.fillStyle = "rgba(15, 23, 42, 0.10)"
                }

                // 圆角条
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
                c.fill()
            }
        }
    }

    // 驱动重绘: 播放时每 50ms 触发, 暂停时每 250ms
    Timer {
        interval: root.playing ? 50 : 250
        running: true
        repeat: true
        onTriggered: {
            wave.anim = (wave.anim + (root.playing ? 0.012 : 0.004)) % 1.0
            wave.requestPaint()
        }
    }

    // position 变化也要重绘 (拖动 / 自动推进)
    onPositionChanged: wave.requestPaint()
    onDurationChanged: wave.requestPaint()

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
