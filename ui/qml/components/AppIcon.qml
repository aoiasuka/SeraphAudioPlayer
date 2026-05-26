import QtQuick

// 统一图标组件 - 用 Canvas + SVG path 解析器绘制 (不依赖 QtQuick.Shapes)
// 用法: AppIcon { name: "home"; size: 22; color: "#1C1C1E" }
Item {
    id: root
    property string name: ""
    property int size: 20
    property color color: "#1C1C1E"
    property real strokeWidth: 1.8
    property bool filled: false

    width: size
    height: size

    // 全部按 24x24 viewBox 设计
    readonly property var paths: ({
        "menu":     "M3 6h18 M3 12h18 M3 18h18",
        "home":     "M3 10l9-7 9 7v10a2 2 0 0 1-2 2h-4v-7h-6v7H5a2 2 0 0 1-2-2V10z",
        "library":  "M4 4h4v16H4z M10 4h4v16h-4z M16 6l3 1 3 13-3 1z",
        "playlist": "M3 6h13 M3 12h13 M3 18h9 M17 14v6 M17 14l4-1v6",
        "artist":   "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8z M4 21c0-4 4-7 8-7c4 0 8 3 8 7",
        "album":    "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18z M12 14a2 2 0 1 0 0-4 2 2 0 0 0 0 4z",
        "history":  "M3 12a9 9 0 1 0 3-6.7 M3 4v5h5 M12 7v5l3 2",
        "heart":    "M12 21.35l-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41 .81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35Z",
        "settings": "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M19 13l2-1-2-3-2 1 M5 13l-2-1 2-3 2 1 M13 5l-1-2-3 0 1 2 M11 19l1 2 3 0-1-2",
        "plus":     "M12 5v14 M5 12h14",
        "search":   "M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.3-4.3",
        "play":     "M8 5v14l11-7z",
        "pause":    "M6 5h4v14H6z M14 5h4v14h-4z",
        "next":     "M5 5l10 7-10 7z M18 5h2v14h-2z",
        "prev":     "M19 5l-10 7 10 7z M6 5H4v14h2z",
        "shuffle":  "M16 3h5v5 M21 3l-7 7 M3 21l7-7 M16 21h5v-5 M21 21l-7-7 M3 3l7 7",
        "repeat":   "M17 1l4 4-4 4 M3 11V9a4 4 0 0 1 4-4h14 M7 23l-4-4 4-4 M21 13v2a4 4 0 0 1-4 4H3",
        "volume":   "M11 5L6 9H2v6h4l5 4V5z M16 8a5 5 0 0 1 0 8 M19 5a9 9 0 0 1 0 14",
        "volume-mute": "M11 5L6 9H2v6h4l5 4V5z M23 9l-6 6 M17 9l6 6",
        "list":     "M8 6h13 M8 12h13 M8 18h13 M3 6h.01 M3 12h.01 M3 18h.01",
        "more":     "M6 12a1.5 1.5 0 1 0 0.01 0 M12 12a1.5 1.5 0 1 0 0.01 0 M18 12a1.5 1.5 0 1 0 0.01 0",
        "min":      "M5 12h14",
        "max":      "M5 5h14v14h-14z",
        "close":    "M6 6l12 12 M18 6l-12 12",
        "chevron":  "M9 6l6 6-6 6",
        "run":      "M13 4a2 2 0 1 0 0-4 2 2 0 0 0 0 4z M5 12l3-3 4 2 4-5 3 4 M5 20l3-5 3 2 3-4",
        "briefcase":"M3 8h18v12H3z M8 8V5a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v3",
        "music":    "M9 18V5l12-2v13 M9 18a3 3 0 1 1-6 0 3 3 0 0 1 6 0z M21 16a3 3 0 1 1-6 0 3 3 0 0 1 6 0z",
        "sliders":  "M4 21v-7 M4 10V3 M12 21v-9 M12 8V3 M20 21v-5 M20 12V3 M1 14h6 M9 8h6 M17 16h6"
    })

    Canvas {
        id: canvas
        anchors.fill: parent
        antialiasing: true
        contextType: "2d"

        readonly property string p: root.paths[root.name] || ""
        readonly property color strokeCol: root.filled ? "transparent" : root.color
        readonly property color fillCol: root.filled ? root.color : "transparent"
        readonly property real sw: root.strokeWidth

        onPChanged: requestPaint()
        onStrokeColChanged: requestPaint()
        onFillColChanged: requestPaint()
        onSwChanged: requestPaint()
        onWidthChanged: requestPaint()
        onHeightChanged: requestPaint()
        Component.onCompleted: requestPaint()

        onPaint: {
            var ctx = getContext("2d")
            if (!ctx) return
            ctx.reset()
            if (!p || width <= 0 || height <= 0) return

            var s = width / 24.0
            ctx.scale(s, s)
            ctx.lineJoin = "round"
            ctx.lineCap = "round"
            ctx.lineWidth = sw

            __drawSvgPath(ctx, p)

            if (root.filled) {
                ctx.fillStyle = fillCol
                ctx.fill()
            } else {
                ctx.strokeStyle = strokeCol
                ctx.stroke()
            }
        }

        // ===== SVG path 解析与绘制 (支持 M m L l H h V v C c Q q A a Z z) =====
        function __drawSvgPath(ctx, d) {
            ctx.beginPath()
            var i = 0
            var x = 0, y = 0
            var startX = 0, startY = 0
            var prevCmd = ""

            function skip() {
                while (i < d.length && (d[i] === ' ' || d[i] === ',' || d[i] === '\t' || d[i] === '\n')) i++
            }
            function readNum() {
                skip()
                var s = i
                if (i < d.length && (d[i] === '-' || d[i] === '+')) i++
                while (i < d.length && ((d[i] >= '0' && d[i] <= '9') || d[i] === '.')) i++
                if (i < d.length && (d[i] === 'e' || d[i] === 'E')) {
                    i++
                    if (i < d.length && (d[i] === '-' || d[i] === '+')) i++
                    while (i < d.length && d[i] >= '0' && d[i] <= '9') i++
                }
                return parseFloat(d.substring(s, i))
            }
            function peekIsNum() {
                skip()
                if (i >= d.length) return false
                var c = d[i]
                return c === '-' || c === '+' || c === '.' || (c >= '0' && c <= '9')
            }

            while (i < d.length) {
                skip()
                if (i >= d.length) break
                var c = d[i]
                var cmd
                if ((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z')) {
                    cmd = c
                    i++
                } else {
                    // 隐式重复上一命令
                    cmd = prevCmd
                    if (!cmd) break
                    if (cmd === 'M') cmd = 'L'
                    else if (cmd === 'm') cmd = 'l'
                }
                prevCmd = cmd

                do {
                    switch (cmd) {
                    case 'M': x = readNum(); y = readNum(); ctx.moveTo(x, y); startX = x; startY = y; break
                    case 'm': x += readNum(); y += readNum(); ctx.moveTo(x, y); startX = x; startY = y; break
                    case 'L': x = readNum(); y = readNum(); ctx.lineTo(x, y); break
                    case 'l': x += readNum(); y += readNum(); ctx.lineTo(x, y); break
                    case 'H': x = readNum(); ctx.lineTo(x, y); break
                    case 'h': x += readNum(); ctx.lineTo(x, y); break
                    case 'V': y = readNum(); ctx.lineTo(x, y); break
                    case 'v': y += readNum(); ctx.lineTo(x, y); break
                    case 'C': {
                        var c1x = readNum(), c1y = readNum()
                        var c2x = readNum(), c2y = readNum()
                        var ex = readNum(), ey = readNum()
                        ctx.bezierCurveTo(c1x, c1y, c2x, c2y, ex, ey)
                        x = ex; y = ey
                        break
                    }
                    case 'c': {
                        var rc1x = x + readNum(), rc1y = y + readNum()
                        var rc2x = x + readNum(), rc2y = y + readNum()
                        var rex = x + readNum(), rey = y + readNum()
                        ctx.bezierCurveTo(rc1x, rc1y, rc2x, rc2y, rex, rey)
                        x = rex; y = rey
                        break
                    }
                    case 'Q': {
                        var qcx2 = readNum(), qcy2 = readNum()
                        var qex2 = readNum(), qey2 = readNum()
                        ctx.quadraticCurveTo(qcx2, qcy2, qex2, qey2)
                        x = qex2; y = qey2
                        break
                    }
                    case 'q': {
                        var qcx = x + readNum(), qcy = y + readNum()
                        var qex = x + readNum(), qey = y + readNum()
                        ctx.quadraticCurveTo(qcx, qcy, qex, qey)
                        x = qex; y = qey
                        break
                    }
                    case 'A':
                    case 'a': {
                        var rx = readNum(), ry = readNum()
                        var rot = readNum()
                        var laf = readNum(), sf = readNum()
                        var nx = readNum(), ny = readNum()
                        if (cmd === 'a') { nx += x; ny += y }
                        __svgArc(ctx, x, y, rx, ry, rot, laf, sf, nx, ny)
                        x = nx; y = ny
                        break
                    }
                    case 'Z': case 'z':
                        ctx.closePath()
                        x = startX; y = startY
                        break
                    default:
                        return
                    }
                } while (peekIsNum() && cmd !== 'Z' && cmd !== 'z')
            }
        }

        // SVG 椭圆弧 -> Canvas (用 ellipse 模拟)
        function __svgArc(ctx, x1, y1, rx, ry, rotDeg, largeArc, sweep, x2, y2) {
            if (rx === 0 || ry === 0 || (x1 === x2 && y1 === y2)) {
                ctx.lineTo(x2, y2)
                return
            }
            rx = Math.abs(rx); ry = Math.abs(ry)
            var rot = rotDeg * Math.PI / 180
            var cosR = Math.cos(rot), sinR = Math.sin(rot)

            var dx = (x1 - x2) / 2, dy = (y1 - y2) / 2
            var x1p =  cosR * dx + sinR * dy
            var y1p = -sinR * dx + cosR * dy

            var rxSq = rx * rx, rySq = ry * ry
            var x1pSq = x1p * x1p, y1pSq = y1p * y1p

            var radCheck = x1pSq / rxSq + y1pSq / rySq
            if (radCheck > 1) {
                var sq = Math.sqrt(radCheck)
                rx *= sq; ry *= sq
                rxSq = rx * rx; rySq = ry * ry
            }

            var sign = (largeArc === sweep) ? -1 : 1
            var num = rxSq * rySq - rxSq * y1pSq - rySq * x1pSq
            var den = rxSq * y1pSq + rySq * x1pSq
            var coef = sign * Math.sqrt(Math.max(0, num / den))
            var cxp = coef *  (rx * y1p / ry)
            var cyp = coef * -(ry * x1p / rx)

            var cx = cosR * cxp - sinR * cyp + (x1 + x2) / 2
            var cy = sinR * cxp + cosR * cyp + (y1 + y2) / 2

            function ang(ux, uy, vx, vy) {
                var dot = ux * vx + uy * vy
                var len = Math.sqrt((ux*ux + uy*uy) * (vx*vx + vy*vy))
                var a = Math.acos(Math.max(-1, Math.min(1, dot / len)))
                return (ux * vy - uy * vx < 0) ? -a : a
            }

            var theta1 = ang(1, 0, (x1p - cxp) / rx, (y1p - cyp) / ry)
            var dTheta = ang((x1p - cxp) / rx, (y1p - cyp) / ry,
                             (-x1p - cxp) / rx, (-y1p - cyp) / ry)
            if (!sweep && dTheta > 0) dTheta -= 2 * Math.PI
            else if (sweep && dTheta < 0) dTheta += 2 * Math.PI

            ctx.save()
            ctx.translate(cx, cy)
            ctx.rotate(rot)
            ctx.scale(rx, ry)
            ctx.arc(0, 0, 1, theta1, theta1 + dTheta, !sweep)
            ctx.restore()
        }
    }
}
