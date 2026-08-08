# Seraph Audio Player 代码安全审查报告

- 审查日期：2026-08-05
- 审查对象：`D:\_SeraphAudioPlayer`（v0.5.5）
- 技术栈：Tauri 2 + React 18 + TypeScript + Rust（workspace crates）+ Windows 原生集成（WASAPI / SMTC / 任务栏）
- 审查方式：静态代码审查（前端、Rust 后端、IPC、能力配置、CSP、CI/CD）+ `npm audit` 依赖审计
- 审查范围：`src/`、`src-tauri/`、`crates/`、`scripts/`、`.github/workflows/`、`package.json`、`Cargo.toml`、`tauri.conf.json`
- 未覆盖：动态渗透测试、模糊测试、Rust 依赖漏洞库审计（本机未安装 `cargo-audit`）、安装包逆向验证

## 一、总体结论

项目整体安全水平良好，未发现可直接远程利用的高危漏洞。后端在多个关键边界上做了明显的安全设计：

- 严格限定 B 站下载域名白名单，防止登录 Cookie 被带到第三方/明文地址；
- B 站登录 Cookie 存放在 Windows Credential Manager，落盘文件做权限收窄；
- 文件导入、缓存清理、封面 GC 等路径有递归深度、符号链接、目录 marker 等多层防护；
- 歌词/封面/下载响应普遍有体积上限，zlib 解压有炸弹防护；
- IPC 数值入参（音量、seek 等）有 NaN/Infinity/负数校验；
- 前端无 `dangerouslySetInnerHTML`、`eval`、`window.open` 等危险 API，React 默认转义外部文本；
- CSP 严格（`script-src 'self'`、`object-src 'none'`、`frame-src 'none'`），asset 协议运行时只放行 covers 目录。

主要风险集中在**供应链完整性与外部输入边界**：FFmpeg 下载物无完整性校验、多处外部 JSON 响应无体积上限、B 站头像 URL 未校验即携带 Cookie 请求、Release URL 前缀校验可被绕过、dev 依赖存在一个高危漏洞。

共发现 **1 项高危、7 项中危、7 项低危**（详见下表）。

## 二、问题汇总

| 编号 | 级别 | 问题 | 位置 |
| --- | --- | --- | --- |
| S-01 | 高 | FFmpeg 下载包无哈希/签名校验，下载后直接执行 | `src-tauri/src/ipc/bilibili/ffmpeg.rs`、`constants.rs` |
| S-02 | 中 | 头像 URL 未校验即用携带 Cookie 的客户端请求 | `bilibili/commands.rs:230`、`bilibili/import_audio.rs:647` |
| S-03 | 中 | 外部 JSON 响应整体读入内存，无字节上限 | `import_audio.rs:419`、`online_lyrics.rs`、`online_covers.rs`、`update.rs:52` |
| S-04 | 中 | Release URL 仅用 `starts_with` 校验，可被 `releases.evil.com` 等绕过 | `ipc/update.rs:70` |
| S-05 | 中 | Set-Cookie 解析未校验 Domain/Path/Secure 等属性，任意允许域可覆盖 Cookie | `bilibili/session.rs:342` |
| S-06 | 中 | zip 解压单条目无解压后体积上限，存在磁盘耗尽风险 | `bilibili/ffmpeg.rs:200` |
| S-07 | 中 | 音频内嵌封面无大小上限（lofty 路径），恶意音频可撑爆内存/磁盘 | `library/media_library.rs:760`、`811` |
| S-08 | 中 | dev 依赖 undici 存在 1 个高危漏洞（仅测试链路） | `package-lock.json`（jsdom → undici 7.28.0） |
| S-09 | 低 | CSP `connect-src` 含 `http://localhost:*` / `ws://localhost:*` 通配 | `src-tauri/tauri.conf.json:34` |
| S-10 | 低 | `CredReadW` 对空 blob 未判空，`from_raw_parts` 存在理论 UB | `bilibili/session.rs:199` |
| S-11 | 低 | session 临时文件先以默认权限写入、后收窄权限，存在短暂暴露窗口 | `bilibili/session.rs:113` |
| S-12 | 低 | 任务栏窗口被授予 `core:event:allow-emit`，可向任意窗口广播事件 | `capabilities/taskbar-lyrics.json:9` |
| S-13 | 低 | 发布产物未配置代码签名；CI 无 Rust 依赖审计；Actions 未按 SHA 锁定 | `tauri.conf.json`、`ci.yml`、`release.yml` |
| S-14 | 低 | 默认缓存目录候选包含 ProgramData，存在多用户共享目录风险 | `ipc/cache.rs:438` |
| S-15 | 低 | b23.tv 短链解析重定向后未复验最终域名 | `bilibili/import_audio.rs:131` |

