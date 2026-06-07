# Seraph Audio Player — 代码深度审计（BUG 与逻辑性问题）

> 审计范围：`src-tauri/`、`crates/seraph-*`、`src/`（store/hooks/components）。
> 等级说明：**严重** 可能导致用户数据丢失/崩溃/安全风险；**高** 影响核心功能；**中** 边界条件触发；**低** 性能/体验/代码质量。

---

## 🔴 严重（Critical）

### C-1 缓存目录可指向用户音乐文件夹 → 自动清理会删除用户文件
- 位置：`src-tauri/src/ipc/cache.rs:300-374`
- 现象：`validate_cache_dir` 只拒绝空字符串和磁盘根目录；`is_managed_cache_file` 仅按扩展名（`m4a/flac/opus/aac/mp3/download/tmp`）白名单。
- 触发：用户在「缓存路径」里选择自己平时存放本地音乐的目录（例如 `D:\Music`），随后 `clear_cache` 或 `enforce_cache_limit`（自动清理开启时）会扫描整个目录树，按 mtime 删除符合扩展名的文件。
- 影响：用户本地 FLAC/MP3 等无损/有损音乐被无声删除，**不可恢复**。
- 修复建议：
  1. 在 `ensure_cache_dir` 写入 `.seraph-cache` 标记文件，**只对包含该标记的目录执行清理/删除**；现在 marker 只是写入但不参与校验。
  2. `validate_cache_dir` 增加：父目录是 AppData / 临时目录 / 用户显式确认过的白名单。
  3. 切换缓存目录后立刻校验旧目录是否仍有 marker，否则跳过迁移/清理。

### C-2 重采样全部失败时 fallback 返回未重采样的样本 → 严重音调失真
- 位置：`crates/seraph-audio/src/engine.rs:1171-1194 adapt_samples`
- 现象：当 `resample_interleaved_sinc` 失败时退到 `resample_interleaved_linear`，两者再都失败时 `return remapped`（仍是输入采样率的样本），但下游按 `output_rate` 喂给 WASAPI/cpal。
- 触发：理论上仅在 sample/channel/rate 不合法时；但只要触发，比如某些边角格式或异常包，会以**变速/变调**播放，且 RingBuffer 与 `frame_position` 错位累积。
- 影响：长时间播放失真，进度条与实际播放不同步。
- 修复建议：兜底失败时返回 `Err`，由 worker `publish(Error)` + `PlaybackStopped`，前端有明确提示。

### C-3 `mark_tracks_cache_missing_by_paths` 对 Bilibili 缓存永远不命中
- 位置：`src-tauri/src/ipc/library.rs:753-777`
- 现象：构建 `removed = removed_paths.iter().map(import_dedupe_key)`（→ 形如 `d:\cache\bvxx-cid.flac`），再用 `removed.contains(&import_track_key(&track))` 判断。但 `import_track_key`（同文件 922 行）对带 `source_id` 的 Bilibili 曲目返回 `source-id:bvxx`，对 `source_url` 的返回 `source-url:...`。两者形如完全不同的 key，**永不相等**。
- 触发：在「设置」里点「清理缓存」，或自动清理触发后。
- 影响：Bilibili 流媒体缓存被删但 `cache_missing` 标志不会被置上 → 列表里仍显示「正常」，点击播放才真正失败。`ensurePlayableTrack` 走不到重新缓存分支。
- 修复建议：两边用同一 key 体系——removed_paths 端额外算 `source-id:` 形式的 key（从文件名提取 BVID），或扫描 cache 时同时返回 BVID/CID 元信息。

### C-4 整份 `playlist` 进入 localStorage 持久化 → 易爆 QuotaExceededError
- 位置：`src/store/player.ts:1286-1300 partialize`、`createPlayerPersistStorage`
- 现象：`playlist` 完整序列化写 `localStorage`（5–10 MB 限额）。`setItem` 直接调用，未捕获 `QuotaExceededError`。每次播放、导入、点赞等都触发整体 setItem。
- 触发：导入数百首 Bilibili 视频（含 Bilibili 封面 URL + 元数据，每首 ~1KB），或本地大库扫描后。
- 影响：写入失败抛出未捕获异常，可能让 hydration 状态损坏；旧版本浏览器内存压力大。
- 修复建议：
  - `playlist` 不进持久化，启动时由后端 `get_playlist` 重建（Rust 端已有 `library-cache.json`）。
  - 至少包一层 try/catch，setItem 失败时降级到 IndexedDB 或仅持久关键状态。

---

## 🟠 高（High）

