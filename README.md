# Seraph Audio Player

Premium local HiFi audio player built with Rust, Tauri, and React.

Seraph Audio Player 是一款面向本地高保真音乐播放的桌面播放器，目标是在 Windows 上提供低延迟、可控、稳定的本地音频播放体验。底层播放、设备与切歌状态由 Rust 后端负责，React 前端只作为 UI 投影层。

## 核心特性

- **Rust 音频后端**：播放状态、切歌、结束续播、上一首/下一首由 Rust 统一管理，减少前后端状态分叉。
- **WASAPI Exclusive**：支持 Windows WASAPI 独占输出，绕过系统混音路径。
- **多格式解码**：基于 Symphonia / FFmpeg 的多级解码路径，支持常见本地音频与部分流媒体缓存文件。
- **DSD/高采样率处理**：包含 DSD PCM 转换与重采样处理模块。
- **系统媒体控件（SMTC）**：键盘/蓝牙媒体键控制播放，系统音量浮窗与锁屏展示曲目标题、艺术家、封面与进度。
- **实时频谱可视化**：渲染线程实时安全旁路输出样本，FFT log 频段柱状频谱（48 段）随播放展示。
- **本地封面识别**：导入时提取音频内嵌封面（按内容哈希本地缓存、同专辑去重），经 Tauri asset 协议在播放条转盘、专辑/艺术家视图中展示；旧曲库首次启动自动补扫，孤儿封面自动清理；无内嵌封面的曲目支持在线匹配（QQ 音乐 / iTunes）。
- **曲库检索与排序**：曲目列表支持标题/艺术家/专辑检索与多键排序，虚拟列表大曲库流畅滚动。
- **歌单管理**：加入歌单、歌单内排序、移除，支持 M3U8 清单导入导出。
- **播放进度恢复**：重启后恢复上次曲目与播放位置，一键续播。
- **应用内检查更新**：启动静默检查 GitHub Releases，发现新版可一键前往下载页。
- **Bilibili 音频导入与缓存**：支持导入 Bilibili 音频并管理本地缓存。
- **缓存保护机制**：缓存目录写入 `.seraph-cache` 标记，清理时只处理受管理的缓存文件，降低误删风险。
- **状态一致性保护**：播放命令会等待 Rust 音频线程返回真实执行结果，再同步给前端 UI。
- **持久化迁移**：前端播放偏好使用版本化持久化状态，旧字段会在启动时自动迁移到当前结构。
- **收窄 Tauri 权限**：桌面壳只开启窗口控制、事件监听、拖放和打开/保存文件对话框所需权限。
- **中英文 Windows 安装包**：Tauri 打包配置会生成英文/中文 MSI，并为 NSIS EXE 安装器启用语言选择。

## 架构概览

```text
Seraph Audio Player
├─ crates/
│  ├─ seraph-core/        # 共享事件、状态与领域类型
│  ├─ seraph-audio/       # 播放控制器、WASAPI/CPAL 输出、播放会话
│  ├─ seraph-decoder/     # Symphonia / FFmpeg / DSD 解码
│  ├─ seraph-dsp/         # 重采样与 DSD DSP
│  ├─ seraph-playlist/    # 播放列表与曲库模型
│  └─ seraph-visualizer/  # 频谱/可视化基础模块
├─ src-tauri/
│  └─ src/ipc/            # Tauri IPC、缓存、曲库、播放命令
└─ src/
   ├─ components/         # React UI
   ├─ hooks/              # 播放事件、拖放导入、波形等 hook
   └─ store/              # 前端 UI 状态与后端命令封装
```

## 播放状态机

当前播放队列、随机/循环模式、播放结束后的续播、`next_track` / `prev_track` 都由 Rust 后端处理。

前端负责：

- 同步当前队列快照到后端；
- 发送播放、暂停、上一首、下一首等命令；
- 监听后端 `TrackChanged`、`PlaybackStarted`、`PlaybackStopped`、`Progress` 等事件并更新 UI。

这样可以避免播放结束时前端自己推算下一首，导致 UI 状态和真实音频后端分叉。

## 缓存默认路径

缓存设置保存在应用数据目录的 `cache-settings.json`。已有设置文件的用户不会被自动迁移。

新用户首次启动时，默认缓存目录按以下优先级选择：

1. `<应用 exe 所在目录>/bilibili-cache`
2. `C:\ProgramData\Seraph Audio Player\bilibili-cache`
3. Tauri AppData 目录下的 `bilibili-cache`

每个候选路径都会尝试创建目录并写入 `.seraph-cache` 标记；如果不可写或不安全，会自动尝试下一个路径。

## 本地开发

### 环境要求

- Node.js 22+
- Rust stable
- Windows SDK / MSVC 工具链
- Tauri CLI 依赖由项目脚本调用