## 三、问题详情与修复建议

### S-01（高）FFmpeg 下载包无完整性校验

**位置**：`src-tauri/src/ipc/bilibili/ffmpeg.rs`（下载与解压）、`src-tauri/src/ipc/bilibili/constants.rs:30`

应用会从 `gyan.dev` / GitHub Releases 下载 ffmpeg/ffprobe 压缩包，解压后直接由解码器 `Command::new` 启动。下载走 HTTPS，但没有校验 SHA-256、PGP 签名或 Authenticode 签名。一旦上游被攻破、TLS 被中间人（证书校验异常时）、或 GitHub/Gyan 账号被盗，恶意二进制即可在用户机器上执行。

**建议**：
1. 固定已知构建版本并携带发布方提供的校验和文件，下载后先验 SHA-256 再解压；
2. 解压后对 `ffmpeg.exe` / `ffprobe.exe` 做 Windows Authenticode 签名校验（`Get-AuthenticodeSignature` 或 `signtool verify`）；
3. 更稳妥的方案是随安装包捆绑 FFmpeg 并纳入安装器签名，而不是运行时下载。

### S-02（中）头像 URL 未校验即携带 Cookie 请求

**位置**：`src-tauri/src/ipc/bilibili/commands.rs:230` → `src-tauri/src/ipc/bilibili/import_audio.rs:647`

`bilibili_login_status` 拿到 B 站 API 返回的 `face` 字段后，直接用它构造 `client.get(&url).send()`，而这个 client 携带 `SESSDATA` 等登录 Cookie。`face` 属于外部输入：如果 B 站 API 被攻破、返回恶意 URL，或 URL 发生跨主机重定向，Cookie 可能被发往非预期主机。虽然 `reqwest` 对跨主机重定向有敏感头剥离的默认行为，但不应依赖该隐式行为。

**建议**：
1. 头像请求改用**不携带 Cookie 的裸 client**（头像不需要登录态）；
2. 对头像 URL 复用 `is_safe_bilibili_download_url` 一类的白名单校验（仅 `https` + `hdslb.com` / `bilibili.com` 系域名）；
3. 显式限制重定向（如 `Policy::limited(5)`）并在最终 URL 上复验域名。

### S-03（中）外部 JSON 响应无体积上限

**位置**：
- `src-tauri/src/ipc/bilibili/import_audio.rs:419`（`parse_json_response` 直接 `response.bytes()`）
- `src-tauri/src/ipc/library/online_lyrics.rs`（85、117、163、199、252、284 行多处 `.json::<Value>()`）
- `src-tauri/src/ipc/library/online_covers.rs:98,142`
- `src-tauri/src/ipc/update.rs:52`

所有外部 HTTP 响应的 JSON 都整体读入内存后再反序列化。正常响应很小，但若上游被攻破、CDN 异常或返回超大响应，可导致内存暴涨（HTTP 响应无界）。

**建议**：对每个外部响应先走 `read_bytes_capped`（项目已有该工具）设置合理上限（如 8–16 MB），再用 `serde_json::from_slice` 解析。

### S-04（中）Release URL 前缀校验可被绕过

**位置**：`src-tauri/src/ipc/update.rs:70`

```rust
if !url.starts_with("https://github.com/aoiasuka/SeraphAudioPlayer/releases") { ... }
```

