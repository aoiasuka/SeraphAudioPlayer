import QtQuick
import "../components"

// 最近播放
TrackListPage {
    objectName: "historyView"
    pageTitle: "最近播放"
    pageSubtitle: "按时间倒序,最多保留 50 首"
    emptyHint: "暂无播放记录"
    tracks: playerVM.recent
    showClearAll: true

    onClearAllRequested: playerVM.clearRecent()
}