### H-1 设备 ID 用枚举顺序生成 → 设备插拔后 ID 漂移
- 位置：`crates/seraph-audio/src/device.rs:142-144 device_id_for`
- 现象：`format!("cpal:{index}:{}", sanitize_device_id(name))`，`index` 是 cpal `output_devices()` 的迭代序号。
- 触发：拔插 USB DAC / 蓝牙耳机 / 系统增删音频设备。
- 影响：上次记住的 device_id（前端 `currentDeviceId` 进 persist）下次找不到匹配，`output_device_by_id` 返回 `DeviceNotFound`；选择设备失败。前端的 fallback 是「找到默认」，但用户原选的设备会静默切换。
- 修复建议：ID 用 `sanitize_device_id(name)` 单独作为稳定主键，或拿设备的 endpoint id（Windows 上 `IMMDevice::GetId`）作为持久化 key。

### H-2 WASAPI 渲染 worker 启动握手 3 秒超时假阳性
- 位置：`crates/seraph-audio/src/engine.rs:597-619 spawn_wasapi_exclusive_render_worker`
- 现象：用 `mpsc::channel + recv_timeout(3s)`：worker 内部 `run_wasapi_exclusive_render_worker` 完成 `initialize_client` + `start_stream` 后才会发送 ready。3 秒内未收到就直接 `Ok(worker)`，认为启动成功。
- 触发：慢速 DAC / 独占模式协商较久的高采样率（DSD-PCM 转换 + 768kHz）。
- 影响：`PlaybackEngine::play_file` 返回 Ok 并把 session 存上、发 `PlaybackStarted`；但 worker 实际还在 init 或正在失败，事件流出现「先 Started 再 Error+Stopped」的乱序，UI 闪烁。
- 修复建议：要么阻塞等待真实 ready，要么把 ready 作为 mandatory，超时即认为失败并 join worker。

### H-3 same-track resume 时若上次因错误自停 → UI 显示在播但无声
- 位置：`crates/seraph-audio/src/engine.rs:211-221`
- 现象：`play_file` 检查 path/track_id 相同且 `!stopped`；但若上一次播放报错后 `shared.stopped = true`，分支不进入；走的是新建 session 路径。但若上一次只是 `paused` 而后台 worker 已退出（如 decoder EOF 后等 drain 阶段被强 stop），`stopped` 可能未置 true。
- 触发：网络流卡顿 → decoder 报错 publish 但 session 未被显式 stop_session。
- 影响：紧接着触发 resume 会调用 `seek + resume`，但 worker thread 已 join → 没有线程消费 ring，播放假在播。
- 修复建议：resume 前检查 worker 是否仍在跑（保留一个 `worker_alive` AtomicBool），否则强制走重新打开路径。

### H-4 `play_file` 解码失败时旧 session 未停止
- 位置：`crates/seraph-audio/src/engine.rs:222-230`
- 现象：`open_decoder(&path)?` 在 `self.stop_session()` 之前。`?` 短路返回，则当前 session 仍在播放，但前端已发命令以为切歌。
- 触发：用户切到一个损坏 / 不支持的文件。
- 影响：状态机错位：前端 `loadTrack` 已更新 `currentTrackIndex`，但实际播的还是上一首；用户看到「下一首」标题已变，听到的是上一首。
- 修复建议：错误时显式 `event_bus.publish(Error{...})`，或先 stop_session 再 open_decoder。

### H-5 Bilibili 下载/页面抓取无响应大小限制
- 位置：`src-tauri/src/ipc/bilibili.rs:474-491` （HTML 抓取）、`784-805` （音频流）
- 现象：`response.bytes().await` 把响应一次性读入 `Vec<u8>`，没有 `content_length` 检查，也没有 chunk 限流。
- 触发：恶意/异常服务器返回超大响应；或用户粘贴非 Bilibili 域名链接、跳到大文件。
- 影响：内存峰值可达数 GB → OOM、UI 卡死。
- 修复建议：
  - HTML 解析仅需抓取头部 256 KB 找 BVID；用 `bytes_stream()` 截断。
  - 音频流逐 chunk 写盘，限制总大小（例如 < 1.5 GB），超出即 abort。

### H-6 `ensure_audio_file` 重命名失败/中断时可能留下 0 字节文件
- 位置：`src-tauri/src/ipc/bilibili.rs:853-873 write_audio_file`
- 现象：`fs::write(&temp_path, bytes)` 后 `fs::rename(&temp_path, path)`，rename 失败时 `temp_path` 留盘。下次重试 `ensure_audio_file` 见到 `path` 不存在或长度 0 会重下载，但 `.download` 临时文件会成孤儿堆积。
- 影响：缓存目录被垃圾文件污染，`is_managed_cache_file` 把 `.download` 计入用量。
- 修复建议：startup / 任意 cache 操作时清理孤儿 `.download` / `.tmp`。

