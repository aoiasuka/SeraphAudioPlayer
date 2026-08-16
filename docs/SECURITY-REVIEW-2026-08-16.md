# Seraph Audio Player 全量安全审查报告

- **审查日期**：2026-08-16
- **审查对象**：`main` 分支（v0.5.8，commit `cbdfbe2`）
- **审查方式**：只读静态审查，未修改任何代码
- **落地状态（2026-08-16 复核后追记）**：**v0.5.9 已全量落地**。其中 L-2 按本报告结论采取「威胁模型明示」不改行为；L-4 的 DSF 单包封顶建议有意不做（硬截断会切碎 DSF 交错块结构导致解码错位，且分配量已被文件实际大小封顶）；§4.1.8 symphonia features 收敛有意不做（收窄可能改变既有文件解码路径，违背「加固必须实测」基线）。**复核勘误**：①L-3 实际共**四**处未挂重定向白名单——本报告漏点了 `bilibili/ffmpeg.rs` 的 `ffmpeg_download_client()`，v0.5.9 一并挂齐；②§五「taskbar-lyrics 放行 9 个命令」实为 **8 个**（`lib.rs` 白名单）；③L-6 示例「+240 dB 产生 Inf」不精确——+240 dB 仅为 10⁶ 线性增益（全刻度削波），f32 下溢出为 Inf 约需 +1540 dB，但「有限极端值可产生 Inf/NaN」的论断成立（JSON 可传 1e38）。
- **审查范围**：
  - Tauri 后端：`src-tauri/`（40 个 Rust 源文件、`tauri.conf.json`、capabilities、全部 55 个 `#[tauri::command]` 逐一检查）
  - 前端：`src/`（全部 23 处 `invoke` 调用点、全部渲染路径）、根目录 HTML、`vite.config.ts`
  - 核心 Rust crates：`crates/`（6 个 crate、29 个 .rs 文件、约 9400 行）
  - 配置与供应链：`Cargo.lock`（666 个 crate 经 OSV.dev 逐一核验）、`package-lock.json`（307 包，`npm audit` 实际执行）、CI workflows、构建脚本、git 全历史密钥扫描

---

## 一、总体结论

| 严重度 | 数量 |
|--------|------|
| 严重 | **0** |
| 高危 | **0** |
| 中危 | **1** |
| 低危 | **9** |
| 信息级 | 若干（见各节） |

**总评**：项目已经历多轮系统性安全加固（代码内 S-xx/F-xx/N-xx/P-xx/审2-x 编号体系可追溯，git 历史含 v0.5.6 十五项加固、v0.5.7 两轮复审），当前安全工程成熟度在同类 Tauri 应用中显著高于平均水平。本次审查未发现严重/高危问题；历史报告中的 plist/quick-xml/quinn-proto/anyhow/event-listener 漏洞已全部确认升级到位，`npm audit` 零漏洞。

所有发现项合计预计 < 150 行改动即可全部落地，其中多数与项目既有防线（`url_guard`/`http_util`/`path_guard`）风格一致，可直接沿用现有设施。

---

## 二、发现汇总表

| # | 严重度 | 位置 | 问题 | 所在节 |
|---|--------|------|------|--------|
| M-1 | 中 | `crates/seraph-decoder/src/ffmpeg.rs:577` | ffmpeg 裸名回退，Windows 当前目录 EXE 种植 | §3.1 |
| L-1 | 低 | `src-tauri/src/smtc.rs:222-231` | SMTC 封面 `file://` 分支可透传 UNC 路径（SMB 出站/NTLM 探测面） | §3.2 |
| L-2 | 低 | `src-tauri/src/ipc/path_guard.rs:34-72` | 导出类命令可写全盘任意白名单扩展名文件 | §3.3 |
| L-3 | 低 | `src-tauri/src/ipc/library/online_lyrics.rs:11` 等 3 处 | 三个出站 HTTP client 未挂逐跳重定向白名单（盲 SSRF 探测面） | §3.4 |
| L-4 | 低 | `crates/seraph-audio/src/engine.rs:29` 等 | 合法上限参数下最坏内存占用约 590 MB/次播放 | §3.5 |
| L-5 | 低 | `crates/seraph-dsp/src/eq.rs:87`、`crossfeed.rs:72` | `clamp` 在退化采样率下 panic（min > max），pub API 面风险 | §3.6 |
| L-6 | 低 | `crates/seraph-dsp/src/eq.rs:102,267` | EQ 增益无幅度上限，极端参数产生 Inf/NaN 音频 | §3.7 |
| L-7 | 低 | `mockup_archive_lyrics.html:7-9` | 设计稿从多个第三方 CDN 加载脚本（未进构建产物，仅开发机暴露） | §3.8 |
| L-8 | 低 | `Cargo.lock`（serde_with 3.20.0） | 已知 panic 漏洞 GHSA-7gcf-g7xr-8hxj，修复版 3.21.0 | §3.9 |
| L-9 | 低 | `.gitignore` | 遗漏 `.claude/settings.local.json`，换机克隆易意外提交 | §3.10 |

