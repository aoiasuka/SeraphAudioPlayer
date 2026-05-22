import QtQuick

// 16 段频谱柱 + 左右声道 VU bar
Item {
    id: root

    readonly property var bands: playerVM.spectrum || []
    readonly property double vuL: playerVM.vuLeft
    readonly property double vuR: playerVM.vuRight

    Canvas {
        id: canvas
        anchors.fill: parent
        antialiasing: true
        contextType: "2d"

        // 主色
        property color barColor: window.brand
        property color peakColor: "#FFFFFF"

        Connections {
            target: playerVM
            function onVisualUpdated() { canvas.requestPaint() }
        }
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        onBarColorChanged: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            if (!ctx) return
            ctx.reset()
            var w = width, h = height
            if (w <= 0 || h <= 0) return

            // === 频谱柱 (16 个) ===
            var bands = root.bands
            var n = bands.length
            if (n > 0) {
                var vuWidth = 60      // 左右各 30 px 给 VU
                var specW = w - vuWidth - 16
                var specX = vuWidth
                var barW = specW / n * 0.7
                var gap = specW / n - barW

                for (var i = 0; i < n; i++) {
                    var v = Math.max(0, Math.min(1, bands[i]))
                    var bh = v * (h - 6)
                    var x = specX + i * (barW + gap)
                    var y = h - bh - 3

                    var grad = ctx.createLinearGradient(0, y, 0, h)
                    grad.addColorStop(0, Qt.tint(canvas.barColor, "#33FFFFFF"))
                    grad.addColorStop(1, canvas.barColor)
                    ctx.fillStyle = grad
                    ctx.beginPath()
                    var r = 2
                    ctx.moveTo(x + r, y)
                    ctx.lineTo(x + barW - r, y)
                    ctx.quadraticCurveTo(x + barW, y, x + barW, y + r)
                    ctx.lineTo(x + barW, h - 3)
                    ctx.lineTo(x, h - 3)
                    ctx.lineTo(x, y + r)
                    ctx.quadraticCurveTo(x, y, x + r, y)
                    ctx.closePath()
                    ctx.fill()
                }
            }

            // === 左右声道 VU bar ===
            function drawVu(x, vu, peak) {
                var bw = 10
                var bh = h - 6
                // 槽
                ctx.fillStyle = "#33000000"
                ctx.fillRect(x, 3, bw, bh)
                // 填充
                var fh = vu * bh
                var grad2 = ctx.createLinearGradient(0, h - 3, 0, 3)
                grad2.addColorStop(0.0, "#10B981")
                grad2.addColorStop(0.7, "#F59E0B")
                grad2.addColorStop(1.0, "#EF4444")
                ctx.fillStyle = grad2
                ctx.fillRect(x, h - 3 - fh, bw, fh)
                // 峰值线
                if (peak > 0.01) {
                    var py = h - 3 - peak * bh
                    ctx.fillStyle = canvas.peakColor
                    ctx.fillRect(x, py, bw, 1.5)
                }
            }
            drawVu(8,  root.vuL, playerVM.peakLeft)
            drawVu(28, root.vuR, playerVM.peakRight)

            // 标签
            ctx.fillStyle = "#8E8E93"
            ctx.font = "10px 'Microsoft YaHei UI'"
            ctx.fillText("L", 9, h - 8)
            ctx.fillText("R", 29, h - 8)
        }
    }
}
