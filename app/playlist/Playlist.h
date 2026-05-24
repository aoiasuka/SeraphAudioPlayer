// =============================================================================
//  app/playlist/Playlist.h
//
//  播放列表数据模型。承担三件事:
//    1) 维护一组 PlaylistItem(每项至少有 path,可附 title/artist/album/duration
//       /track index/cue 偏移 等元数据)
//    2) 维护"当前项"游标 (current_index)
//    3) 在 next/prev 时按"播放模式"(顺序/列表循环/单曲循环/随机)给出下一项
//
//  线程模型:
//    所有方法都是控制线程调用,内部不加锁。需要并发访问的调用方自己加锁。
//
//  与 PlayerController 的关系:
//    Playlist 是纯数据。怎么"加载并播放"是 controller 的责任。
//    典型用法:UI 选中某项 → controller.loadFile(playlist.itemAt(idx).path)
// =============================================================================
#pragma once

#include <cstdint>
#include <optional>
#include <string>
#include <vector>

namespace apx {

struct PlaylistItem {
    std::wstring  path;            // 文件路径
    std::wstring  title;           // 显示用;来自元数据或文件名
    std::wstring  artist;
    std::wstring  album;
    std::uint32_t track_index = 0; // CUE/M3U 内的 track # (1-based,0=未知)
    double        duration_sec = 0.0;
    // CUE 一文件多 track 时,start/end 在源文件内的秒偏移;non-CUE 项填 0/0
    double        cue_start_sec = 0.0;
    double        cue_end_sec   = 0.0;  // 0 表示直到 EOF
};

enum class PlaybackMode : std::uint8_t {
    Sequential = 0,  // 顺序,到末尾停
    LoopList   = 1,  // 列表循环
    LoopOne    = 2,  // 单曲循环
    Shuffle    = 3,  // 随机
};

class Playlist {
public:
    Playlist();
    ~Playlist() = default;

    Playlist(const Playlist&)            = delete;
    Playlist& operator=(const Playlist&) = delete;
    Playlist(Playlist&&) noexcept            = default;
    Playlist& operator=(Playlist&&) noexcept = default;

    // ---------- 增删改查 ----------
    void clear();
    std::size_t size() const                { return items_.size(); }
    bool        empty() const               { return items_.empty(); }
    const PlaylistItem& itemAt(std::size_t i) const { return items_.at(i); }
    PlaylistItem&       mutableItemAt(std::size_t i) { return items_.at(i); }
    const std::vector<PlaylistItem>& items() const  { return items_; }

    void append(PlaylistItem it);
    void insert(std::size_t at, PlaylistItem it);
    void removeAt(std::size_t i);
    void move(std::size_t from, std::size_t to);
    // 仅追加 path,其它字段留空;UI 异步补元数据
    void appendPath(const std::wstring& path);

    // ---------- 当前位置 ----------
    int  currentIndex() const               { return current_index_; }
    bool setCurrentIndex(int i);            // 越界返回 false
    std::optional<PlaylistItem> currentItem() const;

    // ---------- 模式 + 推进 ----------
    void         setMode(PlaybackMode m)    { mode_ = m; }
    PlaybackMode mode() const               { return mode_; }

    // 计算下一首/上一首索引 (不修改 current_index)
    // 模式效果:
    //   Sequential: 末尾 → -1 (无下一首)
    //   LoopList  : 末尾 → 0
    //   LoopOne   : 总是 current
    //   Shuffle   : 从未播过的项里随机
    int peekNext() const;
    int peekPrev() const;

    // 真正前进/回退,返回新 current_index (-1 表示没了);Shuffle 模式会刷新历史
    int advanceNext();
    int advancePrev();

    // 序列化(为后续 M3U / JSON 落盘留接口;本版只做 in-memory)
    // 留 TODO,不在此实现

private:
    std::vector<PlaylistItem> items_;
    int                       current_index_ = -1;   // -1 = 无当前
    PlaybackMode              mode_ = PlaybackMode::Sequential;
    // Shuffle 历史(防止短列表里立即重复)
    mutable std::vector<int>  shuffle_recent_;
    int  pickShuffle() const;
};

} // namespace apx