信息级事项见 §4。

---

## 三、详细发现

### 3.1【中】M-1：ffmpeg/ffprobe 二进制回退到裸名字，Windows 上可被当前目录植入（CWE-427）

- **位置**：`crates/seraph-decoder/src/ffmpeg.rs:577-583`
- **代码摘录**：
  ```rust
  fn ffmpeg_command_path() -> PathBuf {
      find_ffmpeg().unwrap_or_else(|| PathBuf::from(tool_exe_name("ffmpeg")))
  }
  ```
- **风险**：当 `find_tool()`（EXTRA_TOOL_DIRS / `SERAPH_FFMPEG_*` / exe 目录 / PATH）全部落空时，`Command::new("ffmpeg.exe")` 交由 Windows `CreateProcessW` 解析，其搜索顺序包含**当前工作目录**。若目标机器既未安装 ffmpeg、应用也未捆绑，而应用经文件关联等方式以攻击者可控目录为 cwd 启动（例如一个同时含有 `song.flac` 与 `ffmpeg.exe` 的"歌曲文件夹"），播放该文件即执行植入的任意可执行文件（本地代码执行）。
- **缓解因素**：需"目标机器没有 ffmpeg"+"攻击者可写 cwd"两个前提同时成立；应用自身目录优先于 cwd 搜索。
- **修复建议**：`find_ffmpeg()` 返回 `None` 时直接报 `UnsupportedFormat("ffmpeg not found")`，不要回退裸名交给 CreateProcess 隐式搜索；一行改动即可消除，建议下个版本修复。

### 3.2【低】L-1：SMTC 封面 `file://` 分支可透传 UNC 路径给 Windows Shell（SMB 出站 / NTLM 探测面）

- **位置**：`src-tauri/src/smtc.rs:222-231`
- **代码摘录**：
  ```rust
  fn cover_to_uri(cover: &str) -> Option<String> {
      ...
      if cover.starts_with("http://") || cover.starts_with("https://") {
          return crate::ipc::url_guard::is_safe_system_image_url(cover).then(|| cover.to_string());
      }
      Some(format!("file://{cover}"))   // ← 非 http 一律透传
  }
  ```
- **风险**：`track.cover` 来自磁盘上的 `library-cache.json`（可被同机其他用户/程序离线篡改）。http 分支已过 B 站图床白名单，但 `file://` 分支无任何约束：`cover = "\\attacker.com\share\a.jpg"` 会被 souvlaki 交给 Windows Shell 的 `GetFileFromPathAsync`，触发**向攻击者控制的 SMB 服务器发起出站连接**（域环境下可能伴随 NTLM 认证质询响应，泄露可用于离线破解的认证材料）。
- **修复建议**：`file://` 分支同样校验——`canonicalize` 后必须位于应用 `covers` 目录内（前缀比对），并显式拒绝 UNC 路径（`\\` 或 `//` 开头）。

### 3.3【低】L-2：导出类命令可写全盘任意白名单扩展名文件（渲染进程被攻破时的持久化跳板）

- **位置**：`src-tauri/src/ipc/path_guard.rs:34-72`；使用点 `src/ipc/config.rs:15-30`、`src/ipc/dsp.rs:59-74`、`src/ipc/playlist_io.rs:198-256`
- **代码摘录**：
  ```rust
  pub(crate) fn validate_export_path(raw: &str, allowed_extensions: &[&str]) -> IpcResult<PathBuf> {
      ...
      if !target.is_absolute() { return Err(...); }
      // 拒绝 .. 点段、校验扩展名白名单、父目录必须存在
  ```
