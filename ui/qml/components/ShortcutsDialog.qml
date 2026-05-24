import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// 键盘快捷键设置对话框 (F1 / Ctrl+/ 触发)
// 支持改键: 点击 sequence chip 进入录制模式, 下一次按键即被记录
Dialog {
    id: root
    modal: true
    anchors.centerIn: parent
    width: Math.min(parent ? parent.width - 80 : 760, 760)
    height: 580
    padding: 0
    standardButtons: Dialog.NoButton

    // 正在录制的 action id; 空表示无录制
    property string recordingId: ""

    Overlay.modal: Rectangle {
        color: window.modalScrim
        Behavior on opacity { NumberAnimation { duration: 180 } }
    }

    background: Rectangle {
        radius: window.largeRadius
        color: window.surface
        border.color: window.borderColor
        border.width: 1
        antialiasing: true
    }

    header: Item { visible: false; height: 0 }
    footer: Item { visible: false; height: 0 }

    // ----- 录制助手 -----
    // 把 Qt.Key_* + modifiers 转为标准 QKeySequence 字符串 (如 "Ctrl+Shift+P")
    function nameForKey(key) {
        // 常用键映射. Qt 没暴露名字反查 API, 这里手动列表 (覆盖快捷键最常用的范围)
        var map = {}
        map[Qt.Key_Space] = "Space"
        map[Qt.Key_Tab] = "Tab"
        map[Qt.Key_Backtab] = "Tab"
        map[Qt.Key_Return] = "Return"
        map[Qt.Key_Enter] = "Enter"
        map[Qt.Key_Escape] = "Escape"
        map[Qt.Key_Backspace] = "Backspace"
        map[Qt.Key_Delete] = "Delete"
        map[Qt.Key_Insert] = "Insert"
        map[Qt.Key_Home] = "Home"
        map[Qt.Key_End] = "End"
        map[Qt.Key_PageUp] = "PgUp"
        map[Qt.Key_PageDown] = "PgDown"
        map[Qt.Key_Left] = "Left"
        map[Qt.Key_Right] = "Right"
        map[Qt.Key_Up] = "Up"
        map[Qt.Key_Down] = "Down"
        map[Qt.Key_Comma] = ","
        map[Qt.Key_Period] = "."
        map[Qt.Key_Slash] = "/"
        map[Qt.Key_Backslash] = "\\"
        map[Qt.Key_Semicolon] = ";"
        map[Qt.Key_Apostrophe] = "'"
        map[Qt.Key_BracketLeft] = "["
        map[Qt.Key_BracketRight] = "]"
        map[Qt.Key_Minus] = "-"
        map[Qt.Key_Equal] = "="
        map[Qt.Key_QuoteLeft] = "`"
        for (var i = 0; i < 12; ++i) map[Qt.Key_F1 + i] = "F" + (i + 1)
        if (map[key]) return map[key]
        // A..Z / 0..9 用字符值
        if (key >= Qt.Key_A && key <= Qt.Key_Z) return String.fromCharCode(0x41 + (key - Qt.Key_A))
        if (key >= Qt.Key_0 && key <= Qt.Key_9) return String.fromCharCode(0x30 + (key - Qt.Key_0))
        return ""
    }
    function sequenceFromEvent(event) {
        var parts = []
        if (event.modifiers & Qt.ControlModifier) parts.push("Ctrl")
        if (event.modifiers & Qt.AltModifier)     parts.push("Alt")
        if (event.modifiers & Qt.ShiftModifier)   parts.push("Shift")
        if (event.modifiers & Qt.MetaModifier)    parts.push("Meta")
        var k = nameForKey(event.key)
        if (!k) return ""
        parts.push(k)
        return parts.join("+")
    }

    // 焦点项: 录制中接管键盘. 空闲下不接管 (让快捷键能正常工作).
    Item {
        id: keyCapture
        anchors.fill: parent
        focus: root.recordingId !== ""
        Keys.onPressed: function(event) {
            // 录制时屏蔽掉单独按下修饰键的事件
            if (event.key === Qt.Key_Control || event.key === Qt.Key_Shift ||
                event.key === Qt.Key_Alt || event.key === Qt.Key_Meta) {
                return
            }
            event.accepted = true
            if (event.key === Qt.Key_Escape) {
                root.recordingId = ""    // 取消
                return
            }
            var seq = root.sequenceFromEvent(event)
            if (!seq) return
            shortcutsVM.setKey(root.recordingId, seq)
            root.recordingId = ""
        }
    }

    contentItem: Item {
        anchors.fill: parent

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: 20
            spacing: 12

            // 标题栏
            RowLayout {
                Layout.fillWidth: true
                spacing: 8

                Text {
                    text: "键盘快捷键"
                    font.family: window.fontFamily
                    font.pixelSize: 18
                    font.weight: Font.Bold
                    color: window.textPrimary
                }
                Text {
                    text: root.recordingId.length > 0
                        ? "按下新键以记录,Esc 取消"
                        : "点击右侧键位按钮可改键"
                    font.family: window.fontFamily
                    font.pixelSize: 12
                    color: root.recordingId.length > 0 ? window.brand : window.textSecondary
                }
                Item { Layout.fillWidth: true }

                // 全部恢复
                Rectangle {
                    Layout.preferredHeight: 28
                    Layout.preferredWidth: resetAllTxt.implicitWidth + 24
                    radius: 14
                    color: resetAllArea.containsMouse ? window.surfaceAlt : "transparent"
                    border.color: window.borderColor
                    border.width: 1
                    Text {
                        id: resetAllTxt
                        anchors.centerIn: parent
                        text: "全部恢复"
                        font.family: window.fontFamily
                        font.pixelSize: 12
                        color: window.textPrimary
                    }
                    MouseArea {
                        id: resetAllArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: shortcutsVM.resetAll()
                    }
                }

                // 关闭
                Item {
                    Layout.preferredWidth: 30
                    Layout.preferredHeight: 30
                    Rectangle {
                        anchors.fill: parent
                        radius: 15
                        color: shortcutCloseArea.containsMouse ? window.hoverBg : "transparent"
                        Behavior on color { ColorAnimation { duration: 120 } }
                    }
                    AppIcon {
                        anchors.centerIn: parent
                        name: "close"; size: 14
                        color: window.textPrimary; strokeWidth: 2
                    }
                    MouseArea {
                        id: shortcutCloseArea
                        anchors.fill: parent
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: { root.recordingId = ""; root.close() }
                    }
                }
            }

            ScrollView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                clip: true

                ColumnLayout {
                    width: root.availableWidth - 40
                    spacing: 16

                    Repeater {
                        model: shortcutsVM.groups
                        delegate: ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 6

                            Text {
                                text: modelData.title
                                font.family: window.fontFamily
                                font.pixelSize: 13
                                font.weight: Font.DemiBold
                                color: window.brand
                            }

                            Repeater {
                                model: modelData.items
                                delegate: RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 12

                                    // label
                                    Text {
                                        Layout.fillWidth: true
                                        text: modelData.label
                                        font.family: window.fontFamily
                                        font.pixelSize: 13
                                        color: window.textPrimary
                                    }

                                    // custom indicator
                                    Text {
                                        visible: modelData.custom
                                        text: "已自定义"
                                        font.family: window.fontFamily
                                        font.pixelSize: 11
                                        color: window.brand
                                    }

                                    // 恢复默认
                                    Rectangle {
                                        visible: modelData.custom
                                        Layout.preferredHeight: 26
                                        Layout.preferredWidth: rdTxt.implicitWidth + 16
                                        radius: 13
                                        color: rdArea.containsMouse ? window.surfaceAlt : "transparent"
                                        border.color: window.borderColor
                                        border.width: 1
                                        Text {
                                            id: rdTxt
                                            anchors.centerIn: parent
                                            text: "重置"
                                            font.family: window.fontFamily
                                            font.pixelSize: 11
                                            color: window.textPrimary
                                        }
                                        MouseArea {
                                            id: rdArea
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: shortcutsVM.resetKey(modelData.id)
                                        }
                                    }

                                    // 当前键位 (chip)
                                    Rectangle {
                                        Layout.preferredHeight: 28
                                        Layout.preferredWidth: Math.max(110, keyTxt.implicitWidth + 24)
                                        radius: window.smallRadius
                                        color: root.recordingId === modelData.id
                                            ? window.brand
                                            : (keyArea.containsMouse ? window.surfaceAlt : window.surface)
                                        border.color: root.recordingId === modelData.id
                                            ? window.brand : window.borderColor
                                        border.width: 1
                                        Behavior on color { ColorAnimation { duration: 120 } }

                                        Text {
                                            id: keyTxt
                                            anchors.centerIn: parent
                                            text: root.recordingId === modelData.id
                                                ? "等待按键..."
                                                : modelData.key
                                            font.family: "Consolas"
                                            font.pixelSize: 12
                                            font.weight: Font.DemiBold
                                            color: root.recordingId === modelData.id
                                                ? "#FFFFFF" : window.textPrimary
                                        }
                                        MouseArea {
                                            id: keyArea
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            cursorShape: Qt.PointingHandCursor
                                            onClicked: {
                                                if (root.recordingId === modelData.id) {
                                                    root.recordingId = ""
                                                } else {
                                                    root.recordingId = modelData.id
                                                    keyCapture.forceActiveFocus()
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    onClosed: root.recordingId = ""
}
