# Seraph Audio Player — Bug 筛查报告

> 审查时间：2026-06-10
> 范围：`src/`（React 前端）、`crates/`（Rust 音频核心）、`src-tauri/`（Tauri 后端），共约 1.1 万行源码逐文件人工审查。
> 严重程度：🔴 高（功能错误/崩溃/假死） · 🟠 中（明显可感知的行为错误） · 🟡 低（边缘场景/一致性/性能）

---

## 🔴 高严重度

### 1. 播放中 seek 请求被解码线程"吞掉"，解码器从未真正跳转
**文件**：`crates/seraph-audio/src/engine.rs:1069-1076`（`push_samples`）、`engines.rs:962-968`（`run_decode_worker` 循环顶部）

`engine.seek()` 把目标秒数写入 `shared.seek_request`，本应由 `run_decode_worker` 循环顶部取出并调用 `decoder.seek()`。但稳态播放时（3 秒环形缓冲已满），解码线程几乎总是阻塞在 `push_samples` 内部的 sleep 循环里；`push_samples` 在第 1070 行 `seek_request.lock().take()` **消费掉了请求**，只更新了 `frame_position` 就 `return`，既没有调 `decoder.seek()`，也没有把请求放回去。回到主循环时 `seek_request` 已经是 `None`。

**后果**：拖动进度条后，UI 显示新位置，但实际播放的是解码器旧位置继续解出的音频（最多偏差 ≈3 秒缓冲量），音画严重不符。

**修复建议**：`push_samples` 里检测到 seek 请求时不要 `take()`，而是把请求**留在原位**（或重新 `*lock = Some(seconds)`）后提前返回，让主循环统一执行 `decoder.seek()` + `resampler.reset()`。

---

### 2. WASAPI 独占渲染线程出错后静默退出，UI 永久假死在"播放中"
**文件**：`crates/seraph-audio/src/engine.rs:813-833`（渲染循环）、`engine.rs:467-471`（`stop_session`）

独占模式渲染 worker 在 `get_available_space_in_frames` / `write_to_device` 失败（典型场景：拔掉 USB DAC、设备被其他独占程序抢占）时直接 `return Err(...)`，这个错误**只有**在下一次 `stop_session` join 时被 `warn!` 打一条日志，从不向 `EventBus` 发布 `Error` / `PlaybackStopped` 事件。解码线程随后把环形缓冲填满后永久 sleep。

**后果**：设备丢失后无声、进度条冻结、播放按钮仍显示"暂停"图标，用户无任何错误提示。共享模式（cpal）的 `err_fn` 同样只 `warn!`（`engine.rs:526`），存在同类问题。

**修复建议**：给渲染 worker 传入 `EventBus`，出错时发布 `PlayerEvent::Error` + `PlaybackStopped` 并置 `shared.stopped = true`，让解码线程一并退出。

---

## 🟠 中严重度

### 3. 曲尾缓冲期（EOF drain）内 seek 会直接"播完"整首歌
**文件**：`crates/seraph-audio/src/engine.rs:1022-1034`

解码到 EOF 后进入 drain 循环等待环形缓冲被消费完，这个循环**完全不检查 `seek_request`**。此时用户往回拖进度条：`engine.seek()` 会 bump `buffer_generation`，渲染线程把剩余旧 generation 样本全部丢弃 → 缓冲瞬间清空 → drain 循环退出 → 发布 `PlaybackEnded` → 前端自动切下一曲。

**后果**：在最后约 3 秒（缓冲长度）内任何回拖操作都会导致跳到下一首，而不是回放。

**修复建议**：drain 循环中也检查 `seek_request`，命中时回到主解码循环（需要把主循环和 drain 合并或用状态标记）。

---

### 4. 应用重启后引擎音量与 UI 音量不同步
**文件**：`crates/seraph-audio/src/engine.rs:207`（默认 `volume: 0.7`）、`src/store/player.ts:207-216`（`applyOutputConfiguration`）、`src/hooks/useHydratePlayerStore.ts`

前端把音量持久化到 localStorage，但 rehydrate 后（`useHydratePlayerStore`）以及每次播放前（`applyOutputConfiguration` 只发 `set_output_driver` / `select_output_device`）都**不会**向后端发送 `set_volume`。引擎侧默认 0.7。

**后果**：用户上次把音量调到 20% 并重启应用，UI 显示 20%，但实际播放音量是 70%——直到用户碰一下音量条才矫正。可能造成突然的大音量。

