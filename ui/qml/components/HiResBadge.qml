import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

// Hi-Res 高保真金色徽章 — 参考 Synapse HiFi UI
//
// 视觉:
//   - 渐变金底 (#fffbf0 → #fff8e1) + 1px 金色 (#d4af37) 描边
//   - 左侧小标签 "Hi-Res"
//   - 中间显示格式/采样率 (从 playerVM.formatInfo 读取)
//   - 右侧 "Sample Rate / Bitrate" 说明文字
Rectangle {
    id: root
    implicitHeight: 22
    implicitWidth: row.implicitWidth + 14
    radius: 4
    border.color: window.goldBorder
    border.width: 1
    antialiasing: true

    // 文本由外部传入, 默认从 playerVM.formatInfo 取
    property string formatText: playerVM.formatInfo || ""

    gradient: Gradient {
        orientation: Gradient.Horizontal
        GradientStop { position: 0; color: window.goldBgTop }
        GradientStop { position: 1; color: window.goldBgBottom }
    }

    RowLayout {
        id: row
        anchors.centerIn: parent
        spacing: 6

        // 左侧 "Hi-Res" 小标签
        Rectangle {
            Layout.preferredHeight: 14
            Layout.preferredWidth: hires.implicitWidth + 8
            radius: 3
            color: "#FFFBF0"
            border.color: Qt.rgba(0.83, 0.69, 0.22, 0.6)  // #d4af37 @60%
            border.width: 1

            Text {
                id: hires
                anchors.centerIn: parent
                text: "Hi-Res"
                font.family: window.fontFamily
                font.pixelSize: 9
                font.weight: Font.Bold
                color: window.goldText
            }
        }

        Text {
            id: techText
            text: root.formatText.length > 0 ? root.formatText : "PCM"
            font.family: window.fontFamily
            font.pixelSize: 10
            font.weight: Font.DemiBold
            color: window.goldText
        }

        Text {
            text: "Sample Rate / Bitrate"
            font.family: window.fontFamily
            font.pixelSize: 9
            color: window.textTertiary
            visible: root.width > techText.x + techText.implicitWidth + 100
        }
    }
}
