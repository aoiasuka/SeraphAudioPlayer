# Audio Player 项目深度检索发现的 BUG 及逻辑错误报告

经过对项目核心代码（包括 `core`、`app/controller` 和 `ui/bridge` 等模块）的深度代码审查，发现了以下严重的 BUG 和逻辑错误。请开发者注意修复。

## 1. 多线程数据竞争 (Data Race) 与潜在的 Use-After-Free
**位置**: `app/controller/PlayerController.cpp`
- **函数**: `PlayerController::duration()` 和 `PlayerController::Impl::estimated_position_sec()`
- **描述**: 这些函数（通常由 UI 线程调用）在没有获取 `ctrl_mutex` 或其他任何锁的情况下，直接访问了 `d_->decoder` 指针和 `d_->current_fmt` 等共享变量。然而，后台的 `producer_loop()` 线程在处理无缝播放（Gapless Playback）切换时，会执行 `decoder = std::move(swap)` 重置智能指针；`teardown_session()` 函数中也会调用 `decoder.reset()`。如果在 UI 线程调用 `duration()` 获取时序的同时，工作线程触发了上述指针替换或销毁，将直接导致数据竞争、野指针访问（Use-After-Free）并引发程序崩溃。

## 2. 队列移动导致当前播放索引 (`m_currentIndex`) 定位错误
**位置**: `ui/bridge/PlayerViewModel.cpp`
- **函数**: `PlayerViewModel::moveQueueItem(int from, int to)`
- **描述**: 在拖拽移动播放列表项以改变顺序后，代码尝试使用 `m_queue.indexOf(curPath)` 来恢复更新 `m_currentIndex`（当前正在播放的曲目索引）。
```cpp
    // 重新定位 m_currentIndex
    if (!curPath.isEmpty()) {
        int newIdx = m_queue.indexOf(curPath);
        if (newIdx >= 0 && newIdx != m_currentIndex) {
            m_currentIndex = newIdx;
            emit currentIndexChanged();
        }
    }
```
- **后果**: `QStringList::indexOf` 永远只返回列表中**第一个**匹配的元素索引。如果播放队列中包含多首相同的歌曲，移动操作后，当前播放位置会被错误地拉回/跳跃到列表中第一首相同的歌曲上。应通过 `from`、`to` 与 `m_currentIndex` 之间的数学位移关系来安全更新当前索引。

## 3. 随机播放模式下的“上一首”逻辑错误（无法跨越边界）
**位置**: `ui/bridge/PlayerViewModel.cpp`
- **函数**: `PlayerViewModel::previous()`
- **描述**: 在随机播放模式 (`m_shuffle == true`) 且指针达到列表最前端 (`m_shufflePos <= 0`) 时，逻辑有误：
```cpp
    if (m_shuffle) {
        if (m_shufflePos <= 0) idx = m_shuffleOrder.isEmpty() ? -1 : m_shuffleOrder.first();
        else idx = m_shuffleOrder.value(--m_shufflePos, -1);
    }
```
- **后果**: 若用户在随机播放序列的开头点击“上一首”，代码不仅忽略了循环播放设置 (`m_repeatMode == 1`)，也不会将 `m_shufflePos` 绕回到列表末尾。相反，它会反复提取 `m_shuffleOrder.first()`，即不断从头播放当前正在播放的第一首歌，无法继续往回跳转。

## 4. 播放位置计算非原子性导致进度条倒退和抖动 (Jitter)
**位置**: `app/controller/PlayerController.cpp`
- **函数**: `PlayerController::Impl::estimated_position_sec()`
- **描述**: 获取当前估计的播放进度时，采用的是：
```cpp
        const std::int64_t decoded_frames = decoder->currentFrame();
        const std::int64_t in_ring_frames = static_cast<std::int64_t>(ring->readable() / frame_bytes);
        const std::int64_t played = decoded_frames - in_ring_frames ...
```
- **后果**: 这里没有加锁保证这两个状态的同步读取。如果 UI 线程刚读取完 `decoded_frames` (例如 1000 帧)，后台生产者线程立刻运行并向解码器请求了 500 帧塞入 `ring`，然后 UI 线程才去读取 `ring->readable()`。此时 `in_ring_frames` 会包含了新产生的 500 帧数据，两者相减 `played` 会突然变小 (从 1000 变为 500)。表现在 UI 上，就是播放器进度条在偶尔会发生向后跳跃/倒退的视觉闪烁。应当保证读取这两个指标属于同一瞬时快照。