---

## 🟡 中（Medium）

### M-1 `decode_worker` 在 paused + EOF 时空转
- 位置：`crates/seraph-audio/src/engine.rs:942-951`
- 现象：decoder EOF 后进入 drain 阶段：`while !stopped && producer.slots() < max_buffer_samples { sleep(5ms) }`。若此时正好 `paused = true`，render 不消费，producer.slots 不变 → 永远不退出，直到 stop。
- 影响：线程占用直到用户显式 stop / 切歌；CPU 低，但 `shared` 的 Arc 计数不释放。
- 修复建议：drain 循环增加 paused 检查，paused 时直接 break。

### M-2 `select_audio_stream` 选规则反直觉
- 位置：`src-tauri/src/ipc/bilibili.rs:556-595`
- 现象：先按用户偏好把 Atmos、FLAC、普通流全装入候选，最后用 `max_by_key((kind_rank, bandwidth))` 选。问题是即使用户关闭 `prefer_dolby_atmos`，仍可能选到 Dolby（因为 Normal 也按 bandwidth 比较，但 Dolby 已经不进候选了）。但若用户关闭 `prefer_flac` 同时关闭 `prefer_dolby_atmos`，只剩 Normal——这是预期。问题在「FLAC 被选中但 ffmpeg 不可用」时 `output_extension` 仍是 `m4a`（见 1483-1488）——文件名带 `.m4a` 但里面是 FLAC 原始流，元数据探测会失败 → bitdepth/采样率显示「Unknown」。
- 影响：UI 标签和真实格式不符；后续解码可能走 FFmpeg fallback，慢。
- 修复建议：未 remux 的 FLAC 直接落 `.flac` 扩展（或落 `.fmp4`），并在元数据栏标记 stream 类型。

### M-3 `apply_online_lyrics` / `save_track_lyrics` 缺大小校验
- 位置：`src-tauri/src/ipc/library.rs:160-189, 212-236`
- 现象：前端 `MAX_LYRIC_FILE_BYTES = 2MB` 限制，但 Rust 端没限。任意通过 IPC 直接调用（或前端被改）都能传巨型 `lyrics_bytes`。
- 影响：内存峰值；恶意 zlib bomb 在 QRC/KRC 解密路径展开 → 解压无上限。
- 修复建议：服务端二次校验 `lyrics_bytes.len() <= 4*1024*1024`；`inflate_zlib_utf8` 用 `take(N)` 限制解压上限。

### M-4 `cookie_header()` 不区分过期 cookie
- 位置：`src-tauri/src/ipc/bilibili.rs:1239-1257 merge_set_cookie_headers` + `1492-1504`
- 现象：只解析 `name=value`，忽略 `expires`/`max-age`。一旦写入 `BTreeMap` 永不清理。
- 触发：长期使用后 cookie 可能持有过期 token，登录态间歇失效又被「恢复」。
- 影响：登录态校验偶发异常，难以诊断。
- 修复建议：解析 `expires`/`max-age` 并在 cookie_header 输出前剔除过期项。

### M-5 设备 `is_default` 判定脆弱
- 位置：`crates/seraph-audio/src/device.rs:38-53`
- 现象：先单独取 `host.default_output_device().and_then(|d| d.name())`，再在枚举里按 name 字符串相等比较。多个设备同名（笔记本扬声器 + USB 同名 DAC）时全都标 default。
- 影响：UI 误标 default；保存设备 id 也可能错。
- 修复建议：在 cpal 上比较 device 句柄（cpal 没暴露稳定 id，可在 Windows 上接 wasapi crate 取 endpoint id）。

### M-6 `ensure_audio_file` 已存在判定过宽
- 位置：`src-tauri/src/ipc/bilibili.rs:757-762`
- 现象：`path.is_file() && len > 0` 即认为可用。但 ffmpeg remux 半途崩溃留下的非法 FLAC（>0 bytes）会被复用。
- 修复建议：写文件结束后落一个 `.ok` sentinel，或在 path 同名记 sha256，启动时校验。

