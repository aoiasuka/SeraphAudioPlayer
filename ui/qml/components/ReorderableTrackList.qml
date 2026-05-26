import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQml.Models
import QtQuick.Effects

// 可拖拽重排的曲目列表
//   - 外部传入 model (QVariantList of track maps)
//   - 用户按住左侧 grip 拖动整行,松开后触发 itemMoved(from, to)
//   - 列表过滤为空(searchText 不为空)时,不显示 grip,只能上下移按钮
Item {
    id: root

    // 接口
    property var model: []
    property bool allowReorder: true
    property bool showRemove: false
    property var contextMenu: null            // TrackContextMenu 实例

    signal itemClicked(string path)
    signal itemMoved(int from, int to)
    signal itemRemoved(int index, string path)

    // 防止上一行刚被拖完,DropArea 又触发一次:用 dragging 标志保护
    property int draggingIndex: -1

    ListView {
        id: list
        anchors.fill: parent
        model: root.model
        spacing: 2
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        cacheBuffer: 240
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded; width: 8 }

        delegate: Item {
            id: cell
            width: list.width
            height: 60
            z: dragArea.held ? 100 : 1

            property var item: modelData
            property int origIndex: index

            // 显示在该 cell 上方的"插入指示线"
            Rectangle {
                id: hint
                anchors.left: parent.left
                anchors.right: parent.right
                height: 2
                radius: 1
                color: window.brand
                opacity: dropTop.containsDrag ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: 100 } }
                y: 0
            }

            // 实际内容容器
            Rectangle {
                id: content
                width: parent.width
                height: 56
                radius: 12
                color: dragArea.held ? Qt.lighter(window.cardHover, 1.1)
                                     : (item.isCurrent === true ? window.activeBg : "transparent")
                border.color: dragArea.held ? window.brand
                             : (item.isCurrent === true ? window.brand : "transparent")
                border.width: 1
                anchors.horizontalCenter: parent.horizontalCenter
                y: 2

                Behavior on color  { ColorAnimation { duration: 120 } }

                // 拖拽时浮起
                states: State {
                    when: dragArea.held
                    PropertyChanges { target: content; opacity: 0.95 }
                }

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: 4
                    anchors.rightMargin: 8
                    spacing: 8

                    // 拖拽手柄
                    Item {
                        Layout.preferredWidth: 22
                        Layout.preferredHeight: 56
                        visible: root.allowReorder

                        Column {
                            anchors.centerIn: parent
                            spacing: 3
                            Repeater {
                                model: 3
                                delegate: Rectangle {
                                    width: 12; height: 2; radius: 1
                                    color: dragArea.containsMouse ? window.brand : window.textTertiary
                                }
                            }
                        }
                    }

                    // 封面
                    Item {
                        Layout.preferredWidth: 40
                        Layout.preferredHeight: 40
                        clip: true

                        Rectangle {
                            anchors.fill: parent
                            radius: 6
                            color: item.isCurrent === true ? window.brandSoft : "transparent"
                            border.color: window.borderColor
                            border.width: 1
                        }

                        Rectangle {
                            id: reorderListCoverImgMask
                            width: reorderListCoverImg.width
                            height: reorderListCoverImg.height
                            radius: 6
                            color: "black"
                            antialiasing: true
                        }

                        Image {
                            id: reorderListCoverImg
                            anchors.fill: parent
                            source: item.coverUrl || ""
                            visible: source.toString().length > 0 && status === Image.Ready && !(item.isCurrent === true)
                            fillMode: Image.PreserveAspectCrop
                            asynchronous: true
                            cache: true

                            layer.enabled: true
                            layer.effect: MultiEffect {
                                maskEnabled: true
                                maskSource: ShaderEffectSource {
                                    sourceItem: reorderListCoverImgMask
                                    hideSource: true
                                }
                            }
                        }

                        AppIcon {
                            anchors.centerIn: parent
                            visible: !(item.isCurrent === true) && !reorderListCoverImg.visible
                            name: "music"
                            size: 16
                            color: window.textTertiary
                            strokeWidth: 1.6
                        }

                        AppIcon {
                            anchors.centerIn: parent
                            visible: item.isCurrent === true
                            name: "volume"; size: 16; color: window.brand; strokeWidth: 2
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        spacing: 2
                        Text {
                            Layout.fillWidth: true
                            text: item.title || ""
                            font.family: window.fontFamily
                            font.pixelSize: 14
                            font.weight: Font.DemiBold
                            color: item.isCurrent === true ? window.brand : window.textPrimary
                            elide: Text.ElideRight
                        }
                        Text {
                            Layout.fillWidth: true
                            text: (item.artist || "") + (item.album ? " · " + item.album : "")
                            font.family: window.fontFamily
                            font.pixelSize: 12
                            color: window.textSecondary
                            elide: Text.ElideRight
                        }
                    }

                    Text {
                        text: item.duration || ""
                        Layout.preferredWidth: 50
                        horizontalAlignment: Text.AlignRight
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textTertiary
                    }

                    // 更多
                    Item {
                        Layout.preferredWidth: 26
                        Layout.preferredHeight: 26
                        AppIcon {
                            anchors.centerIn: parent
                            name: "more"; size: 16; color: window.textTertiary
                            strokeWidth: 2; filled: true
                        }
                        MouseArea {
                            anchors.fill: parent
                            cursorShape: Qt.PointingHandCursor
                            onClicked: if (root.contextMenu) root.contextMenu.openFor(item.path)
                        }
                    }

                    // 移除
                    Item {
                        visible: root.showRemove
                        Layout.preferredWidth: 26
                        Layout.preferredHeight: 26
                        AppIcon {
                            anchors.centerIn: parent
                            name: "close"; size: 12
                            color: rmArea.containsMouse ? "#DC2626" : window.textTertiary
                            strokeWidth: 2
                        }
                        MouseArea {
                            id: rmArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.itemRemoved(index, item.path)
                        }
                    }
                }

                Drag.active: dragArea.held
                Drag.source: cell
                Drag.hotSpot.x: width / 2
                Drag.hotSpot.y: height / 2
            }

            MouseArea {
                id: dragArea
                anchors.fill: content
                hoverEnabled: true
                cursorShape: held ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                property bool held: false
                drag.target: held ? content : undefined
                drag.axis: Drag.YAxis
                drag.minimumY: -(cell.y)
                drag.maximumY: list.height - cell.height + 4

                onPressed: function(mouse) {
                    if (!root.allowReorder) return
                    // 仅当鼠标在左侧手柄区域(0..30) 才进入拖动
                    if (mouse.x <= 30) {
                        held = true
                        root.draggingIndex = index
                    }
                }
                onReleased: {
                    if (held) {
                        content.Drag.drop()
                        held = false
                        root.draggingIndex = -1
                        // 复位
                        content.x = 0
                        content.y = 2
                    }
                }
                onClicked: {
                    if (!held) root.itemClicked(item.path)
                }
            }

            // 顶部 DropArea:拖到这里 = "插入到本行之前"
            DropArea {
                id: dropTop
                anchors.top: parent.top
                anchors.left: parent.left
                anchors.right: parent.right
                height: 16
                onDropped: function(drop) {
                    var src = drop.source
                    if (!src) return
                    var from = src.origIndex
                    var to = cell.origIndex
                    if (from < to) to -= 1   // 因为 move 后目标位置会相应调整
                    if (from !== to) root.itemMoved(from, to)
                }
            }
        }
    }
}