`starts_with` 不校验边界：`https://github.com/aoiasuka/SeraphAudioPlayer/releases.evil.com/...` 也会通过校验，随后交给 `explorer.exe` 打开，可被用来诱导用户访问钓鱼页面。当前 `html_url` 来自 GitHub API（攻击面较小），但校验本身是脆弱的。

**建议**：用 `reqwest::Url::parse` 解析后校验 `scheme == https`、`host == github.com`、`path` 精确以 `/aoiasuka/SeraphAudioPlayer/releases` 开头，并拒绝 userinfo/query 注入；或直接改为在代码中拼接固定 URL，不信任 API 返回的 `html_url`。

### S-05（中）Set-Cookie 解析未校验 Cookie 属性

**位置**：`src-tauri/src/ipc/bilibili/session.rs:342`

`merge_set_cookie_headers` 手工解析 Set-Cookie，只保留名称/值/过期时间，完全忽略 `Domain`、`Path`、`Secure`、`HttpOnly`。后果是：任意被允许访问的 B 站子域返回的 Set-Cookie 都能覆盖整个 Cookie 集合（包括 `SESSDATA`），且所有 Cookie 在后续请求中发给所有白名单域名，忽略域名/路径约束。

**建议**：
1. 只持久化明确需要的 Cookie 名单（如 `SESSDATA`、`bili_jct`、`DedeUserID`、`DedeUserID__ckMd5`、`sid` 等）；
2. 校验 `Domain` 必须是 `bilibili.com` 或其子域、`Path=/`；
3. 登录态 Cookie 必须带 `Secure` 才允许保存；
4. 更简单可靠的做法是使用成熟的 cookie crate（如 `cookie_store`）。

### S-06（中）zip 解压单条目无解压后体积上限

**位置**：`src-tauri/src/ipc/bilibili/ffmpeg.rs:200`（`extract_ffmpeg_tools`）

下载压缩包整体有 400 MB 上限，但 `std::io::copy(&mut entry, &mut out_file)` 不检查单个条目解压后的大小。恶意/异常的压缩包可以构造“压缩后很小、解压后数十 GB”的条目，把磁盘写满（zip bomb 的变体）。

**建议**：解压前检查 `entry.size()`，拒绝超过阈值（如 256 MB）的条目；同时累计已解压总量并设置上限。

### S-07（中）内嵌封面无大小上限

**位置**：`src-tauri/src/ipc/library/media_library.rs:760`（`cover_art_from_tags`）、`811`（`save_cover_art`）

对 FLAC/MP3 等格式，lofty 解析出的 APIC 图片数据直接 `to_vec()` 到内存并落盘，没有体积校验（WAV/DSD 的兜底解析已有 32 MB 限制，但 lofty 主路径没有）。恶意音频文件可内嵌超大封面，导入时撑爆内存/磁盘。

**建议**：在 `cover_art_from_tags` / `save_cover_art` 入口统一校验 `picture.data().len()`，超过阈值（建议 20–32 MB）直接丢弃并告警。

### S-08（中）dev 依赖 undici 高危漏洞

**来源**：`npm audit`（2026-08-05 实测）

```text
undici 7.0.0 - 7.28.0
high severity（响应走私/CRLF 注入/信息泄露等多项公告）
fix available via `npm audit fix`
node_modules/undici（由 jsdom 29.1.1 引入）
```

生产依赖（`npm audit --omit=dev`）为 **0 漏洞**；该漏洞仅在测试链路（jsdom）使用，不影响发布产物，但当前 CI 的 `npm audit --audit-level=moderate` 会因此失败。

**建议**：执行 `npm audit fix` 或升级 `jsdom`/`vitest` 到修复版本。

### S-09（低）CSP `connect-src` 含 localhost 通配

**位置**：`src-tauri/tauri.conf.json:34`

```text
connect-src ... http://localhost:* ws://localhost:* http://127.0.0.1:* ws://127.0.0.1:*
```

生产包并不需要访问本地任意端口（Vite HMR 仅在开发期使用）。一旦渲染进程出现 XSS，攻击者可以利用该通配探测/访问本机 80、443 等任意端口服务。

**建议**：生产 CSP 移除 `localhost/127.0.0.1` 通配，仅保留 `ipc: http://ipc.localhost`；开发期配置通过 `tauri.conf.json` 的 `dev` 覆盖或构建脚本注入。