- **风险**：防线已收窄到"绝对路径 + 扩展名白名单（.json/.txt/.m3u8/.m3u）+ 父目录存在"，但**目标目录无任何限制**，且 `export_app_config` 的 `content` 是任意字符串。若主窗口渲染进程被攻破（XSS/依赖漏洞），可直接以任意内容覆盖用户可写的任何 `.json`/`.txt` 文件（如 VSCode `settings.json`、npm 项目 `package.json`），构成从 XSS 到用户登录会话持久化执行的跳板。正常流程路径来自 dialog，但 IPC 层无法区分"用户在系统对话框确认的路径"与"渲染进程直构的路径"。
- **修复建议**（按成本递增任选其一）：
  1. 导出目标限制在用户目录（`home_dir` 前缀）内；
  2. 后端为 dialog 结果发一次性 token，导出命令只接受 token 对应路径；
  3. 至少对非 dialog 来源路径的 `export_*` 写入做事件告警。
  当前实现是可接受的权衡，但应在威胁模型中明示。

### 3.4【低】L-3：三处出站 HTTP client 未挂逐跳重定向白名单（与项目自身 F-01/F-04/F-07 纪律不一致）

- **位置**：
  - `src-tauri/src/ipc/library/online_lyrics.rs:11-26`（网易云/酷狗/QQ 歌词搜索与拉取）
  - `src-tauri/src/ipc/update.rs:39-42`（GitHub API 更新检查）
  - `src-tauri/src/ipc/bilibili/session.rs:8-18`（B 站 API + 短链解析裸 client）及 `src/ipc/bilibili/import_audio.rs:138-150`（`resolve_bvid` 只复验**最终** host，中间跳不拦）
- **代码对比**（已加固的下载 client vs 未加固的歌词 client）：
  ```rust
  // session.rs:31 —— 下载 client 已逐跳复验（F-01）
  .redirect(guarded_redirect_policy(is_safe_bilibili_download_url))
  // online_lyrics.rs:21-24 —— 歌词 client 无 redirect 策略，reqwest 默认跟随 10 跳
  Client::builder().default_headers(headers).timeout(Duration::from_secs(12)).build()
  ```
- **风险**：请求虽指向固定 URL，但上游/中间层若返回 302 指向 `http://127.0.0.1:port`、内网地址，应用会跟随发出请求，构成**盲 SSRF 内网探测面**（凭据泄露风险已由 reqwest 跨 host 重定向自动剥离 `Cookie`/`Authorization` 缓解，且歌词 client 本不带凭据）。攻击者难以直接读取响应体，但可通过应用行为差异（超时/成功/错误提示）推断内网服务存在性。项目自定纪律是"新增出站请求一律走 `guarded_redirect_policy`"（`http_util.rs:46-55`），上述三处属遗留不一致。
- **修复建议**：为歌词源（music.163.com / lyrics.kugou.com / c.y.qq.com）、api.github.com、bilibili.com 系各建一份 host 后缀白名单，统一挂 `guarded_redirect_policy`。

### 3.5【低】L-4：合法上限参数下的最坏内存占用（约 590 MB/次播放）

- **位置**：`crates/seraph-audio/src/engine.rs:29, 728-731`；`crates/seraph-decoder/src/dsd.rs:213-223`
- **代码摘录**：
  ```rust
  const MAX_RING_BUFFER_SAMPLES: usize = 768_000 * 32 * TARGET_BUFFER_SECONDS; // 73,728,000
  ...
  let max_buffer_samples = (output_rate as usize)
      .saturating_mul(output_channels)
      .saturating_mul(TARGET_BUFFER_SECONDS)
      .min(MAX_RING_BUFFER_SAMPLES);
  ```
- **风险**：`validate_stream_info` 放行 8k–768 kHz / 1–32 ch，一个几 KB 头部 + 巨大 data 的"合法区间"恶意文件（如 768 kHz × 32 ch）会让 rtrb 环形缓冲一次分配约 590 MB（73.7M 槽 × 8 字节）；DSF 路径每包还有最多 `block_size(≤1MB) × channels(≤32) = 32 MB` 的 raw 读缓冲。这是有上界的内存压力型 DoS（低配机器可 OOM），非无界。
- **修复建议**：按声道数/采样率动态折减 `TARGET_BUFFER_SECONDS`（如 channels > 2 或 rate > 192 kHz 时降到 1 s）；DSF 的 `block_size_per_channel × channels` 读包大小封顶（如 ≤ 4 MB/包）。