### 安装依赖

```bash
npm install
```

### 前端开发模式

```bash
npm run dev
```

### Tauri 桌面开发模式

```bash
npm run tauri:dev
```

### 类型检查与测试

```bash
npm run typecheck
npm test
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm audit --audit-level=low
```

这些检查也会在 GitHub Release 工作流中作为发布门禁执行。

### 版本升级

```bash
npm run bump 0.4.0
```

一次同步 `package.json`、`src-tauri/tauri.conf.json`、workspace `Cargo.toml` 三处版本号并刷新两个 lock 文件。

## 构建安装包

只生成 Windows EXE 安装器：

```bash
npm run tauri -- build --bundles nsis
```

同时生成 Windows EXE 安装器和 MSI：

```bash
npm run tauri -- build --bundles nsis,msi
```

产物默认位于：

```text
target/release/bundle/nsis/
target/release/bundle/msi/
```

当前 Tauri 配置会生成：

- NSIS EXE 安装器；
- `en-US` MSI；
- `zh-CN` MSI。

## 版本记录

### v0.4.0

架构与存储：

- **模块结构重构**：`library` 与 `bilibili` 从 `include!` 拼接改为真模块树（共享项收敛到各自 `prelude`，跨模块项显式 `pub(crate)`），恢复模块边界与 rust-analyzer 索引能力。
- **曲库存储改造**：曲库快照内存常驻（读命令零磁盘 IO），歌词从主 JSON 拆分为 `library-lyrics.json` 边车文件（旧格式自动迁移），主文件写入体积与序列化成本显著下降；保持原子写、损坏备份拒写、读改写互斥等既有保护。
- **结构化 IPC 错误**：library/cache 命令族迁移到 `IpcError`（thiserror，序列化为 `{ code, message }`），前端可按错误码分支处理。

新功能：

- **SMTC 系统媒体控件**：键盘/蓝牙媒体键播放、暂停、切歌、定位；系统音量浮窗与锁屏展示标题/艺术家/专辑/封面/进度。媒体键动作与应用内操作走同一控制路径。
- **实时频谱可视化**：渲染循环以实时安全方式（try_lock + 预分配环形缓冲，绝不阻塞音频线程）旁路输出样本，FFT 在 IPC 线程计算，右栏 48 段 log 频谱 ~30fps 展示，暂停自动衰减收敛。
- **曲库检索与排序**：所有曲目列表页支持标题/艺术家/专辑检索（大小写不敏感）与默认/标题/艺术家/专辑/时长排序，过滤只影响展示不影响播放队列。
- **播放进度恢复**：按 5 秒粒度持久化播放位置，重启后恢复上次曲目与进度，播放键直接续播。
- **应用内检查更新**：设置页手动检查 + 启动静默检查 GitHub Releases，发现新版一键打开下载页（URL 白名单校验）。
- **歌单增强**：曲目行「加入歌单」入口 + 选择弹窗；歌单详情页支持排序（上移/下移）、移除、导出 M3U8；支持从 M3U8 清单导入并自动建歌单。
- **封面覆盖面扩展**：UP NEXT 卡片与艺术家页展示封面；无内嵌封面的本地曲目可在线匹配（QQ 音乐优先、iTunes 兜底，内容哈希落盘）；covers 目录孤儿封面启动自动清理（1 小时宽限期防误删）。

工程化：

- 前端测试 13 → 36 项（引入 @testing-library/react：检索/排序/歌单弹窗等组件交互覆盖）；后端测试 41 → 54 项（M3U8 解析、频谱环形缓冲、版本比较、封面 GC 等）。
- 新增 `npm run bump x.y.z` 版本同步脚本（三处版本声明 + 两个 lock 文件一次到位）；修正 workspace `repository` 字段。

### v0.3.8

- **本地封面识别**：导入本地曲目时用 lofty 提取内嵌封面（优先 Front Cover；MIME 缺失时按魔数识别 JPEG/PNG/GIF/BMP/WEBP），按内容哈希写入应用数据目录 `covers/`（同专辑多曲共用一张图，曲库 JSON 只存路径）；旧曲库首次启动自动一次性补扫（后台线程执行，完成后写标记，后续启动零开销）；封面经 Tauri asset 协议加载，访问范围仅开放 `covers` 目录。
- **播放条转盘封面化**：当前曲目有封面时（本地提取或 B 站视频封面），封面以圆形填入左下角转盘并沿用原旋转动画；无封面或封面加载失败时回退默认同心圆刻度盘。
- **专辑视图封面**：专辑网格展示本地提取的封面；专辑内第一首缺图时用组内其他曲目封面兜底。
- **界面减拥挤改版**：页面大标题头压缩为单行（描述超宽截断、悬停显示全文）；流媒体页控制台去掉三段分区标签，压缩为「归档输入行 + 账号/音质开关行」两行工具条；播放队列改为占满剩余高度，1080p 下可见行数明显增加（本地音乐、最近播放、我喜欢、专辑/艺术家详情页同步生效）。
- **修复**：B 站登录/FFmpeg 状态刷新在 `invoke` 返回 `undefined` 时不再把状态覆盖为 `undefined`（此前会导致流媒体页读取 `loggedIn` 崩溃）。

