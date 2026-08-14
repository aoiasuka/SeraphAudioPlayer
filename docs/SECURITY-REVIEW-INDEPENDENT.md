# Seraph Audio Player 独立安全复审报告(v0.5.6 代码基线)

> 本报告是一次**独立**复审:未参考 docs/ 下任何既有审查报告,结论全部来自对
> `src/`(前端)、`src-tauri/`(后端 IPC 与壳)、`crates/`(Rust 工作区)、
> `.github/` + 构建配置的直接审计,并对关键结论做了依赖源码级交叉验证
> (本机 cargo registry 中的 reqwest 0.13.4、rtrb 0.3.4、symphonia 0.5.5)。
> 审查为纯只读,未改动任何代码。

## 审查方式

- 四路并行独立审计:前端 `src/`、后端 `src-tauri/`、Rust `crates/`、CI/构建/供应链;
- 主审逐文件复核全部高危链路:FFmpeg 下载执行链、HTTP/URL 白名单、凭据存储、
  capabilities/CSP、子进程、缓存清理、封面/歌词/DSD/ID3 解析、前端 XSS sink;
- 对子代理的关键断言做源码级核实并修正(下文中注明);
- 遗留事项见文末(5 个 Action SHA 的人工核验)。

## 总览

| 级别 | 数量 | 编号 |
|---|---|---|
| 严重 | 1 | R-01(单个恶意音频文件 → 整个进程 abort) |
| 中 | 8 | F-01(修正后)、F-02、F-03、R-03、S-01、S-02、S-03、W-01 |
| 低/加固 | 26 | F-04..F-08、R-02、R-04..R-07、W-02..W-08、S-04..S-11 |

总体判断:既有安全基线扎实——FFmpeg 下载链(SHA-256 双官方源锚定)、URL 逐要素
白名单、响应体全局上限、Cookie 只进 Windows Credential Manager、ACL 先建空文件
再收窄、生产 CSP 无 localhost 通配、capabilities 最小化、GitHub Action 全 SHA
锁定,全部经逐行复核。剩余问题集中在三个残余面:**不可信文件头的数值边界**
(R-01)、**重定向最终 host 复验覆盖不完整**(F-01/F-04/F-07)、**纵深防御缺口**
(F-02/F-03)。

---

## 严重

### R-01 | 严重 | `crates/seraph-audio/src/engine.rs:432,696` + `crates/seraph-decoder/src/symphonia.rs:331,348-366` | 文件头采样率无上界 → ~103 GB 环形缓冲预分配 → 进程 abort(DoS)

**攻击路径**(每个环节均以源码验证):

1. 构造 fmt 头 `sample_rate = 0xFFFFFFFF` 的最小 WAV。symphonia riff demuxer
   对采样率**零校验原样读取**(registry 源码
   `symphonia-format-riff-0.5.5/src/wave/chunks.rs:378`:
   `let sample_rate = reader.read_u32()?;`),PCM codec 只要求字段存在
   (同源 `codec-pcm:381`)。
2. `stream_info_from_codec` 无上界钳制,4.29e9 直接进入 `StreamInfo.sample_rate`;
   ALAC magic cookie 路径同样只查 `> 0`(`symphonia.rs:331`);ffmpeg 兜底的
   `parse_ffprobe_output` 同样只查 `> 0`(`ffmpeg.rs:394-399`)。
3. WASAPI 独占驱动把文件采样率直接作为输出率(`engine.rs:881-897`)。注意
   `Engine` 的**默认驱动就是 WasapiExclusive**(`engine.rs:334`),设置里选
   WASAPI 即命中(前端 "direct" 映射 Shared、迁移逻辑把 "usb" 迁到 "wasapi")。
4. `max_buffer_samples = output_rate × channels × 3` = 4294967295 × 2 × 3
   ≈ 2.58e10(`engine.rs:696`),随后 `RingBuffer::new` 在 WASAPI 流创建**之前**
   执行(`engine.rs:432` 早于 `445`)。