### 3.6【低】L-5：EQ/Crossfeed 的 `clamp` 在退化采样率下会 panic（min > max）

- **位置**：`crates/seraph-dsp/src/eq.rs:87`、`crates/seraph-dsp/src/crossfeed.rs:72`
- **代码摘录**：
  ```rust
  let freq = band.freq.clamp(1.0, nyquist - 1.0).max(1.0);            // eq.rs
  let cutoff = settings.cutoff_hz.clamp(100.0, sample_rate.max(2.0) * 0.5 - 1.0);  // crossfeed.rs
  ```
- **风险**：`f32/f64::clamp` 在 `min > max` 时直接 panic。eq 在 `sample_rate < 4.0`、crossfeed 在 `sample_rate ≤ 202` 时触发。当前内部调用链（engine → DspWorkerSession）保证采样率 ≥ 8000（解码层闸门），**实际不可达**；但两者都是 `pub` API，第三方/未来调用方传入退化采样率即 panic。
- **修复建议**：先取 `let hi = (nyquist - 1.0).max(1.0);` 再 clamp；crossfeed 同理。

### 3.7【低】L-6：EQ 增益无幅度上限，极端参数产生 Inf/NaN 音频

- **位置**：`crates/seraph-dsp/src/eq.rs:102, 267`
- **代码摘录**：
  ```rust
  let a = 10.0_f32.powf(gain_db / 40.0);   // gain_db 无幅度钳制
  self.preamp_gain = 10.0_f32.powf(preamp_db / 20.0);
  ```
- **风险**：`gain_db`/`preamp` 仅校验 `is_finite()` 未限幅。+240 dB 之类数值会使系数溢出为 Inf → 输出 NaN/Inf 直达 DAC（render 侧 F32 路径只 clamp 有限值，NaN 原样写出）。输入源是用户自己的 DSP 设置（JSON 无法表达 NaN/Inf），属自伤面而非远程攻击面，但劣化听感且可能损坏扬声器。
- **修复建议**：`gain_db.clamp(-24.0, 24.0)`、`preamp_db.clamp(-24.0, 24.0)`；设计完系数后校验全部 `is_finite()`，否则回退 identity。

### 3.8【低】L-7：mockup_archive_lyrics.html 从多个第三方 CDN 加载脚本与样式

- **位置**：`mockup_archive_lyrics.html:7-9, 72`
- **代码摘录**：
  ```html
  <script src="https://cdn.tailwindcss.com"></script>
  <link href="https://fonts.googleapis.com/css2?family=..." rel="stylesheet">
  <link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.4.0/css/all.min.css">
  <img src="https://placehold.co/800x800/111/ddd?text=ALBUM" ...>
  ```
- **风险**：Tailwind Play CDN 官方明确"不得用于生产"。若该文件被误打包进安装包或分发，任一 CDN（tailwind/cdnjs/placehold.co）被投毒即可在该页面上下文执行任意脚本。**已验证缓解**：`vite.config.ts:37-43` 的 rollup 入口只含 `index.html` 与 `taskbar.html`，且 `dist/` 中确认只有这两个 HTML——该 mockup 当前不进入产物，实际暴露面仅限开发者本机。
- **修复建议**：移入 `design/` 目录或删除；至少在文件头加注释声明"设计稿，禁止分发/引用"。

### 3.9【低】L-8：serde_with 3.20.0 存在已知 panic 漏洞（GHSA-7gcf-g7xr-8hxj）

- **位置**：`Cargo.lock`（serde_with 条目，来源为 `tauri-utils` 传递依赖）
- **风险**：`KeyValueMap` 序列化在空序列/空 map 条目时 panic（本地拒绝服务）。CVSS 4.0：`AV:L/AC:L/VC:N/VI:N/VA:L`，修复版本 3.21.0，当前锁定 3.20.0。该 GHSA **无 RUSTSEC 别名**，因此 `cargo audit` 门禁检测不到。实际触发需上游 tauri-utils 启用 `KeyValueMap` feature，风险很低。
- **修复建议**：`cargo update -p serde_with --precise 3.21.0`。

### 3.10【低】L-9：.gitignore 遗漏 `.claude/settings.local.json`