### S-10（低）`CredReadW` 空 blob 未判空

**位置**：`src-tauri/src/ipc/bilibili/session.rs:199`

```rust
let blob = slice::from_raw_parts(
    credential_ref.CredentialBlob,
    credential_ref.CredentialBlobSize as usize,
);
```

若凭据存在但 `CredentialBlobSize == 0` 且 `CredentialBlob` 为 null，`from_raw_parts` 属于未定义行为（Rust 要求指针非空且对齐）。当前实际写入的 Cookie 非空，风险很低，但属于防御性缺陷。

**建议**：先判空 `CredentialBlob.is_null() || CredentialBlobSize == 0`，为空直接返回 `Ok(None)`。

### S-11（低）session 临时文件权限窗口

**位置**：`src-tauri/src/ipc/bilibili/session.rs:113`

`write_session_file` 先以默认 ACL 写临时文件、rename 后再 `icacls` 收窄权限。虽然文件里不含 Cookie 本体（Cookie 在 Credential Manager），但包含过期时间等元数据；且收窄失败只告警不报错。

**建议**：写入前先为临时文件设置最小 ACL（或在文件创建时用 `OpenOptions` + 安全描述符），并把权限收窄失败升级为可观测告警。

### S-12（低）任务栏窗口事件广播权限过宽

**位置**：`src-tauri/capabilities/taskbar-lyrics.json:9`

任务栏歌词窗口只应消费事件，却被授予 `core:event:allow-emit`，可以伪造任意事件广播给主窗口（如冒充 `seraph://event`）。当前加载的是本地打包内容，风险可控，但权限面可以更小。

**建议**：移除 `allow-emit`；若确需从该窗口发事件，改用受限的 `allow-emit-to` 并指定目标 label。

### S-13（低）发布链路缺少签名与 Rust 依赖审计

- `tauri.conf.json` 的 `bundle.windows` 未配置 `certificateThumbprint`，NSIS/MSI 安装包未签名，Windows SmartScreen 会告警，用户容易绕过告警安装被篡改的包；
- `.github/workflows/ci.yml` 只做 `npm audit`，没有 `cargo audit` / `cargo deny`；
- `release.yml` 使用 `tauri-apps/tauri-action@v0`（major 版本标签），未按 commit SHA 锁定，存在供应链风险。

**建议**：
1. 购买/配置代码签名证书并在发布流程签名；
2. CI 增加 `cargo audit`（或 `cargo deny`）步骤，并将 Rust 依赖漏洞纳入门槛；
3. GitHub Actions 按 commit SHA 固定版本，并把 `GITHUB_TOKEN` 权限最小化。

### S-14（低）ProgramData 作为默认缓存目录候选

**位置**：`src-tauri/src/ipc/cache.rs:438`

默认缓存目录候选包含 `%ProgramData%\Seraph Audio Player\bilibili-cache`。ProgramData 对多个用户可写，另一个本地用户可能预置 marker 或文件，影响缓存清理的完整性（多用户共享机器场景）。

**建议**：优先使用 `%LOCALAPPDATA%` / app_data_dir 下的用户级目录；ProgramData 候选仅保留给安装器创建的受控目录，并校验目录所有者/ACL。

### S-15（低）短链重定向后未复验最终域名

**位置**：`src-tauri/src/ipc/bilibili/import_audio.rs:131`

`resolve_bvid` 校验了用户输入的初始域名（b23.tv/acg.tv/bilibili.com），随后用裸 client 跟随重定向，但只从最终 URL/HTML 中提取 BV 号，没有复验重定向链的最终主机。该请求不携带 Cookie，实际泄露风险低，但理论上可被用于探测本机 HTTP 服务（重定向到 localhost 时）。

**建议**：限制重定向次数，并在最终 URL 上再次调用 `is_bilibili_host` 校验。

## 四、做得好的安全实践（保留项）