5. rtrb 0.3.4 的 `RingBuffer::new` 无任何容量断言,直接
   `Vec::with_capacity(capacity)`(registry 源码 `rtrb-0.3.4/src/lib.rs:138-142`)。
   缓冲区元素为 `f32`(4 字节),分配量 ≈ 2.58e10 × 4 ≈ **103 GB**,分配失败 →
   `handle_alloc_error` → **整个进程 abort**;`catch_unwind`(`engine.rs:404`)
   只兜 panic、兜不住 abort,主窗口与歌词条一起崩溃。
   (早期口径「206 GB / ×8 字节」不准确,已按 rtrb 实际元素类型修正;不影响结论。)

**修复建议**:

- 在解码边界统一钳制 `sample_rate ∈ [8_000, 768_000]`、`channels ∈ [1, 32]`
  (放进 `stream_info_from_codec`、`parse_ffprobe_output`、`apply_alac_cookie_overrides`
  三处);
- `PlaybackShared::new` 的 `max_buffer_samples` 设硬上限(如 32M 样本)作纵深防线;
- `validate_dsd_header` 的 100M 上界收紧到 768k 同口径。

---

## 中危

### F-01 | 中 | `src-tauri/src/ipc/bilibili/import_audio.rs:575-587` | 音频下载重定向后不复验最终 host

> 原始子代理结论「reqwest 跨域重定向不清 Cookie 头、SESSDATA 外泄」**不成立**,
> 已修正。查 vendored reqwest 0.13.4 源码(`redirect.rs:239-252`):任何跨 host /
> 跨端口 / 跨 scheme(含同 host 降级 http)的重定向都会剥离
> `Cookie`/`cookie2`/`Authorization`,并有对应回归测试(`redirect.rs:437-452`)。
> SESSDATA 不会被带出 B 站 CDN 域。

残余有效问题:重定向目标内容仍会被**当作音频下载入缓存并交给解码器**——即
无凭据的 SSRF 探测 + 下载物不受白名单约束;下载 client 无总超时,重定向目标
可长期挂起连接。

修复建议:下载 client 挂 `redirect::Policy::custom`,逐跳用
`is_safe_bilibili_download_url` 复验目标 URL(与 `resolve_bvid` 同口径)。

### F-02 | 中 | `ipc/config.rs`、`ipc/dsp.rs`、`ipc/playlist_io.rs` | 文件读写 IPC 接受任意路径、无来源/目录约束

- `export_app_config(path, content)` 可把任意内容写到任意 `.json` 路径;
  `import_app_config` / `import_eq_preset` 可读任意文件(≤2 MB / 512 KB)并把
  内容回传调用方;`export_playlist_m3u8` 写任意 `.m3u8`。正常流程路径来自
  dialog,但命令边界不校验——一旦任一渲染进程被供应链/XSS 攻破(当前前端无
  HTML sink,属纵深防御缺口),即为任意文件读写(可覆盖 `library-cache.json`、
  `cache-settings.json` 或用户文档)。

修复建议:写命令做目录白名单 / 与 dialog 选择结果绑定;读命令限制扩展名与
父目录。

### F-03 | 中→低 | `src-tauri/src/lib.rs:29-85` + `capabilities/taskbar-lyrics.json` | 自定义 IPC 命令未按窗口隔离

Tauri v2 的 capabilities 只约束 core/插件命令,56 个自定义命令对主窗口和
歌词条窗口**一律开放**。歌词条窗口加载独立前端(渲染外部歌词/元数据),一旦其
渲染面被攻破即可调用任意文件读写命令。歌词条的 `allow-emit-to` 收窄方向正确,
但管不到自定义命令。

修复建议:敏感命令(文件读写、设备、播放控制)内校验 `window.label() == "main"`,
或按窗口拆分 handler 注册表。

