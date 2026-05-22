import QtQuick
import QtQuick.Layouts
import QtQuick.Effects

// Hero 横幅 — 现代极简风格的单首"最近播放"
// 设计:
//   - 半透明奶白卡片(让窗口动态主色背景透出微妙色调)
//   - 极细 1px 半透明 border, 16px 大圆角, 无重阴影
//   - 右侧大圆角封面 + 同色系柔和环境光晕(无 blur, 多层低透明叠加模拟)
//   - 左侧: WAV 小标签 + 大字标题 + 歌手·专辑副标题 + 深色精细胶囊 CTA
//   - CTA hover 时轻微缩放 (1.0 -> 1.04)
//
// 实现要点:
//   - 不使用 Rectangle.clip:true (Qt 6 中 Rectangle 的 clip 是矩形剪裁,
//     不会按 radius 裁角, 子项溢出圆角会破坏视觉; 改为通过控制子项位置/尺寸避免溢出)
//   - 光晕用 3 层叠加的纯色圆角矩形, 各层 opacity 渐减, 视觉柔和且无扩散问题
Rectangle {
    id: root
    radius: 16
    color: "transparent"
    antialiasing: true

    // ===== 全局背景填充 (歌曲封面) =====
    Rectangle {
        id: bgMask
        anchors.fill: parent
        radius: 16
        color: "black"
        antialiasing: true
    }

    Image {
        id: bgImg
        anchors.fill: parent
        source: root.hasCover ? root.currentItem.coverUrl : ""
        fillMode: Image.PreserveAspectCrop
        asynchronous: true
        cache: true
        visible: root.hasCover

        layer.enabled: true
        layer.effect: MultiEffect {
            maskEnabled: true
            maskSource: ShaderEffectSource {
                sourceItem: bgMask
                hideSource: true
            }
        }
    }

    // 深色遮罩，保证文字可读性
    Rectangle {
        anchors.fill: parent
        radius: 16
        color: "#B3020617" // 70% 极暗蓝，确保白色文字绝对清晰
        visible: root.hasCover
        antialiasing: true
    }

    // 无封面时的后备底色
    Rectangle {
        anchors.fill: parent
        radius: 16
        color: "#0F172A" // slate-900
        visible: !root.hasCover
        antialiasing: true
    }

    // 边框 (放在最上层以防被遮挡)
    Rectangle {
        anchors.fill: parent
        radius: 16
        color: "transparent"
        border.color: "#26FFFFFF" // 15% white
        border.width: 1
        antialiasing: true
    }

    // 单首数据 (HomeView 传入)
    property var currentItem: null
    property string fallbackTitle: ""
    property string fallbackSubtitle: ""

    signal playClicked()                  // 无 item 时触发 (用于打开文件)
    signal itemClicked(string path)       // 有 item 时触发 (播放该曲目)

    readonly property bool hasCover:
        currentItem && (currentItem.coverUrl || "").length > 0

    // ===== 右侧封面 (主视觉) =====
    Item {
        id: coverSlot
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.rightMargin: 36
        width: Math.min(parent.height - 64, 156)
        height: width

        // 同色系环境光晕: 3 层叠加纯色圆角矩形, 透明度渐减
        // 不用 MultiEffect blur, 避免色块扩散到整张卡片
        Rectangle {
            anchors.centerIn: parent
            width: parent.width + 28
            height: parent.height + 28
            radius: 22
            color: playerVM.currentDominantColor || "#3B82F6"
            opacity: root.hasCover ? 0.10 : 0
            visible: root.hasCover
            z: -1
            Behavior on opacity { NumberAnimation { duration: 300 } }
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width + 16
            height: parent.height + 16
            radius: 18
            color: playerVM.currentDominantColor || "#3B82F6"
            opacity: root.hasCover ? 0.16 : 0
            visible: root.hasCover
            z: -1
            Behavior on opacity { NumberAnimation { duration: 300 } }
        }
        Rectangle {
            anchors.centerIn: parent
            width: parent.width + 6
            height: parent.height + 6
            radius: 14
            color: playerVM.currentDominantColor || "#3B82F6"
            opacity: root.hasCover ? 0.22 : 0
            visible: root.hasCover
            z: -1
            Behavior on opacity { NumberAnimation { duration: 300 } }
        }

        // 封面卡: 圆角 + clip (Image 自带矩形,需要 clip 才贴合圆角)
        Rectangle {
            anchors.fill: parent
            radius: 12
            color: "#1E293B"                  // slate-800, 占位底色
            border.color: "#33FFFFFF"
            border.width: 1
            antialiasing: true
            clip: true

            Rectangle {
                id: heroImgMask
                width: heroImg.width
                height: heroImg.height
                radius: 11
                color: "black"
                antialiasing: true
            }

            Image {
                id: heroImg
                anchors.fill: parent
                anchors.margins: 1
                source: root.hasCover ? root.currentItem.coverUrl : ""
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
                cache: true
                visible: root.hasCover

                layer.enabled: true
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: ShaderEffectSource {
                        sourceItem: heroImgMask
                        hideSource: true
                    }
                }
            }

            // 占位音符 (无封面)
            AppIcon {
                anchors.centerIn: parent
                visible: !root.hasCover
                name: "music"
                size: parent.width * 0.3
                color: "#94A3B8"              // slate-400
                strokeWidth: 1.4
            }
        }
    }

    // ===== 左侧: 文字 + CTA =====
    ColumnLayout {
        anchors.left: parent.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.right: coverSlot.left
        anchors.leftMargin: 36
        anchors.rightMargin: 32
        anchors.topMargin: 32
        anchors.bottomMargin: 32
        spacing: 0

        // 顶部小标签: [WAV] 或 [本地]
        RowLayout {
            spacing: 8

            // 类型标签 (WAV / FLAC / MP3 / 本地)
            Rectangle {
                Layout.preferredHeight: 22
                Layout.preferredWidth: tagText.implicitWidth + 16
                radius: 11
                color: "#33FFFFFF"   // 半透明白底
                border.color: "#1AFFFFFF"
                border.width: 1

                Text {
                    id: tagText
                    anchors.centerIn: parent
                    text: {
                        if (root.currentItem && root.currentItem.suffix) return root.currentItem.suffix
                        return "本地"
                    }
                    font.family: window.fontFamily
                    font.pixelSize: 10
                    font.weight: Font.DemiBold
                    font.letterSpacing: 0.6
                    color: "#F8FAFC"      // slate-50
                }
            }

            // 章节标签
            Text {
                text: root.currentItem ? "最近播放" : "本地音乐"
                font.family: window.fontFamily
                font.pixelSize: 11
                font.weight: Font.DemiBold
                font.letterSpacing: 1.4
                color: "#94A3B8"      // slate-400
            }
        }

        Item { Layout.fillHeight: true }

        // 主标题 — 大字粗体
        Text {
            Layout.fillWidth: true
            Layout.topMargin: 8
            text: root.currentItem
                  ? (root.currentItem.title || "未知曲目")
                  : (root.fallbackTitle.length > 0 ? root.fallbackTitle : "导入音频开始播放")
            font.family: window.fontFamily
            font.pixelSize: 30
            font.weight: Font.Bold
            font.letterSpacing: -0.4
            color: "#FFFFFF"          // 纯白
            elide: Text.ElideRight
        }

        // 副标题: 歌手 · 专辑 (严禁本地路径)
        Text {
            Layout.fillWidth: true
            Layout.topMargin: 10
            text: {
                if (!root.currentItem) return root.fallbackSubtitle
                var a = root.currentItem.artist || ""
                var b = root.currentItem.album || ""
                // 跳过"目录路径"类副标题 (PlayerViewModel 在缺元数据时会用 dir 兜底为 album)
                // 路径多以盘符或斜杠开头 — 检测后丢弃
                if (b && (b.indexOf("/") >= 0 || b.indexOf("\\") >= 0 || /^[A-Za-z]:/.test(b))) b = ""
                if (a && b) return a + "  ·  " + b
                if (a)      return a
                if (b)      return b
                return "未知信息"
            }
            font.family: window.fontFamily
            font.pixelSize: 14
            color: "#94A3B8"
            elide: Text.ElideRight
        }

        Item { Layout.fillHeight: true }

        // CTA: 深色精细胶囊, hover 缩放
        Item {
            id: ctaWrap
            Layout.preferredHeight: 40
            Layout.preferredWidth: 132

            // 用 scale + Behavior 实现"轻轻放大"
            scale: ctaArea.containsMouse ? 1.04 : 1.0
            Behavior on scale { NumberAnimation { duration: 160; easing.type: Easing.OutQuad } }

            Rectangle {
                anchors.fill: parent
                radius: 20
                color: ctaArea.pressed ? "#E2E8F0"
                     : (ctaArea.containsMouse ? "#F8FAFC" : "#FFFFFF")
                Behavior on color { ColorAnimation { duration: 150 } }
            }

            RowLayout {
                anchors.centerIn: parent
                spacing: 8

                AppIcon {
                    name: "play"
                    size: 12
                    color: "#0F172A"
                    filled: true
                }
                Text {
                    text: root.currentItem ? "立即播放" : "选择文件"
                    font.family: window.fontFamily
                    font.pixelSize: 13
                    font.weight: Font.DemiBold
                    color: "#0F172A"
                    font.letterSpacing: 0.2
                }
            }

            MouseArea {
                id: ctaArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (root.currentItem) root.itemClicked(root.currentItem.path)
                    else root.playClicked()
                }
            }
        }
    }
}