### M-7 前端 `play` 命令链时序：driver 切换可能打断当前播放
- 位置：`src/store/player.ts:210-228 applyOutputConfiguration` + `sendPlayCommand`
- 现象：每次 `sendPlayCommand` 都先 `set_output_driver`、再 `select_output_device`，最后 `play`。如果 driver 实际未变（Rust 端 `if self.driver == next { return }`）尚 OK；但 `select_output_device` 在已选中相同 device 时也直接 return，不会重启 session。问题在 `play_file` 内分支：若 path/track_id 一致 → resume；driver 不变 → 不重建 session。**但** 如果上一首失败后 path 已变更，再次「同曲恢复」可能错误地走 resume 路径。
- 影响：用户切设备/驱动后偶发音轨没切换。
- 修复建议：保持当前结构，但在前端切换 driver 后强制 stop 再 play。

### M-8 `nextShuffleTrackIndex` 用伪随机 hash → 不随机
- 位置：`src/store/player.ts:149-167`
- 现象：`hashString(currentId:recent:length) % pool.length`。同样 `currentId + recent` 永远选同一首。
- 影响：「随机播放」在序列稳定时退化为周期播放。
- 修复建议：用 `Math.random()` 或 crypto.getRandomValues。

### M-9 `useFileDropImport` 注册全局 `dragover/drop` preventDefault → 拖入文本/链接受影响
- 位置：`src/hooks/useFileDropImport.ts:33-39`
- 现象：全局阻止 default，意味着任意输入框拖入文本也无法落 default。
- 影响：输入框拖拽体验丢失（用户拖一段文本到输入框无法插入）。
- 修复建议：只阻止 window 顶层；在 `<input>` 上 stop propagation。

### M-10 `usePlayback` progress 事件 trackId 不匹配时直接吞掉
- 位置：`src/hooks/usePlayback.ts:14-29`
- 现象：返回 `{}` 让 zustand 不更新；但用户在 loadTrack 后下一首 Progress 早于 PlaybackStarted 到达时，UI 卡在上一首时间。
- 影响：偶发进度跳动。
- 修复建议：未匹配时仍重置 currentTime=0 直到 PlaybackStarted。

### M-11 `WaveformProgress` 进度条 seek 精度受 mock baseline 影响
- 位置：`src/hooks/useWaveform.ts`
- 现象：波形 baseline 用 trackId hash 生成，与真实音频无关。只是装饰。OK 但与「在播位置」不对应，用户误解。
- 影响：信任度低；非 bug。
- 修复建议：长期接入真实 PCM peak data；短期加 "Waveform preview" label。

### M-12 `cancel/queueVolumeCommand` 用全局可变量，跨实例污染
- 位置：`src/store/player.ts:122-124, 519-546`
- 现象：`volumeCommandTimer`、`pendingVolumeCommand` 是模块级变量。HMR / 多 store 实例时残留 timer。
- 影响：开发环境偶现重复 set_volume；生产单实例无影响。
- 修复建议：放进 store 内部 `getState()` ref，或绑到 store API。

---

## 🟢 低（Low / 性能 / 体验）

### L-1 `seraph-visualizer::spectrum_bins` 是 O(N²) DFT，注释自称 FFT
- 位置：`crates/seraph-visualizer/src/fft.rs:146-164`
- 影响：1024 × 32 ≈ 32K sin/cos 每帧 (~66ms)，单核 ~5–10% CPU。
- 建议：替换为真正 FFT（`rustfft` / `realfft`）。

### L-2 `audio_format_from_magic` 多次重复打开同一文件
- 位置：`src-tauri/src/ipc/library.rs:949-973, 1225-1234, 1107-1150`
- 影响：导入时一首歌可能被 open 3+ 次（is_audio_file + audio_format_label + is_dsd_file + parse_audio_metadata）。
- 建议：在 `track_from_path` 入口探测一次 magic + extension，沿用结果。

### L-3 `FfmpegDecoder::seek` 每次 spawn 新进程
- 位置：`crates/seraph-decoder/src/ffmpeg.rs:155-157`
- 影响：拖动进度条频繁 seek，启动新 ffmpeg 进程 ~50–100ms 延迟。
- 建议：保留 ffmpeg 进程，通过 `stdin` 控制；或合并近距 seek。

### L-4 `engine.rs` ring buffer 用 `QueuedSample{generation,value}` 浪费 4× 内存
- 位置：`crates/seraph-audio/src/engine.rs:163-167`
- 现象：每个 f32 样本带一个 u64 generation，对齐后 16 字节/样本（vs. 4 字节）。
- 影响：192kHz × 2 × 3s × 16B ≈ 18 MB；768kHz × 2 × 3s × 16B ≈ 72 MB。
- 建议：generation 放 ring 外的 stamp（每个 batch 一个 generation），或用 packetized ring。