**修复建议**：在 `applyOutputConfiguration()` 中追加 `set_volume`（取 `isMuted ? 0 : volume`），或 rehydrate 完成后立即同步一次。

---

### 5. 随机模式下"下一首播放"预览每次渲染都变，且与实际切歌结果不一致
**文件**：`src/store/player.ts:146-164`（`nextShuffleTrackIndex` 使用 `Math.random`）、`player.ts:681-691`（`nextTrackPreview`）、`src/components/sidebar/UpNextCard.tsx:6`

`nextTrackPreview()` 在 zustand selector 里直接调用，随机模式下每次 store 更新（包括每 250ms 的 progress 事件）都会重新 `Math.random()` 选一首：
1. UpNext 卡片上的曲目名高频闪变；
2. 用户点击"下一首"时 `nextTrack()` 又**独立**随机一次，实际播放的几乎从不等于预览显示的那首。

**修复建议**：把"已抽中的下一首"缓存进 store（如 `pendingShuffleIndex`），`nextTrackPreview` 与 `nextTrack` 共用，曲目切换后再重新抽。

---

### 6. 截断/损坏的 DSF 文件可触发 slice 越界 panic
**文件**：`crates/seraph-decoder/src/dsd.rs:454-488`（`decode_dsf_block`）、`dsd.rs:170-179`（`next_packet` 单次 `read` 短读）

`next_packet` 用单次 `file.read()` 读取一个 block 组（未循环读满），且 `data_len`（头部声明）可能大于实际文件剩余字节。当末块短读时 `raw.len() < channels * block_size_per_channel`，`decode_dsf_block` 用 `per_channel_bytes = raw.len() / channels` 算可用帧数，但取样时 `channel_offset = channel * block_size_per_channel + frame_offset` 仍按**完整 block 步长**索引 —— 当 `block_size_per_channel > per_channel_bytes` 时，对第 2+ 声道的 `&raw[channel_offset..channel_offset+8]` 会越界 panic，导致解码线程 abort（panic in thread）。

**修复建议**：读满 `read_len`（循环 read 或 `read_exact` 容错），并在 `decode_dsf_block` 中限制 `pcm_frames` 使 `(channels-1)*block_size + frames*8 <= raw.len()`，或直接丢弃不完整的末块组。

---

### 7. 未知时长（duration=0）的曲目进度永远显示 0:00
**文件**：`crates/seraph-audio/src/engine.rs:1116`（`seconds.min(total_seconds.max(0.0))`）、`src/hooks/usePlayback.ts:29-30`（`duration = track?.duration ?? Infinity` 后 `Math.min(seconds, duration)`）

后端把进度钳到 `total`（=0），前端又把进度钳到 `track.duration`（=0，非 nullish 不会落到 Infinity 分支）。双重钳制下，元数据探测不到时长的曲目（部分 ffprobe 失败的流、损坏头文件）进度条与时间标签永远停在 0:00，但音频在正常播放。

**修复建议**：两侧都改为 `total > 0` 时才做 min 钳制。

---

### 8. `loadBackendLibrary` 合并曲库后 `currentTrackIndex` 不重映射
**文件**：`src/store/player.ts:908-927`

启动 rehydrate 时恢复了持久化的 `currentTrackIndex`，随后 `loadBackendLibrary()` 用 `mergeTracksByPath` + `dedupeTracksWithLiked` 生成的新 playlist 顺序/长度都可能与持久化时不同（去重、新增、排序变化），但 index 原样保留。

**后果**：重启后"当前曲目"可能指向另一首歌；极端情况下（去重缩短列表）index 越界 → `currentTrack()` 返回 null。

**修复建议**：合并前记录当前曲目 `id`，合并后用 `findIndex(t => t.id === prevId)` 重定位，找不到再回退 0。

---

## 🟡 低严重度

### 9. LRC `[offset:]` 标签符号与通用约定相反
**文件**：`src-tauri/src/ipc/library.rs:1941-1954`、测试 `library.rs:2586-2591`

实现是 `time + offset`（正 offset 让歌词**变晚**）。LRC 通行约定是正 offset 让歌词**提前**（time - offset）。带 offset 标签的歌词文件会偏移 2×offset。

### 10. ffmpeg 解码器近距 seek 时帧计数按 chunk 截断，多声道时间戳漂移
**文件**：`crates/seraph-decoder/src/ffmpeg.rs:258-268`

