// =============================================================================
//  app/playlist/Playlist.cpp
// =============================================================================
#include "Playlist.h"

#include <algorithm>
#include <random>

namespace apx {

Playlist::Playlist() = default;

void Playlist::clear()
{
    items_.clear();
    current_index_ = -1;
    shuffle_recent_.clear();
}

void Playlist::append(PlaylistItem it)
{
    items_.push_back(std::move(it));
}

void Playlist::insert(std::size_t at, PlaylistItem it)
{
    if (at > items_.size()) at = items_.size();
    items_.insert(items_.begin() + at, std::move(it));
    // 调整 current_index
    if (current_index_ >= static_cast<int>(at)) current_index_ += 1;
}

void Playlist::removeAt(std::size_t i)
{
    if (i >= items_.size()) return;
    items_.erase(items_.begin() + i);
    if (items_.empty()) {
        current_index_ = -1;
    } else if (current_index_ == static_cast<int>(i)) {
        // 当前被删:停留在同位置,若超出则回退到最后
        if (current_index_ >= static_cast<int>(items_.size()))
            current_index_ = static_cast<int>(items_.size()) - 1;
    } else if (current_index_ > static_cast<int>(i)) {
        current_index_ -= 1;
    }
}

void Playlist::move(std::size_t from, std::size_t to)
{
    if (from >= items_.size() || to >= items_.size() || from == to) return;
    PlaylistItem tmp = std::move(items_[from]);
    items_.erase(items_.begin() + from);
    items_.insert(items_.begin() + to, std::move(tmp));
    // 调 current_index
    if (current_index_ == static_cast<int>(from)) {
        current_index_ = static_cast<int>(to);
    } else if (from < to
               && current_index_ > static_cast<int>(from)
               && current_index_ <= static_cast<int>(to)) {
        current_index_ -= 1;
    } else if (from > to
               && current_index_ >= static_cast<int>(to)
               && current_index_ < static_cast<int>(from)) {
        current_index_ += 1;
    }
}

void Playlist::appendPath(const std::wstring& path)
{
    PlaylistItem it;
    it.path = path;
    items_.push_back(std::move(it));
}

bool Playlist::setCurrentIndex(int i)
{
    if (i < -1 || i >= static_cast<int>(items_.size())) return false;
    current_index_ = i;
    return true;
}

std::optional<PlaylistItem> Playlist::currentItem() const
{
    if (current_index_ < 0 || current_index_ >= static_cast<int>(items_.size()))
        return std::nullopt;
    return items_[current_index_];
}

int Playlist::pickShuffle() const
{
    const int n = static_cast<int>(items_.size());
    if (n == 0) return -1;
    if (n == 1) return 0;

    // 不在最近历史中的索引集合
    std::vector<int> candidates;
    candidates.reserve(n);
    for (int i = 0; i < n; ++i) {
        if (std::find(shuffle_recent_.begin(), shuffle_recent_.end(), i)
            == shuffle_recent_.end()) {
            candidates.push_back(i);
        }
    }
    if (candidates.empty()) candidates.assign(n, 0), candidates.clear();
    if (candidates.empty()) {
        // 历史满了 → 重置历史,但保留 current_index 避免立刻重复
        shuffle_recent_.clear();
        for (int i = 0; i < n; ++i) if (i != current_index_) candidates.push_back(i);
        if (candidates.empty()) return current_index_;
    }
    // C++ <random> 默认 mt19937;线程局部静态保证不会被并发摧毁
    thread_local std::mt19937 rng{std::random_device{}()};
    std::uniform_int_distribution<int> dist(0, static_cast<int>(candidates.size()) - 1);
    return candidates[dist(rng)];
}

int Playlist::peekNext() const
{
    if (items_.empty()) return -1;
    switch (mode_) {
    case PlaybackMode::LoopOne:
        return current_index_ < 0 ? 0 : current_index_;
    case PlaybackMode::Shuffle:
        return pickShuffle();
    case PlaybackMode::LoopList:
        if (current_index_ < 0) return 0;
        return (current_index_ + 1) % static_cast<int>(items_.size());
    case PlaybackMode::Sequential:
    default:
        if (current_index_ < 0) return 0;
        if (current_index_ + 1 >= static_cast<int>(items_.size())) return -1;
        return current_index_ + 1;
    }
}

int Playlist::peekPrev() const
{
    if (items_.empty()) return -1;
    switch (mode_) {
    case PlaybackMode::LoopOne:
        return current_index_ < 0 ? 0 : current_index_;
    case PlaybackMode::Shuffle:
        return pickShuffle();   // 上一首在随机模式下也是再抽一个,简单实用
    case PlaybackMode::LoopList:
        if (current_index_ <= 0) return static_cast<int>(items_.size()) - 1;
        return current_index_ - 1;
    case PlaybackMode::Sequential:
    default:
        if (current_index_ <= 0) return -1;
        return current_index_ - 1;
    }
}

int Playlist::advanceNext()
{
    const int nx = peekNext();
    if (nx >= 0) {
        current_index_ = nx;
        if (mode_ == PlaybackMode::Shuffle) {
            shuffle_recent_.push_back(nx);
            // 历史长度 = min(items.size()/2, 32),避免短列表里反复同一首
            const std::size_t cap =
                std::min<std::size_t>(items_.size() / 2 + 1, 32u);
            while (shuffle_recent_.size() > cap)
                shuffle_recent_.erase(shuffle_recent_.begin());
        }
    }
    return nx;
}

int Playlist::advancePrev()
{
    const int p = peekPrev();
    if (p >= 0) current_index_ = p;
    return p;
}

} // namespace apx
