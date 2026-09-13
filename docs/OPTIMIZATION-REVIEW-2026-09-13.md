# 项目优化审查报告（2026-09-13）

当前最值得投入的是**大曲库的数据读写与队列同步、可视化计算调度、搜索排序和资源体积**。项目已经具备较完整的稳定性防护与回归测试，下一轮可以围绕这些具体开销逐项优化。

> 后续修复已开始落实，代码变化、复测数据和剩余事项见 [优化修复记录](./OPTIMIZATION-FIXES-2026-09-13.md)。下文保留审查时的原始基线。

审查基线：`v0.5.11`，Git 提交 `d52298e`。本次仅新增审查文档，未修改业务代码。已对照 [上次缺陷修复记录](./BUG-FIXES-2026-09-12.md)，其中 BUG-01～12 不作为未修复问题重复列出。

## 1. 范围与验证结果

检查范围包括 React/Zustand 状态、页面渲染、Tauri IPC、Rust 曲库存储、音频输出、声学分析、流媒体下载及 CI/发布配置。

本机环境：Windows，Node `v24.14.0`，Rust/Cargo `1.98.1`，使用现有依赖与构建缓存。

| 检查 | 本次结果 |
| --- | --- |
| `npm test -- --reporter=dot` | 21 个测试文件、187 项通过 |
| `cargo test --workspace --offline --locked --quiet` | 268 项通过：audio 32、decoder 32、dsp 32、tauri 156、visualizer 16 |
| `npm run build` | 通过，包含 TypeScript 检查与 Vite 生产构建 |
| `cargo clippy --workspace --all-targets --offline --locked -- -D warnings` | 通过 |
| `cargo fmt --all --check` | 通过 |
| 前端合成曲库微基准 | 已执行，见第 3 节及附录 A |
| Rust 响度历史快照微基准 | 已执行，见第 3 节及附录 B |

测试合计 **455 项通过**。前端测试仍输出 EQ 存储不可用提示及模拟 403 的错误日志；Rust 测试有 Windows 链接器输出提示，均未使检查失败。

**证据口径：**下文的调用关系、全量复制、同步计算、资源大小为已确认事实；性能影响结合合成微基准判断。没有启动真实 Tauri 窗口进行性能采样，也没有测量声卡输出、USB DAC、安装包启动耗时或长时间实际播放。本次未重新进行在线依赖漏洞审计或安装器构建。

## 2. 优先级总览

这里的 P1/P2/P3 表示优化顺序，不等同于缺陷严重级别。P1 建议优先安排；P2 随下一轮迭代推进；P3 结合相关功能改动逐步完成。工作量“小/中/大”是相对估计。

| 编号 | 优先级 | 优化方向 | 主要收益 | 工作量 |
| --- | --- | --- | --- | --- |
| OPT-01 | P1 | 队列按版本同步，切歌发送增量状态 | 降低大曲库切歌的 IPC 与序列化开销 | 中～大 |
| OPT-02 | P1 | 单曲查询建立索引，列表与歌词分开读取 | 减少全库深拷贝、启动传输与内存分配 | 中 |
| OPT-03 | P1 | 曲库按变更写入，并补齐跨文件提交与恢复 | 减少写放大，改善异常中断后的数据一致性 | 中～大 |
| OPT-04 | P1 | 可视化按需计算，工作线程产出快照 | 降低普通播放时的 CPU 与命令处理开销 | 中 |
| OPT-05 | P2 | 可视化轮询限制在途请求，补齐生命周期管理 | 避免慢响应堆积、过期帧与隐藏窗口空转 | 小～中 |
| OPT-06 | P2 | 响度统计按数据更新频率缓存结果 | 降低长会话重复扫描、排序与分配 | 小～中 |
| OPT-07 | P2 | 曲库快速展示，后台补扫与增量导入 | 缩短可用等待时间，改善批量导入反馈 | 中 |
| OPT-08 | P2 | 搜索复用排序索引，大计算移出输入路径 | 改善万首曲库输入响应 | 中 |
| OPT-09 | P2 | 专辑/艺术家网格虚拟化与封面缩略图 | 限制 DOM、图片解码和内存占用 | 中 |
| OPT-10 | P2 | 字体格式精简，重页面按需加载 | 减小前端资源与首次解析体积 | 小～中 |
| OPT-11 | P2 | 下载取消贯穿当前文件，磁盘写入移出 async 执行线程 | 改善弱网、慢磁盘下的操作响应 | 中 |
| OPT-12 | P2 | 持久化防抖覆盖 JSON 序列化 | 降低拖动音量等操作的重复编码开销 | 小 |
| OPT-13 | P2 | 发布版诊断日志与音频性能计数 | 让实际卡顿、断音和初始化失败可定位 | 中 |
| OPT-14 | P2 | IPC 契约与错误类型统一 | 降低跨语言维护和异常路径排查成本 | 中 |
| OPT-15 | P2 | 固定验证环境，补性能与桌面验收 | 防止优化回退与环境漂移 | 中 |
| OPT-16 | P3 | 继续按职责拆分热点模块 | 降低后续功能修改与回归成本 | 分批进行 |

