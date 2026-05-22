import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 歌词面板 — 居中显示当前行,周围行渐淡
Item {
    id: root

    readonly property var lyrics: playerVM.currentLyrics || []
    readonly property int activeIndex: playerVM.currentLyricIndex

    // 当前行变化时滚到中间
    onActiveIndexChanged: scrollToActive()
    onWidthChanged: scrollToActive()
    onHeightChanged: scrollToActive()

    function scrollToActive() {
        if (activeIndex < 0 || activeIndex >= list.count) return
        // SnapPosition.Center 让目标项滚到视口中央
        list.positionViewAtIndex(activeIndex, ListView.Center)
    }

    Component.onCompleted: scrollToActive()

    // 空态
    Item {
        anchors.centerIn: parent
        visible: !playerVM.hasLyrics
        width: parent.width
        height: 80

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
                text: "把同名 .lrc 文件放在音频旁边即可显示"
                font.family: window.fontFamily
                font.pixelSize: 12
                color: window.textTertiary
            }
        }
    }

    ListView {
        id: list
        anchors.fill: parent
        visible: playerVM.hasLyrics
        clip: true
        model: root.lyrics
        spacing: 6
        boundsBehavior: Flickable.StopAtBounds
        // 整体上下留白,让首末行也能滚到中间
        topMargin: height / 2 - 30
        bottomMargin: height / 2 - 30

        // 平滑滚动
        highlightMoveVelocity: -1
        highlightMoveDuration: 350

        delegate: Item {
            width: list.width
            height: 32

            readonly property bool isActive: index === root.activeIndex
            readonly property int distance: Math.abs(index - root.activeIndex)
            readonly property real fadeOpacity:
                root.activeIndex < 0 ? 0.85
                : (isActive ? 1.0
                : (distance === 1 ? 0.70
                : (distance === 2 ? 0.45 : 0.28)))

            Text {
                anchors.fill: parent
                horizontalAlignment: Text.AlignHCenter
                verticalAlignment: Text.AlignVCenter
                text: modelData.text
                font.family: window.fontFamily
                font.pixelSize: isActive ? 16 : 14
                font.weight: isActive ? Font.DemiBold : Font.Medium
                color: isActive ? window.textPrimary : window.textSecondary
                opacity: fadeOpacity
                elide: Text.ElideRight
                Behavior on font.pixelSize { NumberAnimation { duration: 200 } }
                Behavior on opacity { NumberAnimation { duration: 250 } }
                Behavior on color { ColorAnimation { duration: 200 } }
            }

            MouseArea {
                anchors.fill: parent
                cursorShape: Qt.PointingHandCursor
                onClicked: {
                    if (modelData.time !== undefined) playerVM.seek(modelData.time)
                }
            }
        }
    }
}