1. **凭据存储**：B 站登录 Cookie 存 Windows Credential Manager（`CredWriteW`/`CredReadW`），session 文件不含 Cookie 本体并做 ACL 收窄。
2. **下载域名白名单**：`is_safe_bilibili_download_url` 要求 `https` + 白名单后缀，音频下载前校验，防止 Cookie 外泄。
3. **路径与递归防护**：音乐目录导入用 `canonicalize` 去重 + 64 层深度上限；缓存扫描跳过符号链接/junction。
4. **误删防护**：缓存目录必须含 `.seraph-cache` marker 才允许清理；封面 GC 只删 1 小时前的孤儿文件。
5. **体积与解压炸弹防护**：歌词 4 MB、m3u8 32 MB、配置 2 MB、EQ 预设 512 KB、头像 512 KB、音频下载 1.5 GB、zlib 解压 8 MB 上限。
6. **原子写**：曲库缓存、歌词、session、缓存设置均采用临时文件 + rename，损坏文件备份为 `.corrupt`。
7. **前端渲染安全**：无 `dangerouslySetInnerHTML`/`eval`/`window.open`，歌词、标题等外部文本由 React 转义；CSP 无 `unsafe-eval`，脚本仅允许 `'self'`。
8. **命令执行安全**：ffmpeg/ffprobe/explorer/icacls 全部使用 `Command::arg` 直传参数（不经 shell），并对以 `-` 开头的文件路径加 `file:` 前缀。
9. **IPC 输入校验**：seek/音量/位置比例拒绝 NaN/Infinity/越界；track id、路径、文件大小均有后端二次校验，不只信任前端。
10. **CI 基线**：提交锁文件（`Cargo.lock`、`package-lock.json`），CI 覆盖 npm audit、Rust fmt/clippy/test、前端 typecheck/test/build。

## 五、修复优先级建议

**立即处理（1–2 周）**

1. S-01：FFmpeg 下载物加哈希/签名校验（或改随包分发）；
2. S-02：头像请求改裸 client + URL 白名单；
3. S-03：所有外部 JSON 响应加字节上限；
4. S-08：升级 undici/jsdom，恢复 CI 绿色。

**短期处理（1 个月内）**

5. S-04：Release URL 用 URL 解析做精确校验；
6. S-05：Cookie 持久化加属性与名称白名单校验；
7. S-06/S-07：zip 条目与内嵌封面加体积上限；
8. S-13：配置代码签名、CI 增加 cargo audit、Actions 锁 SHA。

**持续改进**

9. S-09：收紧生产 CSP 的 connect-src；
10. S-10/S-11/S-12/S-14/S-15：按上文建议逐项加固。

## 六、复核记录（2026-08-05 二次核验）

本报告完成后，逐条对照源码（含行号）、配置、CI 与 `package-lock.json` 重新核实，并再次执行 `npm audit` / `npm audit --omit=dev`。复核结论如下：