### R-03 | 中 | `crates/seraph-decoder/src/ffmpeg.rs:341-356,229-238` + `engine.rs:672-674` | ffprobe/ffmpeg 子进程与管道读无超时

- `probe_with_ffprobe` 用 `.output()` 同步无限等待;`next_packet` 对 stdout
  阻塞 `read`;解码线程卡死时 `stop_session` 的 `worker.join()` 无限阻塞 →
  引擎线程死亡,所有播放命令失效,只能重启应用。触发条件:让 ffprobe 解析挂起
  的畸形文件,或断连网络盘上的慢文件(网络盘单次探测可挂数秒是已记录的现实场景)。

修复建议:ffprobe 加超时 kill;解码管道读改带超时轮询;`join` 加时限,超时
kill 子进程并 detach。

### S-01 | 中 | `.github/workflows/ci.yml:8-9,42` | 任意 PR 可在 CI runner 执行代码

`pull_request` 事件执行 PR merge commit 内的 workflow;fork PR 可改写 ci.yml,
或改 lockfile 引入带 `postinstall` 的恶意包(`npm ci` 未加 `--ignore-scripts`)。
令牌只读、CI 无 secrets,实际影响限于 runner 资源滥用 + 缓存投毒(缓存键含
lockfile 哈希,投毒面有限)。

修复建议:`npm ci --ignore-scripts`(当前依赖树零安装脚本,零成本);lockfile
变更 PR 人工复核。

### S-02 | 中 | `.github/workflows/release.yml:3-10` | tag 触发执行 tag 内快照的旧 workflow

`push: tags` 执行的是**被推 tag 所在提交**的 workflow 文件。加固前的 release.yml
(4a910c4^)全部是浮动 action tag 且无 cargo audit 门禁——有写权限者把 `v*` tag
打到旧提交,即用 `contents: write` 令牌跑旧门禁;浮动 tag 若被上游移动即成恶意
action 代码执行。

修复建议:GitHub tag protection rules;发布 SOP 固化「tag 必须指向含最新
hardened workflow 的提交」。

### S-03 | 中 | `src-tauri/tauri.conf.json:52-60` + `release.yml:64-73` | 安装包未签名、Release 无校验和资产

无 Authenticode 签名(无 `certificateThumbprint`/`signCommand`)、无 `.sha256`
资产(tauri-action 未设 `createUpdaterArtifacts`),用户无法验证安装包完整性;
维护者账号被攻破后可直接发布恶意二进制。

修复建议:接入代码签名(Azure Trusted Signing 或自备证书);上传 checksums 资产。

### W-01 | 中 | `src/components/ui/TypewriterText.tsx:14-29` + `src/components/sidebar/LyricsPanel.tsx` | 歌词行数/单行长度无上限 → UI 冻结

后端 `save_track_lyrics` 只有 4 MB 总字节上限、无行数/单行上限;恶意 LRC 单行
近 2 MB 时,TypewriterText 按 30 ms/字符逐字 `slice`(每次 O(n) 重建)可持续卡死;
数万行时 LyricsPanel 一次性渲染数万 `<p>`(已核实 `.map` 全量渲染、无虚拟化)。

修复建议:后端解析时限制单行 ≤512 字符、总行数 ≤5000;前端超长文本禁用打字机
动画。

---

## 低危 / 加固项

### 后端(`src-tauri`)

- **F-04** `ipc/library/online_covers.rs:152-165`:iTunes `artworkUrl100` 未校验
  scheme/host、重定向不复验(无 Cookie、4 MB 截断,SSRF 探针级)。建议校验
  `https` + `*.mzstatic.com` 白名单 + 逐跳复验。
- **F-05** `ipc/system.rs:59`、`ipc/update.rs:109`、`ipc/bilibili/session.rs:333`:
  `icacls`/`explorer` 按裸名 `Command::new` 解析,CreateProcess 搜索顺序含进程
  CWD——应用从共享/下载目录启动时存在同名 EXE 种植面。建议改用
  `System32\icacls.exe` / `explorer.exe` 绝对路径。