- **位置**：`.gitignore`（全文）；文件本体 `.claude/settings.local.json`
- **风险**：该文件目前靠本机用户级全局 gitignore 忽略，项目 `.gitignore` 本身没有覆盖。文件含本机用户名、绝对路径、AI 工具权限白名单，无密钥泄露，但换机器或其他贡献者克隆后极易意外提交。当前确认未被 git 跟踪，无实际泄露。
- **修复建议**：在 `.gitignore` 增加一行 `.claude/settings.local.json`（或整个 `.claude/`）。

---

## 四、信息级事项（观察项 / 威胁模型明示）

### 4.1 Tauri 后端

1. **主窗口任意路径信任模型（明示项）**：`import_tracks(paths)`/`play(path)`/`save_track_lyrics(track_path)`/`reveal_in_explorer` 接受主窗口传来的任意绝对路径。主窗口渲染进程被攻破时可读取任意音频文件并解析元数据/内嵌封面回显、令播放引擎解码任意文件、弹出 explorer。这是本地音乐播放器的固有功能边界（前端 = 文件系统全权代理），且 CSP 严格、无远程内容加载，攻破前提本身很难成立。`lib.rs:94-104` 的 F-03 拦截已确保攻击面更暴露的 taskbar-lyrics 窗口摸不到这些命令。**建议在文档中明示"主窗口渲染进程 = 全信任"的威胁模型。**
2. **CSP 小配置漂移**：CSP `img-src` 只放 `*.hdslb.com`，而后端 `url_guard::BILIBILI_IMAGE_HOST_SUFFIXES`（`url_guard.rs:9`）还含 `bilibili.com`——若未来前端直接展示 `bilibili.com` 域图片会被 CSP 拦（功能问题）。建议对齐；如追求极致可将 `*.hdslb.com` 收窄为实际用到的 `i0/i1/i2.hdslb.com`。
3. **release 零日志**：tracing 仅 `debug_assertions` 初始化（`lib.rs:23-24`），release 无排障日志——安全性有利，运营上是权衡，可考虑文件级 opt-in 日志开关。
4. **`lyrics.rs` 的 `QRC_KEY`/`KRC_KEY`/`QMC1_PRIVKEY`** 为 QQ 音乐歌词格式的公开协议常量，不属于秘密。
5. **analysis.rs 排序的隐式依赖**（`crates/seraph-visualizer/src/analysis.rs:441`）：`partial_cmp().expect("finite energies")` 依赖"NaN 恰好被上游 `> abs_gate` 过滤"这一巧合。建议改为 `total_cmp` 并在累计处显式跳过非有限值，防止未来重构引入 IPC 线程 panic。
6. **ffmpeg 锁中毒 expect**（`seraph-decoder/src/ffmpeg.rs:523,600`）：持锁区只有 Vec 操作，实际不可达；可改 `unwrap_or_else(|p| p.into_inner())` 彻底消除。
7. **u64→usize 截断**（`seraph-decoder/src/dsd.rs:212,259`）：仅影响 32 位目标且截断方向是"读得更少"，当前产品仅 Windows x64，无实际影响。
8. **symphonia features 收敛建议**：`seraph-decoder/Cargo.toml` 启用 `features = ["all"]`，而 exotic 格式本就设计走 ffmpeg fallback。建议收敛为实际所需（flac/mp3/wav/opus/aac/isomp4/ogg 等），缩小恶意文件的解析器暴露面。
9. **SERAPH_FFMPEG_* 环境变量**可指定子进程路径：本地攻击者若能控制用户环境变量，等价于已控制该用户进程，属可接受设计；仅需确保未来不让不可信内容（如播放列表文件字段）写入这些变量。
10. **seraph-playlist crate** 只有类型与 trait 定义（约 60 行），无 I/O、序列化或路径处理，crate 内无攻击面；真正的歌单持久化在 `src-tauri/src/ipc/playlist_io.rs`（本次已覆盖审查，未见路径遍历问题）。

### 4.2 前端

