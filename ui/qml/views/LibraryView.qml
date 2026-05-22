import QtQuick
import "../components"

// 音乐库 — 聚合所有已知曲目
TrackListPage {
    objectName: "libraryView"
    pageTitle: "音乐库"
    pageSubtitle: "本机所有已加入队列、播放过或收藏的曲目"
    emptyHint: "音乐库为空 — 点右上「添加文件」导入音频"
    emptyAction: "添加音频文件"
    tracks: playerVM.library
    showOpenFile: true

    onEmptyActionRequested: {
        // 复用页面顶部的同一 FileDialog
        // 该 FileDialog 在 TrackListPage 中内置且为非可见对象,
        // 这里用同样的逻辑通过 ViewModel 触发
    }
}
