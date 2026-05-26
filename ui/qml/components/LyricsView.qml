import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

// 歌词面板 — 居中显示当前行,周围行渐淡;有翻译时副行小字显示
Item {
    id: root

    readonly property var lyrics: playerVM.currentLyrics || []
    readonly property int activeIndex: playerVM.currentLyricIndex
    readonly property var meta: playerVM.lyricsMeta || ({})

    onActiveIndexChanged: scrollToActive()
    onWidthChanged: scrollToActive()
    onHeightChanged: scrollToActive()

    function scrollToActive() {
        if (activeIndex < 0 || activeIndex >= list.count) return
        list.positionViewAtIndex(activeIndex, ListView.Center)
    }
    Component.onCompleted: scrollToActive()

    // 顶部薄状态条:歌词来源 + 操作按钮
    Rectangle {
        id: topbar
        anchors.top: parent.top
        anchors.left: parent.left
        anchors.right: parent.right
        height: 28
        color: "transparent"
        visible: true

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: 12
            anchors.rightMargin: 8
            spacing: 8

            Text {
                Layout.fillWidth: true
                text: {
                    if (!playerVM.hasLyrics) return ""
                    var src = root.meta.source || ""
                    var off = root.meta.offset_ms || 0
                    var parts = []
                    if (src === "manual") parts.push("手动加载")
                    else if (src === "external") parts.push(".lrc")
                    else if (src === "embedded") parts.push("内嵌")
                    if (Math.abs(off) > 0.5) parts.push("offset " + off.toFixed(0) + "ms")
                    if (root.meta.by) parts.push("by " + root.meta.by)
                    return parts.join(" · ")
                }
                font.family: window.fontFamily
                font.pixelSize: 11
                color: window.textTertiary
                elide: Text.ElideRight
            }

            component LrcButton: Button {
                id: btn
                property string toolTipText: ""
                padding: 4
                leftPadding: 10
                rightPadding: 10
                
                ToolTip.text: toolTipText
                ToolTip.visible: hovered
                
                contentItem: Text {
                    text: btn.text
                    font.family: window.fontFamily
                    font.pixelSize: 11
                    font.weight: btn.hovered ? Font.Medium : Font.Normal
                    color: btn.hovered ? window.textPrimary : window.textSecondary
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                }
                
                background: Rectangle {
                    radius: height / 2
                    color: btn.pressed ? "#1A000000" : (btn.hovered ? window.hoverBg : "transparent")
                    Behavior on color { ColorAnimation { duration: 120 } }
                }
            }

            LrcButton {
                text: "刷新"
                toolTipText: "重新从磁盘扫描同名 .lrc"
                onClicked: playerVM.refreshLyrics()
            }
            LrcButton {
                text: "加载..."
                toolTipText: "选一个 .lrc 文件覆盖当前歌词"
                onClicked: lrcDialog.open()
            }
            LrcButton {
                text: "清空"
                visible: playerVM.hasLyrics
                toolTipText: "清空当前显示的歌词"
                onClicked: playerVM.clearLyrics()
            }
        }
    }

    FileDialog {
        id: lrcDialog
        title: "选择歌词文件 (.lrc)"
        nameFilters: ["LRC 歌词 (*.lrc *.LRC *.txt)"]
        onAccepted: playerVM.loadExternalLyrics(selectedFile.toString())
    }

    // 空态
    Item {
        anchors.top: topbar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        visible: !playerVM.hasLyrics

        ColumnLayout {
            anchors.centerIn: parent
            spacing: 8
            Text {
                Layout.alignment: Qt.AlignHCenter
                text: "暂无歌词"
                font.family: window.fontFamily
                font.pixelSize: 14
                color: window.textSecondary
            }
            Text {
                Layout.alignment: Qt.AlignHCenter
                text: '把同名 .lrc 文件放在音频旁边,或点上方 "加载..." 选取'
                font.family: window.fontFamily
                font.pixelSize: 12
                color: window.textTertiary
            }
        }
    }

    ListView {
        id: list
        anchors.top: topbar.bottom
        anchors.bottom: parent.bottom
        anchors.left: parent.left
        anchors.right: parent.right
        visible: playerVM.hasLyrics
        clip: true
        model: root.lyrics
        spacing: 6
        boundsBehavior: Flickable.StopAtBounds
        topMargin: height / 2 - 30
        bottomMargin: height / 2 - 30

        highlightMoveVelocity: -1
        highlightMoveDuration: 350

        delegate: Item {
            width: list.width
            // 有翻译副行时多一行高度
            height: (modelData.hasTranslation ? 50 : 32)

            readonly property bool isActive: index === root.activeIndex
            readonly property int  distance: Math.abs(index - root.activeIndex)
            readonly property real fadeOpacity:
                root.activeIndex < 0 ? 0.85
                : (isActive ? 1.0
                : (distance === 1 ? 0.70
                : (distance === 2 ? 0.45 : 0.28)))

            Column {
                anchors.fill: parent
                spacing: 2

                Text {
                    width: parent.width
                    horizontalAlignment: Text.AlignHCenter
                    verticalAlignment: Text.AlignVCenter
                    text: modelData.text
                    font.family: window.fontFamily
                    font.pixelSize: isActive ? 16 : 14
                    font.weight: isActive ? Font.Bold : Font.Medium
                    color: isActive ? window.brand : window.textSecondary
                    opacity: fadeOpacity
                    elide: Text.ElideRight
                    Behavior on font.pixelSize { NumberAnimation { duration: 200 } }
                    Behavior on opacity { NumberAnimation { duration: 250 } }
                    Behavior on color { ColorAnimation { duration: 200 } }
                }
                Text {
                    width: parent.width
                    visible: modelData.hasTranslation
                    horizontalAlignment: Text.AlignHCenter
                    text: modelData.translation
                    font.family: window.fontFamily
                    font.pixelSize: isActive ? 12 : 11
                    color: window.textTertiary
                    opacity: fadeOpacity * 0.9
                    elide: Text.ElideRight
                }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                onClicked: function(mouse) {
                    if (mouse.button === Qt.RightButton) {
                        ctxMenu.popup()
                    } else {
                        if (modelData.time !== undefined) playerVM.seek(modelData.time)
                    }
                }
            }
            Menu {
                id: ctxMenu
                MenuItem { text: "跳到此行"; onTriggered: playerVM.seek(modelData.time) }
                MenuItem { text: "刷新歌词";   onTriggered: playerVM.refreshLyrics() }
                MenuItem { text: "加载外部..."; onTriggered: lrcDialog.open() }
                MenuItem { text: "清空歌词";   onTriggered: playerVM.clearLyrics() }
            }
        }
    }
}