1. **B 站头像 URL 未经归一化直接进 `<img src>`**（`src/components/pages/main-pages/StreamingPage.tsx:175-179`）：`loginStatus.face` 未走 `coverSrc()` 统一入口。`<img>` 上下文中 `javascript:` 不执行、生产 CSP 会拦截非白名单域（fail-closed），不可利用为 XSS；若 B 站未来返回其他 CDN 域头像会被 CSP 静默拦截（功能问题）。建议改用 `coverSrc()` 统一归一化。
2. **`coverSrc` 放行任意 `data:`/`blob:` URL 未限制 MIME**（`src/lib/tauri.ts:152-160`）：`data:text/html`、`data:image/svg+xml` 也能通过。在 `<img>` 元素中均无法执行脚本，仅为纵深防御缺口。建议白名单收紧为 `/^(https?:|blob:|data:image\/(png|jpe?g|webp|gif))/i`，拒绝 SVG data URL。
3. **曲目元数据注入 CSS 自定义属性**（`src/components/player/AlbumArt.tsx:11-27`）：`glow1/glow2` 经 CSSOM 写入样式属性，恶意值最多造成样式错乱，现代浏览器中 CSS 值无法直接执行 JS。建议渲染前用 `/^#[0-9a-f]{3,8}$/i` 校验，非法回落默认色。
4. **B 站链接/BV 号仅 `trim()` 后直传后端**（`src/store/player/bilibiliActions.ts:93-103,156-170`）：前端无格式/协议预校验。后端已有域白名单（`resolve_bvid` 等），风险有限；建议前端加一层 `^(https?://(www\.)?(bilibili\.com|b23\.tv)/|BV[0-9A-Za-z]{10}|av\d+)` 预校验作为纵深。
5. **localStorage 明文保存收听痕迹**（`persistStorage.ts:45-62`）：`liked`/`userPlaylists`/`recentTrackIds`/续播位置，**不含任何凭据**（B 站 Cookie 在 Rust 侧 Credential Manager）。同机其他进程读取 WebView2 目录可获知收听习惯。可选：提供"清除全部收听痕迹"按钮。
6. **CSP `style-src 'unsafe-inline'`**：React 内联样式的常见妥协，可接受。**`assetProtocol.scope: []`** 为空数组 = fail-closed 起步（最安全取向），封面经 `convertFileSrc` 由 Rust 侧运行时注入 scope。
7. **测试文件中的 `innerHTML`**（`useSpacePlayback.test.ts:33`）：仅测试环境 DOM 清理。**全仓库生产代码 0 处 `innerHTML`/`dangerouslySetInnerHTML`/`insertAdjacentHTML`/`document.write`、0 处 eval/new Function、0 处 window.open、前端零 fetch/XHR/WebSocket（全部网络下沉 Rust）。**

### 4.3 配置与供应链

1. **glib 0.18.5 unsound（RUSTSEC-2024-0429）**：tauri 2 上游锁定 gtk 0.18 栈，Linux-only 依赖，本项目仅产出 Windows 安装包。CI 注释已完整记录决策依据（`ci.yml:60`、`release.yml:72`），属合理豁免；保持跟踪 tauri 上游 gtk 0.20 迁移进度，届时移除 ignore。
2. **18 个 unmaintained crate**（cargo audit warning 级）：gtk-rs 0.18 全家桶（tauri Linux 栈）、derivative/instant/paste/proc-macro-error、unic-* 系列（urlpattern 引入）。均为"停止维护"公告非可利用漏洞，随上游升级自然消除。
3. **CI 注释与 lockfile 小出入**（`ci.yml:41-45`"当前依赖树零安装脚本"）：esbuild@0.28.1 与 fsevents@2.3.3 标记 `hasInstallScript: true`，实际 esbuild ≥0.17 走平台 optionalDependencies 分发、fsevents 在 Windows 不安装，故 `--ignore-scripts` 构建正常（CI 长期绿证明）。建议修正注释表述，防止未来引入真正依赖 install script 的包（如 sharp）时静默产出损坏构建。
4. **git 历史**：91 commits 全历史密钥模式扫描（ghp_/github_pat_/AKIA/私钥/slack token/JWT）零命中；被删的历史审查文档不含凭据。

---

## 五、核验通过项（本次审查确认无问题的领域）