跳读循环里 `self.frames_read += (n / (channels * 4))` 对每个 read 返回值独立整除。声道数不能整除 8192 时（如 5.1 声道 frame_bytes=24），每个 chunk 丢弃余数，时间戳累计偏小。应累计字节数后一次性换算。

### 11. B 站 cookie 过期时间不持久化，重启后过期 cookie "复活"
**文件**：`src-tauri/src/ipc/bilibili.rs:222-231`（`BilibiliSessionFile` 无 `cookie_expires` 字段）、`bilibili.rs:1668-1694`

`merge_set_cookie_headers` 精心维护的 `cookie_expires` 从不写入磁盘；重启加载后所有 cookie 都按"永不过期的 session cookie"处理，已过期凭证会继续被发送。

### 12. 头像下载先全量读入内存再检查大小上限
**文件**：`src-tauri/src/ipc/bilibili.rs:899-904`

`response.bytes()` 无上限读取后才比较 `MAX_AVATAR_BYTES`，与同文件 `read_bytes_capped` 的防御姿态不一致。重定向到大文件时白白吃内存。

### 13. B 站音频整体读入内存（上限 1.5 GB）后才落盘
**文件**：`src-tauri/src/ipc/bilibili.rs:825-857`、`bilibili.rs:925-945`

大体积 FLAC（数百 MB）会整块驻留内存，且 `write_audio_file` 再复制一次。建议流式写入 `.download` 临时文件。

### 14. 目录导入递归无 symlink/junction 环路保护
**文件**：`src-tauri/src/ipc/library.rs:702-726`（`collect_audio_files`）

Windows 目录联接（junction）指向祖先目录时无限递归直至栈溢出/卡死。建议跟踪 canonicalize 后的已访问目录集合或限制递归深度。

### 15. 可视化 FFT 每个解码包都全量计算，结果大多被丢弃
**文件**：`crates/seraph-visualizer/src/fft.rs:84-126`、`crates/seraph-audio/src/engine.rs:1039-1057`

`publish_spectrum_if_due` 每包都调用 `push_samples`（内部跑 1024 点 FFT），但只每 66ms 取一次结果。Hi-Res（384k/768k）下解码包频率很高，大量 FFT 是纯浪费。另：`Spectrum` 事件每 66ms 经 IPC 推到前端，但前端 `usePlayback` 根本没有处理 `spectrum` 类型 —— 纯无效流量。

### 16. Notification 退场动画永远不可见
**文件**：`src/App.tsx:83-87`（`hasNotification && <LazyNotification/>`）、`src/components/modal/Notification.tsx:17-18`

组件内部设计了 2700ms 隐藏（滑出动画）+ 3200ms dismiss 的两段式退场，但 App 在 `notification` 置 null 的瞬间就卸载了整个组件，`translate-x-[120%]` 过渡从未播放，通知是"瞬间消失"的。

### 17. WASAPI 独占模式按设备名匹配，同名设备可能选错
**文件**：`crates/seraph-audio/src/engine.rs:707-712`、对比 `crates/seraph-audio/src/device.rs:81-106`

设备枚举端用 name-hash + `-N` 后缀区分同名设备，但独占渲染 worker 用 `get_device_with_name(&device_name)` 仅按名字匹配，两个同名 DAC 时可能打开错误实例。

### 18. ImportedTrack 元数据探测对每个文件都做磁盘 IO + 进程探测，导入大目录时阻塞 IPC
**文件**：`src-tauri/src/ipc/library.rs:141-157`（`import_tracks` 为同步命令）

每个文件要开文件读 magic、lofty 解析、必要时 `probe_stream_info`（可能拉起 ffprobe 子进程）。同步命令在 Tauri 调度线程上执行，导入上千文件的目录期间其他 IPC（含播放控制）会排队。建议改 `async` + `spawn_blocking`，并考虑分批返回进度。

---

## 备注（非 bug，但值得留意）

- `src-tauri/src/ipc/playback.rs:60-68`：`next_track` / `prev_track` 是空实现，切歌完全靠前端再发 `play`，前端 `sendCommand("next_track")` 是无效调用（`src/store/player.ts:748,763`）。
- `src-tauri/src/state.rs` 的 `player_state` 只写不读，是死状态。
- `crates/seraph-audio/src/wasapi.rs` / `backend.rs` 的 `AudioBackend` trait 全部 `NotImplemented`，为占位骨架。
- `seraph-visualizer` 的 `normalize_bins` 做逐帧归一化，安静段落会把底噪放大成满格频谱（视觉效果问题）。