- **F-06** `ipc/library/media_library.rs:1141`:同目录外部 `.lrc` 读取无大小上限
  (与 4 MB IPC 口径不一致),共享目录放 GB 级 `.lrc` 可致导入时 OOM。建议复用
  `read_bytes_capped` 或 `metadata.len()` 预检。
- **F-07** `ipc/bilibili/import_audio.rs:673-679`:头像下载重定向后不复验最终
  host(裸 client 无 Cookie,低危)。
- **F-08** `ipc/bilibili/ffmpeg.rs:252-282`:ffmpeg zip 解压无条目数上限。
  注:zip 内容被 SHA-256 硬锚定在二进制内,此点纯防御性。

### Rust crates

- **R-02**(原判中,已降级)`seraph-decoder/src/dsd.rs:291,295`:seek 乘法
  `block_index × block_size × channels` 未 checked——release 回绕后被
  `min(data_len)` 钳制、行为安全,debug 才 panic。建议 `saturating_mul`。
- **R-04** `seraph-decoder/src/ffmpeg.rs:535-561`:ffmpeg/ffprobe 解析信任
  `SERAPH_FFMPEG_PATH`/PATH 且只查存在性、无哈希复核(同权限攻击模型,低危)。
- **R-05** `seraph-decoder/src/symphonia.rs:331`:ALAC cookie 采样率仅 `> 0`
  校验——与 R-01 同源,修复时一并钳制。
- **R-06** `seraph-core/src/bus.rs:34-37`:EventBus 无界通道,订阅者不消费即
  累积(现有订阅者均持续消费,低)。
- **R-07** `seraph-audio/src/wasapi.rs:103`:按采样率算 `sync_channel` 容量——
  当前死代码不可达,复用前必须加上限或删除。

### 前端(`src`)

- **W-02** `src/lib/configTransfer.ts:185-194` + `src/boot/applyConfigImport.ts`:
  boot 首 import 裸 `localStorage.setItem` 无 try/catch,配额超限可致单次启动白屏。
- **W-03** `src/store/player.ts:109`:对 `userPlaylists` 仅数组级校验,元素结构坏
  可在渲染期 TypeError(`item.trackIds.includes(...)`)。
- **W-04**(已修正评级)`src/lib/tauri.ts:152-160`:`coverSrc` 放行任意本地路径转
  asset——但 asset scope 已只放开 `app_data/covers` 非递归目录(`lib.rs:91-93`),
  实际暴露面被压到该目录内的存在性探测,信息级。
- **W-05** `src/components/player/AlbumArt.tsx:11-28`:`glow1/glow2` 未做
  `#rrggbb` 格式校验(当前 CSSOM 会拒恶意值,不可达,预防性)。
- **W-06** `src/lib/eqApoParser.ts:143`:`bands.push(...graphicBands)` 展开超大
  数组可 RangeError(文件 512 KB 上限下约 5.5 万点,接近 V8 参数上限)。
- **W-07** `src/lib/configTransfer.ts:120-150`:配置导入 JSON 无体积上限
  (2 MB 后端上限 + 双份复制,低)。
- **W-08** `src/components/pages/main-pages/StreamingPage.tsx:174-179`:登录头像
  `face` 前端零复核(后端已有域白名单,预防性)。

### 供应链 / 构建

- **S-04** `package.json:18-52`:依赖全 `^` 范围(lockfile 兜底,PR 可改 lockfile)。
- **S-05** `scripts/bump-version.mjs:25-46`:正则替换第一个 `"version"` 字段可能
  改错位置;lock 刷新可能漂移;版本号仅正则校验(`01.2.3` 可通过)。
- **S-06** `vite.config.ts:7,20-27`:`TAURI_DEV_HOST` 可扩大 dev server 暴露面
  (生产不受影响)。
- **S-07** `crates/seraph-decoder/Cargo.toml:11`:symphonia `features = ["all"]`
  全格式解析面,建议按实际格式白名单收窄(flac/mp3/aac/alac/wav/pcm/isomp4)。