| 检查项 | 结果 |
|--------|------|
| CSP（`tauri.conf.json:34`） | `script-src 'self'`（无 unsafe-inline/eval）、`object-src 'none'`、`frame-src 'none'`、`base-uri 'self'`、connect/img/media 按需收窄；devCSP 的 `localhost:*` 仅限开发构建 |
| 能力配置（capabilities） | 无 `shell:*`、无 `fs:*`、无 `http:*`；主窗口仅 core 窗口五项 + 事件两项 + `dialog:allow-open/save`；歌词条窗口仅事件三项 + 拖拽；与 `gen/schemas/capabilities.json` 交叉核对一致 |
| 无 `dangerousRemoteDomainIpcAccess`、无 devtools feature、`withGlobalTauri` 未启用 | 通过 |
| 窗口级自定义命令白名单 | `lib.rs:94-104,151-172` 中央拦截，taskbar-lyrics 窗口仅放行 9 个无副作用命令，未知窗口全拒；附扫描前端源码交叉核对的回归测试（`lib.rs:230-310`） |
| XSS（前端渲染路径） | 歌词/通知/错误消息全部 React 文本插值，封面经 `coverSrc()` + `convertFileSrc`，生产代码零 innerHTML 类 API |
| 命令注入 | 所有 `Command::new` 参数走数组传递不经 shell；icacls/explorer 走 `%SystemRoot%\System32` 绝对路径（F-05）；explorer `/select` raw_arg 引号包裹；子进程 `CREATE_NO_WINDOW` |
| ffmpeg 子进程参数注入 | `-` 前缀路径加 `file:` 协议前缀（`ffmpeg.rs:329-338`）；ffprobe 20 s 超时；stderr 4 KB 限额 |
| TLS | reqwest 0.13.4 默认 rustls，全库无 `danger_accept_invalid_certs`，所有出站 URL 强制 https + 白名单 |
| URL 防线 | `url_guard.rs`：https + 无 userinfo + host 后缀白名单单一事实来源，处理后缀伪装/userinfo 混淆，附完整单测 |
| 重定向逐跳复验 | `http_util.rs:56-68` 下载/头像/封面三个 client 已挂（本次新发现 3 处遗漏见 L-3） |
| B 站凭据 | Cookie 存 Windows Credential Manager；session 写入前先收窄 ACL（S-11）；`Set-Cookie` 按名称白名单落盘（S-05）；短链/头像用无 Cookie 裸 client（P0-1/S-02）；下载 URL 强制 https + B 站 CDN 域（M-3） |
| ffmpeg 下载链 | 版本 + SHA-256 双源锚定（gyan.dev + GitHub mirror，S-01）；zip 条目数/解压体积在真实流上 `take` 硬截断（S-06）；并发槽防交错写 |
| 缓存目录安全 | `.seraph-cache` marker 机制防接管已有目录；删除前强制 marker 在盘校验（M-5）；symlink/junction 跳过；ProgramData 候选降级（S-14） |
| 数据完整性 | 曲库/设置/session 全部 temp+rename 原子写；曲库损坏时备份 `.corrupt` 拒绝覆盖写（P0-2） |
| M3U8 导出注入 | CR/LF/U+2028/2029 归一、含换行路径条目丢弃、`#` 开头 `./` 消歧（N-04，附回归测试） |
| 输入大小上限 | JSON 8MB、歌词 4MB、HTML 1MB、头像 512KB、M3U8 32MB/万条、ffmpeg 包 400MB/单条 512MB/4096 条目、音频 1.5GB、zlib 解压 8MB、ID3 32MB、目录递归 64 层 |
| 数值入参校验 | seek/volume/ratio 拒 NaN/Inf（`playback.rs:115/141`）；EQ 数据 NaN/Inf/越界全路径拦截（前端 `eqSanitize.ts` + 后端） |
| panic/DoS 面 | 非测试代码仅 3 处 unwrap/expect 且不变量成立；阻塞 IO 走 `spawn_blocking` 且 JoinError 统一转 IpcError；引擎 open/probe/解码 worker 均有 `catch_unwind` 兜底；音频回调无锁无分配 |
| crates unsafe | **全 crates 树零手写 unsafe**（src-tauri 的 unsafe 限于 Win32/ Credential API 且 HWND 来自自身窗口、`CredFree` 路径正确、`CredentialBlob` 判空后再 `from_raw_parts`） |
| DSD/解码器健壮性 | chunk 回绕/死循环防护、截断短读防护、文件大小交叉校验、seek 饱和乘法，每个防护点有恶意样本回归测试 |
| 历史漏洞 crate | plist 1.10.0、quick-xml 0.41.0、quinn-proto 0.11.16、anyhow 1.0.104、event-listener、rustls 0.23.40、ring 0.17.14、idna 1.1.0、tauri 2.11.2——OSV 零命中 |
| npm audit（含 dev，low 阈值） | **0 漏洞**（125 prod / 181 dev）；运行时依赖仅 12 个主流包；lockfile 全部带 integrity 哈希、无 git/http URL 依赖 |
| CI 供应链 | Action 全量 SHA 锁定且经 GitHub API 逐一比对真实有效；`npm ci --ignore-scripts`；最小权限（contents:read / 按需 write）；双层 audit 门禁先于构建；无 `pull_request_target` 滥用；secrets 仅 `GITHUB_TOKEN` |
| 硬编码密钥 | 源码 + 全部根文件 + 文档 + git 全历史：token/JWT/私钥/AWS Key/AI Key 模式零命中；磁盘无 .env 文件 |
| 更新检查 | GitHub API 响应 capped 读取；`open_release_page` URL 白名单解析级校验（拒 userinfo/端口/点段穿越/`%2e` 变体，附测试）；不做自动下载安装 |