| 编号 | 级别 | 复核结论 | 说明 |
| --- | --- | --- | --- |
| S-01 | 高 | ✅ 属实 | 下载链路无哈希/签名校验；下载的 ffmpeg.exe 后续由 `crates/seraph-decoder/src/decoder.rs:344` 直接 `Command::new` 执行 |
| S-02 | 中 | ✅ 属实 | `commands.rs:229-230` 将携带 Cookie 的 client 传入 `resolve_avatar_data_url`（`import_audio.rs:647`）；`normalize_url`（`parsing.rs:56`）只补 https，无域名白名单 |
| S-03 | 中 | ✅ 属实 | `import_audio.rs:419`（`response.bytes()`）、`online_lyrics.rs:85,117,163,199,252,284`、`online_covers.rs:98,142`、`update.rs:52` 均无字节上限；注意 `online_covers.rs:168` 的图片下载已有 4 MB 上限，报告所指仅为 JSON 读取，表述无冲突 |
| S-04 | 中 | ⚠️ 部分属实 | `update.rs:71` 确为 `starts_with`，可被 `.../releases.evil.com/...` 等路径形式绕过；但该类 URL 的 host 仍是 `github.com`，实际打开的是 GitHub 页面而非 evil.com，"诱导访问钓鱼页面"的说法过重。建议实施修复时将该条后果描述改准确，修复建议不变 |
| S-05 | 中 | ⚠️ 部分属实（事实成立、范围略宽） | `session.rs:342` 确实忽略 Domain/Path/Secure/HttpOnly，且 `cookie_header()` 会把全部 Cookie 拼进所有请求；但该合并函数目前仅在 QR 登录轮询（`commands.rs:165`）调用，来源是 `passport.bilibili.com`，"任意被允许访问的 B 站子域"目前并不成立，建议改为"当前调用点仅登录轮询，但实现层面无域/路径作用域" |
| S-06 | 中 | ✅ 属实 | `ffmpeg.rs:200` 解压时未检查 `entry.size()` 与累计解压量，仅下载总量 400 MB 上限无法阻止高压缩比条目膨胀 |
| S-07 | 中 | ✅ 属实 | `media_library.rs:760`（`to_vec()`）与 `:811`（`fs::write`）均无体积校验；WAV 兜底路径有 32 MB 限制，lofty 主路径没有，报告表述准确 |
| S-08 | 中 | ✅ 属实 | 复核实测：`npm audit` 报 undici 7.0.0–7.28.0（已装 7.28.0）1 个高危；`npm audit --omit=dev` 为 0 漏洞，与报告一致 |
| S-09 | 低 | ✅ 属实 | `tauri.conf.json` CSP `connect-src` 含 `http://localhost:*`、`ws://localhost:*`、`http://127.0.0.1:*`、`ws://127.0.0.1:*` |
| S-10 | 低 | ✅ 属实（行号修正） | `from_raw_parts` 位于 `session.rs:201-203`（报告写 199，属近似行号），未判空 `CredentialBlob` / `CredentialBlobSize == 0` 的问题成立 |
| S-11 | 低 | ✅ 属实 | `session.rs:113` 先 `fs::write` 默认 ACL 临时文件，rename 后才 `restrict_session_file_permissions`；Windows 下收窄失败仅 `tracing::warn` 仍返回 Ok |
| S-12 | 低 | ✅ 属实 | `capabilities/taskbar-lyrics.json:9` 确含 `core:event:allow-emit` |
| S-13 | 低 | ✅ 属实 | `tauri.conf.json` 的 `bundle.windows` 无 `certificateThumbprint`；`ci.yml` 无 `cargo audit`；`release.yml` 使用 `tauri-apps/tauri-action@v0` 未锁 SHA，且 workflow 级 `permissions: contents: write` 偏宽 |
| S-14 | 低 | ✅ 属实 | `cache.rs:405-408` 候选顺序为安装目录 → ProgramData → app_data；`ensure_cache_dir_safe` 会在空目录创建 marker，因此首启确实可能选中 `%ProgramData%\Seraph Audio Player\bilibili-cache` |
| S-15 | 低 | ✅ 属实 | `resolve_bvid`（`import_audio.rs:121`）仅校验初始 host（:132），裸 client 跟随重定向后只提取 BV 号，未复验最终主机；无 Cookie 请求，泄露风险低，SSRF 探测风险成立 |

**复核总结**：15 项中 13 项完全属实、2 项部分属实（S-04、S-05）。部分属实的两项均为"事实成立但影响面/后果描述偏宽"，不影响修复优先级；建议在实施修复时按复核备注修正报告表述。

## 七、附录：本次审查执行的动作

- 全量文件清单与依赖清单核对（`rg --files`、`package.json`、`Cargo.toml`、`tauri.conf.json`）；
- 关键词扫描：密钥/令牌/口令、危险前端 API、命令执行、路径拼接、unsafe、删除操作；
- 逐文件阅读安全敏感模块：IPC 全部命令、B 站会话/下载/FFmpeg、曲库导入/删除/封面/歌词、缓存管理、更新检查、DSP/播放器、任务栏原生代码；
- 配置审查：capabilities、CSP、asset 协议 scope、Vite dev server、GitHub Actions；
- 依赖审计：`npm audit --omit=dev`（0 漏洞）、`npm audit`（dev 链路 1 个高危，undici）；
- 未发现硬编码密钥/令牌/私钥（含 `gitignore` 覆盖检查）。