- **S-08** `release.yml:69-71`:无「tag 版本 ↔ package.json 版本」一致性校验。
- **S-09** `ci.yml:45` vs `release.yml:49`:npm audit 阈值不一致
  (CI moderate / release low),建议统一为 low。
- **S-10** `.github/`:无 dependabot / CodeQL 自动化。
- **S-11** `tauri.conf.json:56-59`:NSIS 默认 downloadBootstrapper,安装期联网
  下载 WebView2。

---

## 已核实安全面(抽查通过)

- **FFmpeg 下载执行链**:固定版本 + 双官方源 + 官方 SHA-256 锚定、流式 400 MB
  上限、`Read::take` 硬截断解压、只提取固定 basename 无 zip-slip、原子写。
- **URL 白名单**:`is_allowed_release_url` 逐要素比对且拒绝残余点段;
  `is_safe_bilibili_download_url` / `is_safe_avatar_url` https + 域后缀;
  `resolve_bvid` 重定向后复验最终 host。
- **凭据**:Cookie 名称白名单 `BILIBILI_COOKIE_ALLOWLIST`;本体只进 Credential
  Manager;session 文件先建空文件收 ACL 再写内容;文件不含 Cookie。
- **配置面**:生产 CSP 无 localhost 通配、`object-src 'none'`、无 devtools 生产
  开启、无 `withGlobalTauri`、无 updater 插件、asset scope 仅 covers 目录。
- **子进程**:全部 `Command::arg` 直传无 shell;`-` 前缀路径 `file:` 消歧;reveal
  路径校验存在性。
- **前端**:全仓零 `dangerouslySetInnerHTML` / `eval` / `window.open` / 动态跳转;
  歌词与元数据全走 React 转义;三个 persist store 均有水合消毒;eqSanitize 全套
  钳制。
- **Rust**:`crates/` 全 workspace **0 处 unsafe**;dsd/wav_id3 解析全 checked
  算术 + 上限;WAV ID3 帧大小启发式与边界落点校验。
- **CI**:Action 全 SHA 锁定、CI 令牌只读、release 仅 `contents: write`、双
  audit 门禁先于构建。

## 修复优先级建议

1. **R-01**(一行边界函数即可止血,单文件可崩全应用)
2. **F-01** 下载重定向复验 + **R-03** 子进程超时
3. **S-02 / S-03** 发布链(tag protection + 代码签名)
4. **F-02 / F-03** 窗口/路径约束(纵深防御)
5. **W-01** 歌词解析上限 + 前端动画退化
6. 其余低危项按排期消化(S-07 收窄 symphonia features 是性价比最高的攻击面缩减)

## 遗留人工核验事项

- 5 个锁定的 Action SHA 已确认「40 位十六进制、仓库名与注释一致」的锁定形式,
  但 SHA 与注释版本号的对应关系未完成联网核验(审查环境无 GitHub API 访问)。
  打开 `github.com/<repo>/commit/<sha>` 确认提交属于对应仓库即可:

| Action | 注释声称 | SHA |
|---|---|---|
| actions/checkout | v5 | `fbc6f3992d24b796d5a048ff273f7fcc4a7b6c09` |
| actions/setup-node | v5 | `a0853c24544627f65ddf259abe73b1d18a591444` |
| dtolnay/rust-toolchain | stable | `4360b52568e2003a75bf9bc1d59f33a8e3fc893c` |
| Swatinem/rust-cache | v2 | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` |
| tauri-apps/tauri-action | v0 | `84b9d35b5fc46c1e45415bdb6144030364f7ebc5` |

风险面很小:即使 SHA 与注释版本不符,它仍是该仓库的某个具体提交,不等同于浮动
tag 被移动的攻击面;真正的安全属性(无浮动 tag、双 audit 门禁、CI 令牌只读)已
从 workflow 文件本身核实。