---

## 六、做得好的地方（值得保持的工程实践）

1. **防线集中且编号可追溯**：`path_guard`/`url_guard`/`http_util` 单一事实来源 + S-xx/F-xx/N-xx/P-xx/审2-x 修复编号贯穿代码注释与 git 历史，回归测试锚定每个修复点。
2. **扫描前端源码交叉核对窗口命令白名单的回归测试**（`lib.rs:230-310`）——这个测试设计本身就是最佳实践。
3. **多窗口事件安全架构**：`emitTo` 定向而非全局 `emit`，配合 capability 从架构上阻止第二窗口伪造/广播事件。
4. **配置导入纵深防御**：2MB 上限前后端口径一致、kind/版本校验、逐字段类型校验、仅设置字段白名单合并、个人数据不受导入覆盖。
5. **前端零网络面**：所有外部请求（歌词/封面/B 站/更新/ffmpeg 下载）下沉 Rust 后端，XSS 即使发生也无凭据可窃。
6. **隐私自觉**：在线歌词弹窗明示数据外发目标；"关闭记忆播放"立即清除已持久化的续播位置。
7. **实时音频工程纪律**：回调无锁无分配无系统调用、频谱 tap `try_lock` 失败即放弃、seek 代际过滤丢弃陈旧样本。

---

## 七、修复优先级建议

| 优先级 | 项 | 预估改动 |
|--------|-----|----------|
| P1（下个版本） | M-1 ffmpeg 裸名回退（§3.1） | ~5 行 |
| P1（顺手） | L-8 serde_with 升级 3.21.0、L-9 .gitignore 补一行 | 2 行 |
| P2 | L-1 SMTC UNC 拒绝（§3.2）、L-3 三处 client 挂 `guarded_redirect_policy`（§3.4） | < 60 行，与既有防线风格一致 |
| P2 | L-4 缓冲动态折减（§3.5）、L-5 clamp 防御（§3.6）、L-6 增益限幅（§3.7） | < 40 行 |
| P3 | L-2 导出路径收窄或 dialog token（§3.3）、L-7 mockup 文件迁移（§3.8） | 视方案而定 |
| P3 | 信息级：coverSrc data: 收紧、face 归一化、symphonia features 收敛、total_cmp、CI 注释修正 | 各几行 |

**结论**：无严重/高危发现，1 中危 + 9 低危，全部修复成本可控。建议保持"新增网络调用/文件命令必须走公共防线（`url_guard`/`http_util`/`path_guard`）"的既有纪律——本次发现的三处重定向白名单遗漏正是该纪律执行不彻底的遗留点。

---

## 附录：审查方法说明

- Tauri 后端：55 个 `#[tauri::command]` 逐一人工审计；capabilities 与 `gen/schemas/capabilities.json` 交叉核对。
- 前端：23 处 `invoke` 调用点逐一追踪参数来源；grep 覆盖 innerHTML/eval/fetch/window.open/postMessage/localStorage/密钥模式。
- crates：29 个文件全读；重点审计解码器（symphonia/dsd/ffmpeg 三路径）、`validate_stream_info` 边界闸、rtrb 环形缓冲上限、DSP 系数生成。
- 供应链：`cargo audit` 因本机网络无法克隆 RustSec advisory-db 失败，改用 OSV.dev API 对 Cargo.lock 全部 666 个 crate 逐一在线核验（含严重度、修复版本、别名）；`npm audit --audit-level=low` 本地实际执行（0 漏洞）；CI Action SHA 经 GitHub API tag→SHA 解析逐一比对；git 全历史（91 commits）密钥模式扫描。