### L-5 远程歌词 dedupe 用 first-line + title 字符串拼 key，弱
- 位置：`src-tauri/src/ipc/library.rs:600-629 dedupe_online_lyrics_candidates`
- 影响：同首歌不同来源若首行翻译不同，被当成两个候选。
- 建议：用 N-gram 或时长 + 全文哈希。

### L-6 `SymphoniaDecoder::next_packet` Re-allocates `SampleBuffer` 每次
- 位置：`crates/seraph-decoder/src/symphonia.rs:160-166`
- 影响：每个 packet 分配一次。
- 建议：复用 SampleBuffer（state 里缓存 capacity）。

### L-7 `device_id_for` 用 lower-case kebab，可能撞名
- 位置：`crates/seraph-audio/src/device.rs:142-156`
- 现象：「Speakers (Realtek(R) Audio)」与「Speakers (Realtek Audio)」清理后撞名（去括号？看代码：非 alphanumeric 替成 `-`，所以括号会被替；可能撞）。
- 影响：选错设备。
- 建议：见 H-1，结合 endpoint id 解决。

### L-8 `useFluentHover` 未审计但默认导出
- 位置：`src/hooks/useFluentHover.ts`（未读取）。
- 建议：抽查；若未被组件使用，删除。

### L-9 `MainPages.tsx StreamingPage` 二维码轮询频率 1.8s，B 站 rate-limit 风险
- 影响：长期挂着登录窗口可能被风控。
- 建议：拉长到 3–5s；登录成功后立即取消。

### L-10 `LyricsPanel` 自动滚动 `scrollTo({behavior: smooth})` 高频触发
- 位置：`src/components/sidebar/LyricsPanel.tsx:124-135`
- 影响：每秒切换 activeIdx 可能多次触发 smooth 滚动，互相打断。
- 建议：节流到 200ms；或仅当切换 group 时滚动。

### L-11 `WaveformProgress` baseline 仅 `track-1` 区分
- 位置：`src/hooks/useWaveform.ts:13`
- 现象：`seed = trackId === "track-1" ? 1.0 : 1.8`，几乎所有曲目共用同一种形状。
- 建议：基于完整 trackId hash 生成多样化 seed。

### L-12 `restrict_session_file_permissions` Windows 路径未处理 `USERNAME` 含特殊字符
- 位置：`src-tauri/src/ipc/bilibili.rs:1210-1234`
- 现象：`%USERNAME%` 字面回退，且 user 名含空格/中文时 icacls 参数解析可能出错。
- 建议：用 `whoami` crate 拿规范用户 SID。

### L-13 RingBuffer drop 缓冲未 publish PlaybackEnded
- 位置：`crates/seraph-audio/src/engine.rs:953`
- 现象：drain 期间被 stop_session() 打断后，外层 PlaybackEnded 由后续 `if !stopped` 分支决定；正确路径 OK。但若 join 时 stop 信号先到，Ended 事件不会发送。
- 建议：把 Ended 事件抽到 stop_session 外层，明确「自然结束」与「强制停止」。

### L-14 `dedupeTracks` / `dedupeTracksWithLiked` 重复迭代
- 位置：`src/store/player.ts:294-332`
- 影响：N×log(N) 之上叠加多次 map 构造，导入大量曲目时 ~ms 级延迟。
- 建议：合并为一次迭代。

### L-15 Notification 计数器 `notificationCounter` 是模块级，HMR 重置后 id 撞
- 位置：`src/store/player.ts:122 + 1272-1275`
- 影响：开发环境偶现通知不重渲。
- 建议：与 store reducer 内部 state 绑定。

### L-16 `SimpleVisualizer` `mono_buffer.lock()` 高频抢锁
- 位置：`crates/seraph-visualizer/src/fft.rs:76-101`
- 现象：每个 sample 都做 lock。
- 建议：批量化加锁，或 SPSC ring。

---

## 总览

| 等级 | 数量 |
|------|------|
| 严重 (Critical) | 4 |
| 高 (High) | 6 |
| 中 (Medium) | 12 |
| 低 (Low) | 16 |

**优先修复顺序建议：**
1. C-1（数据丢失）→ C-3（功能错位）→ C-2（音质）→ C-4（持久化）
2. H-5（OOM 安全）→ H-1（设备稳定性）→ H-2/H-3/H-4（播放健壮性）
3. M-3/M-4（安全/稳定）→ 其他 M
4. L 等级可一并随重构推进
