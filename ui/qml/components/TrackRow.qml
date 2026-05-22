import QtQuick
import QtQuick.Layouts
import QtQuick.Effects

// 单行歌曲 — 现代极简风格
// 设计:
//   - 高 64px (留呼吸感), 12px 圆角, hover 时整行柔和暗背景过渡
//   - 行尾常驻: 心 (喜欢) + 时长; hover 时浮现: 加入队列 + 更多
//   - 当前曲目: 左侧出现一条短竖线 + 标题着色, 不再用整行高亮抢主视觉
Item {
    id: root
    implicitHeight: 64

    property color coverColor: "transparent"
    property string title: ""
    property string artist: ""
    property string album: ""
    property string duration: "00:00"
    property bool liked: false
    // 是否当前正在播放的曲目
    property bool isCurrent: false
    // 封面 URL (有就显示)
    property string coverUrl: ""
    // 用于拖拽的曲目路径
    property string path: ""
    // 是否启用上下移按钮(歌单内重排)
    property bool showReorder: false
    // 是否启用"从此处移除"按钮 (歌单 / 队列)
    property bool showRemove: false
    
    readonly property bool rowHovered: rowArea.containsMouse
                                    || enqArea.containsMouse
                                    || upArea.containsMouse
                                    || downArea.containsMouse
                                    || removeArea.containsMouse
                                    || likeBtnArea.containsMouse
                                    || moreArea.containsMouse

    signal clicked()
    signal likeClicked()
    signal moreClicked()
    signal enqueueClicked()
    signal moveUpClicked()
    signal moveDownClicked()
    signal removeClicked()

    // ===== 整行背景 (也使用了黑色的半透明度，更百搭) =====
    Rectangle {
        anchors.fill: parent
        anchors.topMargin: 1
        anchors.bottomMargin: 1
        radius: 12
        color: root.isCurrent ? Qt.rgba(0, 0, 0, 0.08)
             : (root.rowHovered ? Qt.rgba(0, 0, 0, 0.04) : "transparent")
        border.color: "transparent"
        border.width: 0
        Behavior on color { ColorAnimation { duration: 160 } }
    }

    // 当前播放: 左侧短竖线指示
    Rectangle {
        visible: root.isCurrent
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: 2
        width: 3
        height: parent.height * 0.45
        radius: 1.5
        color: Qt.rgba(0, 0, 0, 0.6) // 也是用半透明
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 16

        // ===== 封面缩略图 =====
        Item {
            Layout.preferredWidth: 44
            Layout.preferredHeight: 44
            clip: true

            Rectangle {
                anchors.fill: parent
                radius: 8
                color: Qt.rgba(1, 1, 1, 0.5) // 底色用半透明白
                border.color: Qt.rgba(0, 0, 0, 0.08)
                border.width: 1
            }

            Rectangle {
                id: tnImgMask
                width: tnImg.width
                height: tnImg.height
                radius: 7
                color: "black"
                antialiasing: true
            }

            Image {
                id: tnImg
                anchors.fill: parent
                anchors.margins: 1
                source: root.coverUrl
                visible: source.toString().length > 0 && status === Image.Ready
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
                cache: true

                layer.enabled: true
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: ShaderEffectSource {
                        sourceItem: tnImgMask
                        hideSource: true
                    }
                }
            }

            // 当前播放: 封面蒙版(确保喇叭图标可见)
            Rectangle {
                anchors.fill: parent
                anchors.margins: 1
                radius: 7
                color: Qt.rgba(0, 0, 0, 0.4)
                visible: root.isCurrent && tnImg.visible
            }

            // 当前播放: 显示喇叭
            AppIcon {
                anchors.centerIn: parent
                visible: root.isCurrent
                name: "volume"
                size: 18
                color: tnImg.visible ? "#FFFFFF" : Qt.rgba(0, 0, 0, 0.6)
                strokeWidth: 1.8
            }

            // 无封面占位
            AppIcon {
                anchors.centerIn: parent
                visible: !root.isCurrent && (!root.coverUrl || root.coverUrl.length === 0)
                name: "music"
                size: 18
                color: Qt.rgba(0, 0, 0, 0.3)
                strokeWidth: 1.5
            }
        }

        // ===== 标题 (自适应，占据主要比例) =====
        Text {
            Layout.fillWidth: true
            Layout.preferredWidth: 350
            Layout.minimumWidth: 120
            text: root.title
            font.family: window.fontFamily
            font.pixelSize: 14
            font.weight: Font.DemiBold
            // 标题使用 85% 或 95% 不透明度黑，层级最高
            color: root.isCurrent ? Qt.rgba(0, 0, 0, 0.95) : Qt.rgba(0, 0, 0, 0.85)
            elide: Text.ElideRight
        }

        // ===== 歌手 (按比例自适应) =====
        Text {
            Layout.fillWidth: true
            Layout.preferredWidth: 180
            Layout.minimumWidth: 80
            text: root.artist
            font.family: window.fontFamily
            font.pixelSize: 13
            // 歌手使用 60% 不透明度
            color: Qt.rgba(0, 0, 0, 0.60)
            elide: Text.ElideRight
        }

        // ===== 专辑 (按比例自适应) =====
        Text {
            Layout.fillWidth: true
            Layout.preferredWidth: 220
            Layout.minimumWidth: 80
            text: {
                var b = root.album || ""
                if (b && (b.indexOf("/") >= 0 || b.indexOf("\\") >= 0 || /^[A-Za-z]:/.test(b))) return ""
                return b
            }
            font.family: window.fontFamily
            font.pixelSize: 13
            // 专辑使用 45% 不透明度
            color: Qt.rgba(0, 0, 0, 0.45)
            elide: Text.ElideRight
        }

        // ===== 时长与悬浮操作组 (固定宽度容器) =====
        Item {
            Layout.preferredWidth: 68
            Layout.fillHeight: true

            // 时长
            Text {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                text: root.duration
                font.family: window.fontFamily
                font.pixelSize: 12
                font.weight: Font.Medium
                // 时长使用 45% 不透明度
                color: Qt.rgba(0, 0, 0, 0.45)
                opacity: root.rowHovered ? 0 : 1
                Behavior on opacity { NumberAnimation { duration: 150 } }
            }

            // Hover 操作组
            Row {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                spacing: 2
                opacity: root.rowHovered ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 150 } }

                // 加入队列
                Item {
                    width: 28; height: 28
                    visible: !root.showRemove && !root.showReorder

                    Rectangle {
                        anchors.fill: parent
                        radius: 14
                        color: enqArea.containsMouse ? Qt.rgba(0, 0, 0, 0.06) : "transparent"
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        name: "plus"
                        size: 14
                        // Hover时85%，默认45%
                        color: enqArea.containsMouse ? Qt.rgba(0, 0, 0, 0.85) : Qt.rgba(0, 0, 0, 0.45)
                        strokeWidth: 2
                    }
                    MouseArea {
                        id: enqArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.enqueueClicked()
                    }
                }

                // 上移 (歌单重排)
                Item {
                    width: 26; height: 26
                    visible: root.showReorder
                    AppIcon {
                        anchors.centerIn: parent
                        name: "chevron"
                        rotation: -90
                        size: 14
                        color: upArea.containsMouse ? Qt.rgba(0, 0, 0, 0.85) : Qt.rgba(0, 0, 0, 0.45)
                        strokeWidth: 2
                    }
                    MouseArea {
                        id: upArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.moveUpClicked()
                    }
                }
                
                // 下移
                Item {
                    width: 26; height: 26
                    visible: root.showReorder
                    AppIcon {
                        anchors.centerIn: parent
                        name: "chevron"
                        rotation: 90
                        size: 14
                        color: downArea.containsMouse ? Qt.rgba(0, 0, 0, 0.85) : Qt.rgba(0, 0, 0, 0.45)
                        strokeWidth: 2
                    }
                    MouseArea {
                        id: downArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.moveDownClicked()
                    }
                }

                // 从此处移除
                Item {
                    width: 28; height: 28
                    visible: root.showRemove
                    Rectangle {
                        anchors.fill: parent
                        radius: 14
                        color: removeArea.containsMouse ? Qt.rgba(239/255, 68/255, 68/255, 0.15) : "transparent"
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        name: "close"
                        size: 12
                        color: removeArea.containsMouse ? "#DC2626" : Qt.rgba(0, 0, 0, 0.45)
                        strokeWidth: 2
                    }
                    MouseArea {
                        id: removeArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: root.removeClicked()
                    }
                }
            }
        }

        // ===== 喜欢 (常显) =====
        Item {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            Rectangle {
                anchors.fill: parent
                radius: 16
                color: likeBtnArea.containsMouse ? Qt.rgba(0, 0, 0, 0.06) : "transparent"
                Behavior on color { ColorAnimation { duration: 120 } }
            }
            AppIcon {
                anchors.centerIn: parent
                name: "heart"
                size: 17
                // 点赞时用主题红，否则根据hover状态调整透明度
                color: root.liked ? window.likeRed
                     : (likeBtnArea.containsMouse ? Qt.rgba(0, 0, 0, 0.65) : Qt.rgba(0, 0, 0, 0.45))
                filled: root.liked
                strokeWidth: 1.8
            }
            MouseArea {
                id: likeBtnArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.likeClicked()
            }
        }

        // ===== 更多 (常显, hover 时圆背景) =====
        Item {
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            Rectangle {
                anchors.fill: parent
                radius: 16
                color: moreArea.containsMouse ? Qt.rgba(0, 0, 0, 0.06) : "transparent"
                Behavior on color { ColorAnimation { duration: 120 } }
            }
            AppIcon {
                anchors.centerIn: parent
                name: "more"
                size: 16
                // hover时85%，默认45%
                color: moreArea.containsMouse ? Qt.rgba(0, 0, 0, 0.85) : Qt.rgba(0, 0, 0, 0.45)
                strokeWidth: 2
                filled: true
            }
            MouseArea {
                id: moreArea
                anchors.fill: parent
                hoverEnabled: true
                cursorShape: Qt.PointingHandCursor
                onClicked: root.moreClicked()
            }
        }
    }

    // ===== 整行点击区 =====
    MouseArea {
        id: rowArea
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: root.clicked()
        // 不阻挡子按钮 — 让子项的 MouseArea 优先处理
        z: -1

        // 拖拽支持
        drag.target: dragGhost
        drag.threshold: 8
        onPressed: function(mouse) {
            dragGhost.x = mouse.x
            dragGhost.y = mouse.y
        }
        onReleased: {
            dragGhost.Drag.drop()
            dragGhost.x = 0
            dragGhost.y = 0
        }
    }

    // 拖拽源 (不可见,仅承载 mimeData)
    Item {
        id: dragGhost
        width: 1; height: 1
        Drag.active: rowArea.drag.active
        Drag.dragType: Drag.Internal
        Drag.supportedActions: Qt.CopyAction
        Drag.mimeData: {
            "application/x-apx-track": root.path
        }
    }
}