## 3. 实测基线

### 3.1 前端资源体积

以下统计来自本次 `npm run build` 生成的 `dist/assets`，单位为十进制，属于未压缩资源大小。

| 项目 | 大小/数量 |
| --- | --- |
| 主入口 JavaScript | 353.97 kB |
| 公共 JavaScript 块 | 167.68 kB |
| 全部 JavaScript | 594.28 kB，13 个文件 |
| 全部 CSS | 281.50 kB |
| WOFF 字体 | 6.216 MB，198 个文件 |
| WOFF2 字体 | 4.891 MB，198 个文件 |
| 全部字体 | **11.108 MB，占 assets 约 92.7%** |
| 全部 assets | 11.983 MB |

字体切片会按字符使用情况加载，**资源目录大小不等于启动时全部加载量，也不等于安装器可节省的体积**。但同时打包 WOFF/WOFF2 的确留下了明显的资源精简空间。

### 3.2 合成曲库：排序与队列请求

直接转译并调用当前 `filterAndSortTracks`、`playbackQueueArgs`；使用固定随机种子生成曲目，不读取用户音乐。预热 5 次，采样 25 次。标题排序采用命中全部曲目的搜索词，以观察大结果集的开销。

| 曲目数 | 默认顺序过滤 P95 | 过滤＋标题排序 P50 / P95 | 队列映射＋JSON 编码 P50 / P95 | 队列 JSON UTF-8 大小 |
| --- | --- | --- | --- | --- |
| 1,000 | 0.11 ms | 2.12 / 2.49 ms | 0.50 / 0.64 ms | 0.205 MB |
| 10,000 | 0.88 ms | 27.73 / 31.72 ms | 4.19 / 5.26 ms | 2.083 MB |
| 50,000 | 4.47 ms | **189.66 / 196.79 ms** | **25.28 / 29.72 ms** | **10.544 MB** |

这是 Node 微基准，未包含 React 渲染、WebView IPC、Rust 反序列化和队列更新。结果说明：当前测试数据下，排序与全量队列同步比单纯过滤更值得优先优化；不能将这些数值直接当作用户设备上的交互延迟。

### 3.3 长历史声学快照

将当前 `analysis.rs` 复制到临时构建目录，附加微基准入口，使用 `rustc -O` 编译；填入与相应时长对应的合成响度历史，直接调用原有 `snapshot()`。预热 10 次，采样 100 次。

| 合成历史对应时长 | 门限块 / LRA 样本数 | 快照 P50 / P95 |
| --- | --- | --- |
| 60 秒 | 600 / 60 | 0.0030 / 0.0031 ms |
| 1 小时 | 36,000 / 3,600 | 0.2097 / 0.2888 ms |
| 20,000 秒，约 5.56 小时 | 200,000 / 20,000 | 1.4422 / 1.6357 ms |

该基准没有输入真实音频，不包含 PCM 分析、FFT、IPC 或有数据的波形输出。结果支持缓存慢变统计值，但单次快照尚未接近 33 ms 的轮询间隔，因此 OPT-06 排在按需计算与请求调度之后。

## 4. 详细建议

### OPT-01：将播放队列同步与当前播放状态分开

