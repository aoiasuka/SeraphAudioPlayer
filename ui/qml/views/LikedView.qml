import QtQuick
import "../components"

// 我喜欢的
TrackListPage {
    objectName: "likedView"
    pageTitle: "我喜欢的"
    pageSubtitle: "点击曲目右侧的 ♥ 即可加入此处"
    emptyHint: "尚未收藏任何曲目"
    tracks: playerVM.liked
}
