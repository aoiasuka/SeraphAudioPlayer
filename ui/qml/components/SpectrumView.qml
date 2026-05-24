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

        property int visualizerType: playerVM.visualizerType
        onVisualizerTypeChanged: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            if (!ctx) return
            ctx.reset()
            var w = width, h = height
            if (w <= 0 || h <= 0) return

            var type = canvas.visualizerType

            // === 频谱柱 (16 个) ===
            var bands = root.bands
            var n = bands.length
            if (n > 0) {
                var vuWidth = 60      // 左右各 30 px 给 VU
                var specW = w - vuWidth - 16
                var specX = vuWidth
                var barW = specW / n * 0.7
                var gap = specW / n - barW

                if (type === 0) {
                    // 0: 经典柱状
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
                } else if (type === 1) {
                    // 1: 镜像柱状
                    var midY = h / 2
                    for (var i = 0; i < n; i++) {
                        var v = Math.max(0, Math.min(1, bands[i]))
                        var bh = v * (h / 2 - 6)
                        var x = specX + i * (barW + gap)
                        
                        var grad = ctx.createLinearGradient(0, midY - bh, 0, midY + bh)
                        grad.addColorStop(0, Qt.tint(canvas.barColor, "#66FFFFFF"))
                        grad.addColorStop(0.5, canvas.barColor)
                        grad.addColorStop(1, Qt.tint(canvas.barColor, "#66FFFFFF"))
                        ctx.fillStyle = grad
                        
                        var r = 2
                        // 上半部分
                        ctx.beginPath()
                        ctx.moveTo(x + r, midY - bh)
                        ctx.lineTo(x + barW - r, midY - bh)
                        ctx.quadraticCurveTo(x + barW, midY - bh, x + barW, midY - bh + r)
                        ctx.lineTo(x + barW, midY)
                        ctx.lineTo(x, midY)
                        ctx.lineTo(x, midY - bh + r)
                        ctx.quadraticCurveTo(x, midY - bh, x + r, midY - bh)
                        ctx.fill()
                        // 下半部分
                        ctx.beginPath()
                        ctx.moveTo(x, midY)
                        ctx.lineTo(x + barW, midY)
                        ctx.lineTo(x + barW, midY + bh - r)
                        ctx.quadraticCurveTo(x + barW, midY + bh, x + barW - r, midY + bh)
                        ctx.lineTo(x + r, midY + bh)
                        ctx.quadraticCurveTo(x, midY + bh, x, midY + bh - r)
                        ctx.fill()
                    }
                } else if (type === 2 || type === 3) {
                    // 2: 平滑曲线  3: 对称波浪
                    var midY = type === 2 ? (h - 3) : (h / 2)
                    var maxHeight = type === 2 ? (h - 6) : (h / 2 - 6)
                    
                    var pts = []
                    pts.push({ x: specX, y: midY })
                    for (var i = 0; i < n; i++) {
                        var v = Math.max(0, Math.min(1, bands[i]))
                        var bh = v * maxHeight
                        var x = specX + i * (barW + gap) + barW / 2
                        pts.push({ x: x, y: midY - bh })
                    }
                    pts.push({ x: specX + specW, y: midY })

                    ctx.beginPath()
                    ctx.moveTo(pts[0].x, pts[0].y)
                    for (var i = 0; i < pts.length - 1; i++) {
                        var p0 = pts[i]
                        var p1 = pts[i + 1]
                        var cx = (p0.x + p1.x) / 2
                        ctx.bezierCurveTo(cx, p0.y, cx, p1.y, p1.x, p1.y)
                    }
                    
                    if (type === 2) {
                        ctx.lineTo(pts[pts.length - 1].x, h - 3)
                        ctx.lineTo(pts[0].x, h - 3)
                        ctx.closePath()
                        var grad = ctx.createLinearGradient(0, 0, 0, h)
                        grad.addColorStop(0, Qt.tint(canvas.barColor, "#80FFFFFF"))
                        grad.addColorStop(1, Qt.rgba(canvas.barColor.r, canvas.barColor.g, canvas.barColor.b, 0.1))
                        ctx.fillStyle = grad
                        ctx.fill()
                        
                        ctx.beginPath()
                        ctx.moveTo(pts[0].x, pts[0].y)
                        for (var i = 0; i < pts.length - 1; i++) {
                            var p0 = pts[i]
                            var p1 = pts[i + 1]
                            var cx = (p0.x + p1.x) / 2
                            ctx.bezierCurveTo(cx, p0.y, cx, p1.y, p1.x, p1.y)
                        }
                        ctx.strokeStyle = canvas.barColor
                        ctx.lineWidth = 2
                        ctx.stroke()
                    } else if (type === 3) {
                        ctx.lineTo(pts[pts.length - 1].x, midY)
                        for (var i = pts.length - 1; i > 0; i--) {
                            var p1 = pts[i]
                            var p0 = pts[i - 1]
                            var cx = (p0.x + p1.x) / 2
                            var mirroredY0 = midY + (midY - p0.y)
                            var mirroredY1 = midY + (midY - p1.y)
                            ctx.bezierCurveTo(cx, mirroredY1, cx, mirroredY0, p0.x, mirroredY0)
                        }
                        ctx.closePath()
                        
                        var grad = ctx.createLinearGradient(0, midY - maxHeight, 0, midY + maxHeight)
                        grad.addColorStop(0, Qt.rgba(canvas.barColor.r, canvas.barColor.g, canvas.barColor.b, 0.1))
                        grad.addColorStop(0.5, canvas.barColor)
                        grad.addColorStop(1, Qt.rgba(canvas.barColor.r, canvas.barColor.g, canvas.barColor.b, 0.1))
                        ctx.fillStyle = grad
                        ctx.fill()
                    }
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