**位置：**[queueSync.ts:26](../src/store/player/queueSync.ts#L26)、[usePlayback.ts:124](../src/hooks/usePlayback.ts#L124)、[playbackActions.ts:264](../src/store/player/playbackActions.ts#L264)、[state.rs:245](../src-tauri/src/state.rs#L245)。

**现状：**`playbackQueueArgs()` 每次映射全部曲目，携带路径、标题、艺术家、专辑、封面和时长。`usePlayback` 在曲目索引、历史和模式变化时同步；“上一首/下一首”动作也先等待一次全量同步。后端随后比较全量 ID、替换队列并整理历史。已有过期响应保护，但没有消除重复工作。

**建议：**队列内容使用 `queueRevision`；导入、删除、排序等结构变更才提交完整队列或差量。选曲、模式、历史和元数据更新走独立的小请求。版本不一致时保留一次全量恢复路径。复用已有 `set_playback_modes`，合并动作与 effect 的重复同步。

**验收：**在 1 万/5 万首曲库中，仅切歌或切模式不再提交整库；记录每次操作的请求数、字节数和 P95。保留随机预选一致性、连续历史回退、系统媒体键、快速切歌及删除竞态回归。

### OPT-02：单曲查询只取单曲，列表数据按用途裁剪

**位置：**[commands.rs:35](../src-tauri/src/ipc/library/commands.rs#L35)、[media_library.rs:77](../src-tauri/src/ipc/library/media_library.rs#L77)、[types.rs:14](../src-tauri/src/ipc/library/types.rs#L14)、[TaskbarLyricsBar.tsx:225](../src/taskbar/TaskbarLyricsBar.tsx#L225)。

**现状：**`get_track_info` 为找一首歌，先调用 `read_cached_tracks` 克隆整个 `Vec<ImportedTrack>`，再线性查找。内存快照包含各曲目的歌词，因此命中内存也有全库深拷贝成本。任务栏歌词条切歌会调用此命令；`get_playlist` 同样一次返回含歌词的全部记录。

**建议：**维护 `trackId → track` 索引和独立顺序表；单曲读取只克隆命中项。列表使用轻量摘要 DTO，歌词在当前曲目需要时按 ID/版本获取。大曲库按页或批次载入；`Arc` 快照可减少 Rust 内部复制，但 IPC 序列化仍需单独优化。

**验收：**1 万/5 万首、不同歌词体积下测量单曲查询耗时和分配量；查询成本不应随整库歌词总量增长。任务栏歌词更新、歌词替换及封面显示行为保持一致。

### OPT-03：减少曲库写放大，补齐数据提交与恢复

**位置：**[media_library.rs:155](../src-tauri/src/ipc/library/media_library.rs#L155)、[media_library.rs:187](../src-tauri/src/ipc/library/media_library.rs#L187)、[media_library.rs:115](../src-tauri/src/ipc/library/media_library.rs#L115)、[commands.rs:190](../src-tauri/src/ipc/library/commands.rs#L190)。

**现状：**每次保存都拆分并克隆曲库，编码完整主文件，再编码和写入全部歌词边车，最后再克隆完整内存快照。仅删除一首歌或修改一首歌词也走该路径。第 167 行关于歌词不反复序列化的注释与第 176 行的实际行为不一致。

主文件和歌词边车分别使用原子替换，但两次替换没有共同提交点；如果主文件成功、边车失败或中途进程退出，两者可能属于不同版本。边车读取/解析失败又会被视为空歌词表。这是由异常路径确认的恢复风险，本次没有模拟磁盘故障。

**建议：**先按元数据/歌词变更区分写入，合并批量更新；边车损坏时备份并报告。随后选择带版本清单的成组快照提交，或将曲目、歌词、歌单迁入 SQLite 事务。迁移应保留旧格式读取与可恢复备份。

**验收：**仅改封面时不重编码全部歌词；修改单曲歌词的写入量可追踪。通过故障注入覆盖第二个文件写失败、进程中断、损坏边车和旧格式迁移，确保重启后读取到一致且可解释的状态。

### OPT-04：可视化由订阅需求驱动，IPC 读取最新结果

**位置：**[visualizer.rs:87](../src-tauri/src/ipc/visualizer.rs#L87)、[visualizer.rs:121](../src-tauri/src/ipc/visualizer.rs#L121)、[fft.rs:99](../crates/seraph-visualizer/src/fft.rs#L99)、[spectrum.rs:63](../crates/seraph-audio/src/spectrum.rs#L63)。

**现状：**侧栏 48 柱频谱也会经 `pump()` 同时运行侧栏 FFT、分析页 FFT 和完整声学分析。两个命令是同步函数，计算在命令调用链内完成；每轮新建采样 `Vec`，FFT 路径还有 mono、快照和复数数组的分配。主分析页与侧栏小频谱在当前布局中互斥显示，但底层仍统一计算两套结果。

**建议：**为侧栏频谱、响度、声场、示波器等维护需求标记；独立工作线程作为 tap 唯一读者，按需计算并发布最新快照。命令只读取结果；复用采样、FFT 输入及 scratch 缓冲。明确积分响度从何时开始累计，避免按需计算后悄悄改变统计语义。

**验收：**分别测量普通播放、分析页、关闭部分仪表和最小化时的 CPU、分配量、命令耗时与采样丢弃数。继续保持音频渲染侧的非阻塞 `try_lock` 与帧对齐约束。

### OPT-05：轮询采用单请求在途，并丢弃过期帧

**位置：**[AnalysisPage.tsx:417](../src/components/pages/main-pages/AnalysisPage.tsx#L417)、[SpectrumPanel.tsx:104](../src/components/sidebar/SpectrumPanel.tsx#L104)、[visualizer.rs:36](../src-tauri/src/ipc/visualizer.rs#L36)。

**现状：**两个组件均每 33 ms 直接发起请求，没有在途限制。分析页清理时只清 interval，迟到 Promise 仍可能更新视图；侧栏已有 `disposed` 检查。分析响应没有曲目/会话代际，组件也没有显式依据窗口可见性暂停请求。

**建议：**使用“完成后调度下一次”或单请求在途机制；响应携带会话代际/帧序号，切歌、暂停和卸载后丢弃旧响应。结合页面可见性与 Tauri 窗口状态停止不需要的计算。所有仪表关闭时同步减少后端需求。

**验收：**模拟 100～300 ms 的 IPC 延迟及响应乱序，确认最多一个请求在途，旧曲目的结果不能回填新曲目；最小化、恢复、切页后没有多重定时器。窗口隐藏时的具体系统节流效果仍需桌面测量。

### OPT-06：响度统计按新数据缓存，历史淘汰使用环形结构

**位置：**[analysis.rs:22](../crates/seraph-visualizer/src/analysis.rs#L22)、[analysis.rs:332](../crates/seraph-visualizer/src/analysis.rs#L332)、[analysis.rs:395](../crates/seraph-visualizer/src/analysis.rs#L395)、[analysis.rs:467](../crates/seraph-visualizer/src/analysis.rs#L467)。

**现状：**历史已有 200,000 个门限块和 20,000 个 LRA 样本的上限，不属于无限内存增长。达到上限后 `Vec::remove(0)` 会搬移元素；每次 `snapshot()` 又扫描积分历史、创建临时数组并对 LRA 排序，而门限块每 100 ms 才更新，LRA 样本每秒才更新。

**建议：**将统计值缓存到相应数据更新时刻，减少 30 fps 下重复求值；使用 `VecDeque` 或固定环形历史替代头部删除。保留门限计算与百分位口径，再依据基准决定是否需要直方图等进一步优化。

**验收：**沿用合成信号正确性测试，补充静音段、动态门限、长历史及重置对照；固定历史连续读取快照不再反复完整排序。若仍保留约 5.56 小时窗口，明确其与整段积分的区别。

### OPT-07：先返回可用曲库，补扫与导入作为独立任务

**位置：**[commands.rs:17](../src-tauri/src/ipc/library/commands.rs#L17)、[commands.rs:89](../src-tauri/src/ipc/library/commands.rs#L89)、[media_library.rs:17](../src-tauri/src/ipc/library/media_library.rs#L17)、[media_library.rs:903](../src-tauri/src/ipc/library/media_library.rs#L903)、[media_library.rs:1000](../src-tauri/src/ipc/library/media_library.rs#L1000)、[useHydratePlayerStore.ts:29](../src/hooks/useHydratePlayerStore.ts#L29)。

**现状：**`get_playlist` 等封面补扫和封面 GC 完成后才返回曲库。补扫有版本标记、GC 每进程只跑一次，但首次升级和启动仍可能等待；GC 期间持有 `LIBRARY_LOCK` 做目录遍历及删除。

本地导入虽已进入 `spawn_blocking`，仍串行遍历并重读每个文件的元数据，全部完成后才返回。没有跨次导入的文件指纹缓存、分批进度或取消参数；单文件解析错误经 `?` 向上传播时会结束本次导入，已经收集的记录尚未提交。

**建议：**先返回现有曲库，后台补扫后发布增量更新；GC 用候选快照并在删除前复核引用，缩短持锁范围。导入采用任务 ID、进度、取消令牌及逐文件结果，按路径/大小/修改时间判断是否需要重扫，并保留强制刷新。需要提速时使用有上限的并行解析。

**验收：**覆盖首次补扫、重复导入同一目录、网络盘/慢磁盘、坏文件、无权限目录和中途取消。好文件的成功结果与失败清单都可见；并发导入/删除/封面更新不能互相覆盖。

### OPT-08：排序结果复用，避免输入时重复完整排序

**位置：**[TrackRows.tsx:45](../src/components/pages/main-pages/TrackRows.tsx#L45)、[TrackRows.tsx:130](../src/components/pages/main-pages/TrackRows.tsx#L130)、[trackFilters.ts:46](../src/components/pages/main-pages/trackFilters.ts#L46)。

**现状：**曲目行已有虚拟滚动，但每次输入仍在 React 渲染路径中执行过滤与排序。`useMemo` 会在 `query` 变化时重算；标题/艺术家/专辑排序重新创建 `Intl.Collator` 并排序命中的全部记录。5 万首合成大结果集的 P95 已接近 197 ms。

**建议：**排序按曲库版本和排序键缓存，再按查询词过滤已排序 ID；缓存规范化检索字段与 collator。`useDeferredValue` 可优先呈现输入，但单次同步排序仍不能被中途打断，较大任务应放 Web Worker 或后端索引中，并按请求版本丢弃旧结果。

**验收：**持续输入、中文输入法组合、清空检索及切换排序时响应稳定；用 React Profiler/WebView 复测 1 万和 5 万首。排序保持稳定、默认队列顺序与曲目 ID 映射不变。

### OPT-09：专辑与艺术家页面限制挂载和图片解码数量

**位置：**[AlbumsPage.tsx:92](../src/components/pages/main-pages/AlbumsPage.tsx#L92)、[ArtistsPage.tsx:76](../src/components/pages/main-pages/ArtistsPage.tsx#L76)、[media_library.rs:847](../src-tauri/src/ipc/library/media_library.rs#L847)。

**现状：**专辑、艺术家网格对全部分组直接 `.map()`，没有曲目列表那样的虚拟化；图片没有 `loading="lazy"`。本地封面按原始图片落盘，文件大小上限不能控制图片像素尺寸，40 px 的艺术家头像也可能解码整张大图。

**建议：**先加入图片懒加载和异步解码，再为大网格采用可随列数变化的虚拟化；生成带像素上限的缩略图，按内容哈希、尺寸和转换版本缓存。保留原图供大封面使用。

**验收：**上千专辑/艺术家、混合高分辨率图片、窗口缩放及键盘访问场景下，挂载卡片与图片请求数接近可见范围；记录滚动掉帧与内存峰值。

### OPT-10：精简字体产物，拆出分析/EQ/流媒体页面

**位置：**[main.tsx:8](../src/main.tsx#L8)、[MainPages.tsx:3](../src/components/pages/MainPages.tsx#L3)、[App.tsx:16](../src/App.tsx#L16)。

**现状：**入口导入 Courier Prime 400、Noto Sans SC 400/700 的完整 CSS，产物包含 396 个字体文件。`App` 已懒加载侧栏和弹窗，但 `MainPages` 静态导入所有主页面，分析绘图、布局编辑、EQ 和扫码页面仍在主入口依赖图内。

**建议：**依据支持的 WebView2 版本只打包必要字体格式，优先评估移除 WOFF；字体仍保留中文动态曲名所需字符或可靠的系统回退。使用 React `lazy` 拆出 Analysis/EQ/Streaming，并在导航悬停或空闲时预热。

**验收：**对照上述构建大小，核对字体网络请求、冷启动到可操作时间和首次切页时间。验证断网、中文生僻字、日文、粗体和高 DPI。移除 6.216 MB WOFF 是资源目录的理论空间，实际安装器收益需重新打包测量。

### OPT-11：下载任务支持当前文件取消，并隔离阻塞写入

**位置：**[bilibili/commands.rs:3](../src-tauri/src/ipc/bilibili/commands.rs#L3)、[bilibili/commands.rs:59](../src-tauri/src/ipc/bilibili/commands.rs#L59)、[import_audio.rs:570](../src-tauri/src/ipc/bilibili/import_audio.rs#L570)。

**现状：**收藏夹已有取消入口，但只在两首之间检查，当前大文件完成前仍会继续下载。音频下载已流式读取并限制大小，不过 async 循环内仍调用同步 `File::create`、`write_all` 和 `flush`；慢磁盘会占用运行时执行线程。

**建议：**取消令牌贯穿解析、网络读取、转封装及临时文件清理；任务带 ID，状态与取消作用于对应任务。写入使用异步文件 API，或有界队列连接专用写入线程；保留下载去重、响应大小上限和整批缓存保护。

**验收：**长文件下载中取消、网络停滞、磁盘变慢、转封装失败时，状态能及时收敛，临时文件按设计清理；已经完成的曲目保留。重试使用退避，批量并发增加前先测服务限流和磁盘负载。

### OPT-12：将 JSON 编码纳入持久化防抖

**位置：**[persistStorage.ts:99](../src/store/player/persistStorage.ts#L99)。

**现状：**300 ms trailing debounce 已减少 `localStorage.setItem`，但 `JSON.stringify(value)` 在创建定时器前执行。音量滑块连续变化时，包含收藏和用户歌单的持久化对象仍会反复完整编码。

**建议：**等待期间只保存最新不可变状态，flush 时统一编码和写入；大型收藏/歌单与频繁变化的音量、进度设置可进一步分开存储。

**验收：**合成大量歌单 ID，统计连续拖动期间的编码次数与主线程耗时；继续通过水合门闩、五秒进度粒度、设置单字段持久化、`pagehide` 最后一次写入及存储失败回退测试。

### OPT-13：先建立发布版诊断和音频性能指标

**位置：**[lib.rs:22](../src-tauri/src/lib.rs#L22)、[lib.rs:174](../src-tauri/src/lib.rs#L174)、[engine.rs:47](../crates/seraph-audio/src/engine.rs#L47)、[engine.rs:1150](../crates/seraph-audio/src/engine.rs#L1150)、[engine.rs:2032](../crates/seraph-audio/src/engine.rs#L2032)。

**现状：**应用仅在 `debug_assertions` 下初始化 tracing，正式版本的现有 tracing 调用缺少这里提供的输出通道。音频不足一帧时直接输出静音，未见 underrun 汇总计数；独占输出采用轮询及至少 100 ms 的设备缓冲，这些数值主要依据已有兼容性修复确定。

**建议：**提供限大小、轮转的本地诊断日志与可导出摘要，记录版本、输出格式、操作阶段和耗时。音频回调只更新轻量计数，在后台汇总缺样、队列水位、解码耗时、命令延迟和 tap 丢帧。以这些数据决定是否需要缓冲档位或事件驱动 WASAPI 输出。

**验收：**正式构建中能追踪初始化、设备切换、缓存和播放错误；日志不包含登录凭据。测量共享/独占、44.1/48/192/384 kHz、暂停恢复、USB DAC 拔插和最小化。调整缓冲或唤醒策略必须有真实硬件回归证据。

### OPT-14：继续统一 IPC 命令、事件和错误契约

**位置：**[tauri.ts:8](../src/lib/tauri.ts#L8)、[tauri.ts:52](../src/lib/tauri.ts#L52)、[error.rs:81](../src-tauri/src/ipc/error.rs#L81)、[event.rs:10](../crates/seraph-core/src/event.rs#L10)、[usePlayback.ts:20](../src/hooks/usePlayback.ts#L20)。

**现状：**命令名是任意字符串，参数是 `Record<string, unknown>`，返回类型由调用方指定；事件前端按字符串和未知字段判断。错误正逐步迁移，但播放/B 站命令仍有字符串返回，部分错误码通过匹配中文消息生成。另外，在已识别为 Tauri 的环境中，API 动态导入失败仍会缓存浏览器 stub，后续命令可能表现为成功的空操作。

**建议：**建立命令到参数/结果的类型映射，或从 Rust DTO 生成 TypeScript 类型；事件采用有会话身份的判别联合。错误在源头形成稳定错误码，界面决定中文提示。桌面 IPC 初始化失败应明确报告并支持恢复，浏览器开发 stub 由运行环境明确选择。

**验收：**错误命令参数在类型检查阶段失败；新增命令权限仍通过现有窗口白名单回归。覆盖桥接加载失败、旧事件、结构化错误及合法浏览器预览路径，避免仅由手写泛型制造类型安全的表象。

### OPT-15：固定验证环境，增加性能和桌面层验收

**位置：**[rust-toolchain.toml](../rust-toolchain.toml)、[package.json:7](../package.json#L7)、[ci.yml:25](../.github/workflows/ci.yml#L25)、[release.yml:65](../.github/workflows/release.yml#L65)。

**现状：**已有测试、类型检查、Rust 格式/Clippy、前后端依赖审计及 Actions SHA 固定，基础门禁较完整。Rust 工具链仍跟随 `stable`，工作流的 Cargo 测试/Clippy 未加 `--locked`，`cargo install --locked cargo-audit` 也未固定工具版本。CI 使用 Node 22，本机是 24；项目未声明统一的 Node 版本范围。没有看到专门性能基准、桌面端到端测试或前端 lint 命令。

**建议：**固定经验证的 Rust 与 Node 范围，日常与发布 Cargo 命令使用 `--locked`；固定审计工具版本并定期升级。增设前端 hooks/lint 检查。将本报告微基准整理为独立性能入口；补曲库导入、首次播放、任务栏歌词和重启恢复的真实桌面验收。

**验收：**干净环境能复现构建；锁文件需更新时检查明确失败。性能记录机器、版本和数据集，对同机基线变化告警。单元测试、浏览器交互、Tauri 桌面和真实音频硬件分层记录，避免把 mock 测试通过当作声卡验证。

### OPT-16：围绕热点继续拆分模块，收拢领域逻辑

**位置：**[engine.rs](../crates/seraph-audio/src/engine.rs)、[media_library.rs](../src-tauri/src/ipc/library/media_library.rs)、[AnalysisPage.tsx](../src/components/pages/main-pages/AnalysisPage.tsx)、[library.rs](../crates/seraph-playlist/src/library.rs)。

**现状：**已有 `crates` 分层、player action 文件和 IPC 子模块拆分。仍有音频引擎约 2,499 行、曲库模块约 1,221 行、分析页约 1,156 行，以上含注释/测试。`seraph-playlist` 定义了媒体库 trait，但实际曲库读写与解析主要位于 Tauri IPC 目录。

**建议：**随上述优化抽离输出会话/渲染适配、曲库仓储/扫描任务、分析数据订阅/面板呈现；让 IPC 层负责参数、权限和调用转换。明确 `ImportedTrack`、核心 Track、前端 Track 各自用途，并通过转换边界衔接。

**验收：**仓储、队列和分析调度可在不构造 Tauri AppHandle 的情况下验证；每次拆分对应一项实际功能或性能目标，原有回归仍通过。

## 5. 建议实施顺序

| 阶段 | 建议范围 | 交付与判断依据 |
| --- | --- | --- |
| 第一轮：范围较小的改进 | OPT-02 单曲索引、OPT-05 轮询控制、OPT-10 字体与页面拆包、OPT-12 编码防抖 | 请求数/字节数、构建体积、编码次数和交互基线可直接对比 |
| 第二轮：数据通路 | OPT-01、OPT-03、OPT-07、OPT-08 | 队列增量、存储故障恢复、增量扫描、大曲库输入响应 |
| 第三轮：持续播放体验 | OPT-04、OPT-06、OPT-09、OPT-11 | 普通播放 CPU、长历史分析、图片内存、下载取消响应 |
| 贯穿各轮 | OPT-13、OPT-14、OPT-15；涉及模块时推进 OPT-16 | 诊断能力、契约检查、性能回归与硬件验收记录 |

建议每次改动保留一份前后对照，至少记录：启动到曲库可操作时间、单曲查询 P95、队列请求字节数、搜索 P95、普通播放/分析页 CPU、图片内存峰值及实际播放缺样数。绝对指标应在目标设备测量后确定。

## 6. 已有能力应继续保留

- 曲目列表已经使用固定行高虚拟滚动和 ID 索引，不需要重复引入同类方案。
- 播放代际、随机下一首预选、设备枚举代际、歌词身份核对等已有回归，应作为队列优化的约束。
- 音频渲染已有环形缓冲、非阻塞 tap、帧对齐、增益斜坡及多处 scratch 复用；性能优化要保留实时线程边界。
- 持久化已有迁移/清洗、水合门闩、写入防抖；曲库已有内存快照、主文件损坏保护及单文件原子替换。
- 网络与发布已有 URL/路径约束、下载体积限制、窗口命令白名单、SHA 固定和依赖审计。本次没有证据支持放松这些约束。

## 附录 A：复现前端合成基准

从仓库根目录在 PowerShell 执行。使用已安装的 TypeScript，仅调用生产纯函数；桥接函数替换为抛错占位，基准不发送 IPC。结果会受机器负载和 Node 版本影响。

```powershell
@'
import fs from 'node:fs';
import ts from 'typescript';
import { performance } from 'node:perf_hooks';

async function loadSource(file, stubInvoke = false) {
  let js = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 }
  }).outputText;
  if (stubInvoke) {
    const declaration = 'import { invoke } from "@/lib/tauri";';
    if (!js.includes(declaration)) throw new Error('Unexpected source import');
    js = js.replace(declaration,
      'const invoke = async () => { throw new Error("IPC is outside this benchmark"); };');
  }
  return import('data:text/javascript;base64,' + Buffer.from(js).toString('base64'));
}
const { filterAndSortTracks } = await loadSource('src/components/pages/main-pages/trackFilters.ts');
const { playbackQueueArgs } = await loadSource('src/store/player/queueSync.ts', true);
let checksum = 0;
function measure(fn) {
  for (let i = 0; i < 5; i++) fn();
  const durations = [];
  for (let i = 0; i < 25; i++) {
    const start = performance.now();
    const result = fn();
    durations.push(performance.now() - start);
    checksum += typeof result === 'string' ? result.length : result.length ?? 0;
  }
  durations.sort((a, b) => a - b);
  return { p50Ms: +durations[12].toFixed(3), p95Ms: +durations[23].toFixed(3) };
}
for (const size of [1000, 10000, 50000]) {
  let seed = 20260913;
  const random = () => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed;
  };
  const playlist = Array.from({ length: size }, (_, i) => ({
    id: 'local-' + String(i).padStart(8, '0'),
    path: 'D:/Music/Album ' + Math.floor(i / 12) + '/Track ' + i + '.flac',
    title: '曲目 ' + random(), artist: '艺术家 ' + (i % 1000),
    album: '专辑 ' + Math.floor(i / 12),
    cover: 'C:/MusicCache/covers/' + String(i % 3000).padStart(16, '0') + '.jpg',
    duration: 180 + i % 120
  }));
  const get = () => ({ playlist, currentTrackIndex: 0, recentTrackIds: [],
    shuffleMode: false, loopMode: false });
  console.log({ tracks: size,
    filter: measure(() => filterAndSortTracks(playlist, '曲目', 'default')),
    sort: measure(() => filterAndSortTracks(playlist, '曲目', 'title')),
    queue: measure(() => JSON.stringify(playbackQueueArgs(get))),
    queueBytes: Buffer.byteLength(JSON.stringify(playbackQueueArgs(get))) });
}
console.log({ node: process.version, checksum });
'@ | node --input-type=module
```

## 附录 B：复现 Rust 响度历史快照基准

同样从仓库根目录执行。只在已忽略的 `target` 目录生成源码副本和可执行文件；填充历史后调用生产 `snapshot()`，不播放声音。此入口依赖当前内部字段，模块调整时需同步更新。

```powershell
$reviewHarness = @'

fn main() {
    use std::{hint::black_box, time::Instant};
    for seconds in [60_usize, 3_600, 20_000] {
        let mut engine = AnalysisEngine::new(48_000, 2);
        let mut seed = 20_260_913_u64;
        let mut energy = || {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            1.0e-8 + ((seed >> 32) as u32 as f64 / u32::MAX as f64) * 0.2
        };
        engine.gating_blocks = (0..seconds * 10).map(|_| energy()).collect();
        engine.lra_samples = (0..seconds).map(|_| energy()).collect();
        for _ in 0..10 { black_box(engine.snapshot()); }
        let mut timings = Vec::with_capacity(100);
        for _ in 0..100 {
            let start = Instant::now();
            black_box(engine.snapshot());
            timings.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
        timings.sort_by(f64::total_cmp);
        println!("history_seconds={seconds}, p50_ms={:.4}, p95_ms={:.4}",
            timings[49], timings[94]);
    }
}
'@
New-Item -ItemType Directory -Path target -Force | Out-Null
$reviewSource = Get-Content -Raw -LiteralPath crates/seraph-visualizer/src/analysis.rs
Set-Content -LiteralPath target/optimization-analysis-benchmark.rs `
    -Value ($reviewSource + $reviewHarness) -Encoding utf8
rustc --edition=2021 -O target/optimization-analysis-benchmark.rs `
    -o target/optimization-analysis-benchmark.exe
if ($LASTEXITCODE -eq 0) { & .\target\optimization-analysis-benchmark.exe }
```