### v0.3.7

- 第二轮全代码库 BUG 审查（四通道并行 + 对 v0.3.6 修复提交的回归审查），修复 2 项高危、11 项中危及 20 余项低危问题。
- 音频引擎：修复暂停→恢复后左右声道持续互换（渲染消费改为严格按帧对齐，v0.3.6 增益斜坡引入的回归）；stop/切歌路径补全 ramp-out，独占模式退出前等待设备缓冲排空，消除硬切爆音；seek 失败时回滚进度并提示，不再与实际声音持续错位；探测畸形文件的 open/probe 路径加 panic 隔离，坏文件不再导致所有播放功能永久失效；4.0/5.0 声道布局下混不再误丢声道；曲终收尾与用户停止并发时不再误触发自动切歌；消除 seek 竞态下短暂闪回旧位置音频的窗口；开播 1 秒内重复点播同曲现在正确从头播放。
- 解码器：Symphonia seek 裁剪按 time_base 正确换算为帧（修复 MP4/MKV 类容器 seek 落点偏差）；ffmpeg 崩溃检测改为轮询等待退出码，中途崩溃不再被误判为正常播完；DSF/DFF 声明时长与文件真实大小交叉校验，截断文件不再显示虚大时长。
- Tauri 后端：B 站会话文件改为原子写入 + 损坏自愈（此前损坏会导致全部 B 站功能瘫痪且需手动删文件）；缓存清理移出 async 直调避免阻塞运行时；收藏夹批量导入不再逐首触发缓存清理误删同批文件；FFmpeg 下载加并发保护；KRC 翻译歌词对齐修复；remux 失败后的 fallback 缓存可被复用不再重复下载。
- 前端：修复启动水合空窗期内操作会用默认状态覆盖 localStorage、导致收藏/歌单不可逆丢失的高危问题；播放代际守卫下沉到 play 命令发出前，快速切歌不再出现"实际播 A、显示 B"；B 站重缓存加进行中去重，重复点击不再触发重复下载；重缓存失败后播放状态正确复位；流媒体页的 FFmpeg 下载与扫码登录状态提升到全局 store，切页不再中断登录轮询或重置下载进度；时长格式化支持小时段并防 NaN；音量拖到 0 后取消静音恢复正确音量等多项修复。

### v0.3.6

- 完成全代码库深度审查（约 1.8 万行），审查报告见 `docs/audit/`，共修复 60+ 项问题。
- 音频引擎：新增每样本增益斜坡，消除暂停/恢复、seek、音量调整时的爆音与 zipper noise；seek 越界钳制；解码线程 panic 兜底；WASAPI 初始化超时不再阻塞引擎线程；5.1/7.1 → 立体声按 ITU 系数下混；16bit 输出加入 TPDF dither 并统一四舍五入量化。
- 解码/DSP：修复 DSF 末尾 padding 导致的曲目结尾爆音；DC blocker 截止频率按实际 PCM 采样率计算；DSD 容器头严格校验（防损坏文件导致崩溃或死循环）；DSD 回放 +6 dB 增益补偿；启用 Symphonia gapless；seek 精确到样本级；修正 DoP 打包字节序。
- Tauri 后端：修复 B 站登录 Cookie 可能泄漏到任意链接的安全问题（host 白名单）；曲库缓存改为原子写入并对损坏文件备份拒写；下载改用专用超时策略客户端；`.eac3` 纳入缓存配额管理；曲库读写加锁防并发丢更新；移除 ffmpeg 第三方下载镜像。
- 前端：启动后恢复上次播放曲目；快速切歌请求代际守卫；歌词应用锁定目标曲目；修复两处事件监听器泄漏；进度条拖动后抑制旧进度回跳；虚拟列表随窗口尺寸自适应。

### v0.3.5

- 修复歌曲列表序号在不同页面中仍显示全局固定编号的问题。
- 当前页面、筛选结果和虚拟滚动列表会按视图内顺序重新显示 `REC.001`、`REC.002` 等编号。
- 播放逻辑仍使用全局播放队列索引，不影响点击播放、收藏和删除操作。

## GitHub Release

仓库包含 `.github/workflows/release.yml`。推送 `v*` tag 时会触发 Windows release 构建，并发布安装包到 GitHub Release。

示例：

```bash
git tag v0.4.0
git push origin v0.4.0
```

## 许可证

MIT License
