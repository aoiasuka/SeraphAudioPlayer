import QtQuick
import QtQuick.Controls

// 曲目右键/更多菜单 (毛玻璃 + 大圆角 + 主题色 Hover)
Menu {
    id: root

    property string path: ""
    property bool liked: false

    function openFor(filePath) {
        root.path = filePath
        root.liked = playerVM.isLiked(filePath)
        root.popup()
    }

    padding: 6
    topPadding: 8
    bottomPadding: 8
    leftPadding: 6
    rightPadding: 6

    // 背景：深色毛玻璃 + 大圆角 + 半透明白边
    background: Rectangle {
        implicitWidth: 220
        radius: window.mediumRadius
        color: window.menuBg
        border.color: window.glassBorder
        border.width: 1
        antialiasing: true
    }

    // 自定义菜单项：圆角矩形 Hover、统一字体颜色、合适的内边距
    delegate: MenuItem {
        id: menuItem
        implicitHeight: 32
        leftPadding: 12
        rightPadding: 12

        contentItem: Text {
            text: menuItem.text
            font.family: window.fontFamily
            font.pixelSize: 13
            color: "#FFFFFF"
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
            opacity: menuItem.enabled ? 1.0 : 0.5
        }

        background: Rectangle {
            implicitHeight: 32
            radius: window.smallRadius
            color: menuItem.highlighted ? window.menuHoverBg : "transparent"
            Behavior on color { ColorAnimation { duration: 120 } }
        }
    }

    MenuItem {
        text: "立即播放"
        onTriggered: playerVM.openFile(root.path)
    }
    MenuItem {
        text: "加入队列"
        onTriggered: playerVM.enqueue(root.path)
    }

    Menu {
        id: addToPlaylistMenu
        title: "加入歌单"

        padding: 6
        topPadding: 8
        bottomPadding: 8
        leftPadding: 6
        rightPadding: 6

        background: Rectangle {
            implicitWidth: 220
            radius: window.mediumRadius
            color: window.menuBg
            border.color: window.glassBorder
            border.width: 1
            antialiasing: true
        }

        delegate: MenuItem {
            id: subMenuItem
            implicitHeight: 32
            leftPadding: 12
            rightPadding: 12
            contentItem: Text {
                text: subMenuItem.text
                font.family: window.fontFamily
                font.pixelSize: 13
                color: "#FFFFFF"
                verticalAlignment: Text.AlignVCenter
                elide: Text.ElideRight
            }
            background: Rectangle {
                implicitHeight: 32
                radius: window.smallRadius
                color: subMenuItem.highlighted ? window.menuHoverBg : "transparent"
                Behavior on color { ColorAnimation { duration: 120 } }
            }
        }

        Instantiator {
            model: playerVM.playlists
            delegate: MenuItem {
                text: modelData.name + " (" + modelData.count + ")"
                onTriggered: playerVM.addToPlaylist(modelData.id, root.path)
            }
            onObjectAdded: function(index, object) {
                addToPlaylistMenu.insertItem(index, object)
            }
            onObjectRemoved: function(index, object) {
                addToPlaylistMenu.removeItem(object)
            }
        }

        MenuSeparator {
            contentItem: Rectangle {
                implicitHeight: 1
                implicitWidth: 180
                color: window.glassBorder
            }
        }
        MenuItem {
            text: "新建歌单..."
            onTriggered: {
                var id = playerVM.createPlaylist("新建歌单")
                playerVM.addToPlaylist(id, root.path)
            }
        }
    }

    MenuSeparator {
        contentItem: Rectangle {
            implicitHeight: 1
            implicitWidth: 180
            color: window.glassBorder
        }
    }
    MenuItem {
        text: root.liked ? "取消喜欢" : "添加到我喜欢"
        onTriggered: playerVM.toggleLike(root.path)
    }
    MenuSeparator {
        contentItem: Rectangle {
            implicitHeight: 1
            implicitWidth: 180
            color: window.glassBorder
        }
    }
    MenuItem {
        text: "从最近播放中移除"
        onTriggered: playerVM.removeFromRecent(root.path)
    }
}
