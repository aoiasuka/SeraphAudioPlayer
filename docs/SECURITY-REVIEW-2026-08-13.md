# Seraph Audio Player 代码安全审查报告

- 审查日期：2026-08-13
- 审查对象：`Seraph Audio Player` v0.5.6
- 代码基线：commit `45216de9ed5045f63ce083b7942814e814e5f9d4`（2026-08-08，`fix: 升级 plist/quick-xml 与 quinn-proto，修复 cargo audit 高危漏洞`）
- 技术栈：Tauri 2 + React 18 + TypeScript + Rust（workspace crates）+ Windows 原生集成（WASAPI / SMTC / 任务栏）
- 审查方式：全量静态代码审查（前端 `src/`、后端 `src-tauri/`、`crates/`、`scripts/`、`.github/workflows/`、`package.json` / `Cargo.toml` / 两个 lock 文件、`tauri.conf.json` 与 capabilities）+ 依赖版本与公开漏洞库（RustSec / npm）交叉核对
- 未覆盖（建议后续补充）：动态渗透测试、模糊测试（音频解析器 / 歌词解析器）、安装包逆向验证、`npm audit` / `cargo audit` 的本次实时联网复核（CI 已持续强制，见第七节）
- 上一轮：v0.5.5 的《SECURITY-REVIEW-2026-08-05.md》共报告 S-01~S-15（1 高 / 7 中 / 7 低），本轮已逐项复核，全部落地，复核结论见第六节

## 一、总体结论

项目整体安全水平良好，**本轮未发现高危或中危问题**。v0.5.5 报告的 15 项问题（S-01~S-15）在 v0.5.6 代码中已全部修复并有测试覆盖，后端对关键边界（下载域名白名单、Cookie 持久化、外响应体体积上限、压缩炸弹、路径与递归防护、IPC 数值入参）的处理明显高于同类本地播放器的平均水平。

本轮新增 7 项低危、2 项信息级发现，主要分布在：

1. **依赖治理滞后**：RustSec 的两个 unsound 公告（anyhow、event-listener）已有修复版，但 lockfile 仍停留在受影响版本，CI 的 `--ignore` 注释已过时（N-01）。
2. **两处外部 URL 校验盲区**：SMTC 封面 URL 未校验协议/域名（N-02）、iTunes 封面 URL 未校验域名（N-03）——都是「上游 API 被攻破」场景下的纵深防御缺口，均不涉及凭据外泄。
3. **发布与运行时的纵深防御**：安装包未配置 Authenticode 签名（N-05）、Tauri 未启用 `freezePrototype`（N-06）、本地 ffmpeg 定位链缺少签名校验（N-07）。
4. **格式注入与最小权限**：M3U8 导出未消毒元数据换行（N-04）；CSP 中存在冗余第三方域名（N-08）；在线匹配功能有元数据外发隐私影响（N-09）。

## 二、威胁模型与关键资产

本应用是本地优先的 HiFi 播放器，攻击面来自四个方面：

| 面 | 攻击者 | 能拿到什么 / 能做什么 |
| --- | --- | --- |
| 网络 | B 站/歌词/封面/更新 API 被攻破、CDN 异常、恶意服务器 | 投喂恶意 JSON/图片/音频/URL，诱导应用访问任意主机 |
| 文件 | 恶意音频文件、恶意歌词/预设/清单/配置/压缩包 | 解析器崩溃、内存/磁盘耗尽、路径穿越、元数据注入 |
| 本地 | 同机其他用户或进程 | 窃取 B 站会话 Cookie、篡改缓存/曲库/ffmpeg 可执行文件 |
| 供应链 | npm/crates.io 依赖、GitHub Actions、下载源 | 在构建或运行时植入恶意代码 |

需要保护的核心资产按价值排序：B 站登录态（SESSDATA 等 Cookie）> 用户曲库/歌词/歌单数据 > 主机文件系统与网络探测能力。所有高危资产路径（Cookie 白名单 + Credential Manager + ACL 收窄、曲库原子写 + 备份、下载域名白名单）均已在上轮修复并保持。

## 三、新增发现汇总

| 编号 | 级别 | 问题 | 位置 |
| --- | --- | --- | --- |
| N-01 | 低 | anyhow 1.0.102、event-listener 5.4.1 处于受影响版本，修复版已发布，CI ignore 注释过时 | `Cargo.lock`；`.github/workflows/ci.yml:52-55`、`release.yml:56` |
| N-02 | 低 | SMTC 封面 URL 未校验协议/域名，外部输入可直达系统缩略图抓取 | `src-tauri/src/smtc.rs:217` |
| N-03 | 低 | iTunes 封面 URL（`artworkUrl100`）未校验协议/域名，无 Cookie 的受限 SSRF | `src-tauri/src/ipc/library/online_covers.rs:130-163` |
| N-04 | 低 | M3U8 导出未消毒标题/艺术家/路径中的 CR/LF 与 `#`，可注入伪造条目 | `src-tauri/src/ipc/playlist_io.rs:225-229` |
| N-05 | 低 | 发布产物仍未配置 Authenticode 代码签名（S-13 剩余项） | `src-tauri/tauri.conf.json:52`（bundle.windows 仅有语言配置） |
| N-06 | 低 | Tauri 未启用 `app.security.freezePrototype` | `src-tauri/tauri.conf.json:29-34` |
| N-07 | 低 | 本地 ffmpeg/ffprobe 定位链（含 PATH、exe 目录、环境变量）无签名校验 | `crates/seraph-decoder/src/ffmpeg.rs`（`tool_candidates`） |
| N-08 | 信息 | CSP `connect-src` 含冗余第三方域名，与最小权限原则不符 | `src-tauri/tauri.conf.json:34` |
| N-09 | 信息 | 在线歌词/封面匹配会把曲目标题+艺术家发送至第三方（网易云/酷狗/QQ/Apple） | `src-tauri/src/ipc/library/online_lyrics.rs`、`online_covers.rs` |

## 四、新增发现详情与修复建议

### N-01（低）Rust 依赖 unsound 公告有修复版未升级

**位置**：`Cargo.lock`（`anyhow 1.0.102`、`event-listener 5.4.1`）；`.github/workflows/ci.yml:52-55`、`release.yml:56`

RustSec 数据库显示：

- `RUSTSEC-2026-0190`：anyhow `Error::downcast_mut()` 违反借用规则，受影响版本 ≤1.0.102，**修复版 ≥1.0.103**；
- `RUSTSEC-2026-0221`：event-listener 的 `StackSlot<'_, T>` 无条件实现 `Send + Sync`，可让 `!Send` 标签跨线程造成数据竞争，**修复版 ≥5.4.2**；
- `RUSTSEC-2024-0429`：glib `VariantStrIter` unsound，lockfile 为 0.18.5，Linux 专属且被 Tauri 上游锁定 0.18，Windows 产物不包含，ignore 仍合理。

CI 注释称前两项「暂无 semver 兼容修复版」，该说法已过时。当前 ignore 使 `cargo audit` 全绿是「被豁免的全绿」，而非真实清零。二者均为 unsound 类（本项目当前调用路径未见触发已知模式，故定级为低），但修复成本极低。

**建议**：

1. `cargo update -p anyhow --precise 1.0.103`、`cargo update -p event-listener --precise 5.4.2`（保留 semver 约束），跑通 `cargo test --workspace` 后提交；
2. 从两个 workflow 的 `cargo audit` 命令中移除 `--ignore RUSTSEC-2026-0190 --ignore RUSTSEC-2026-0221`，只保留 glib 一项并更新注释；
3. 为三项 ignore 建立月度复查习惯：修复版一旦可用立即升级并删 ignore。

### N-02（低）SMTC 封面 URL 未校验协议/域名

**位置**：`src-tauri/src/smtc.rs:217`（`cover_to_uri`）

`cover_to_uri` 对 `http://` / `https://` 前缀的 URL 一律透传，交给 Windows SMTC 缩略图加载。B 站曲目的 `cover` 字段来自 `playurl`/`view` 外部 API（`resolved.video.pic`），头像 URL 已在上轮 S-02 中加了 `https + hdslb.com/bilibili.com` 白名单，但封面 URL 没有同等防护：若上游 API 被攻破返回恶意 URL，系统会以服务身份发起图片抓取（无登录 Cookie，但存在协议降级与任意主机探测）。同时 `pic` 字段未像头像那样做 `normalize_url`，`http://` 明文封面会直接透传。

**建议**：

1. 在 `track_from_resolved_audio` 写入 `cover` 前，对 URL 复用 `is_safe_avatar_url`（或同源白名单函数），非白名单 URL 置空回退默认封面；
2. `cover_to_uri` 内对 http(s) 再做一次白名单校验兜底，拒绝 `http://` 明文与白名单外主机；
3. 本地路径分支保持现状（`file://` + 原生 Windows 路径）。

### N-03（低）iTunes 封面 URL 未校验协议/域名

**位置**：`src-tauri/src/ipc/library/online_covers.rs:130-163`

`fetch_itunes_cover_bytes` 取 API 返回的 `artworkUrl100`，经 `replace("100x100","600x600")` 后直接 `download_image`。URL 属外部输入，未校验 scheme/host，也未限制重定向（reqwest 默认 10 跳）。该 client 无 Cookie，且响应有 4 MB 上限与图片魔数校验，因此实际影响是「受限盲 SSRF / 内网探测」——一旦 iTunes 上游被攻破，可诱导后端向任意主机发起 ≤4 MB 的 GET。QQ 封面源（`y.gtimg.cn`）是固定主机插值 `albummid`，不存在此问题。

**建议**：

1. `download_image` 入口校验 `scheme == https` 且 host 在 `apple.com` / `mzstatic.com` 白名单内；
2. 显式设置重定向策略（如 `Policy::limited(3)`），并在最终 URL 上复验域名；
3. 与 N-02 一致，建议把「域名白名单校验」收敛成一处公共函数，避免后续新增源再次遗漏。

### N-04（低）M3U8 导出未消毒元数据中的换行与注释符

**位置**：`src-tauri/src/ipc/playlist_io.rs:225-229`

导出端直接把 `entry.duration` / `entry.artist` / `entry.title` / `entry.path` 拼进 `#EXTINF` 与路径行。曲目标题/艺术家来自音频标签或 B 站 API，若含 `\r`/`\n` 可注入伪造条目；以 `#` 开头的路径会被解析为注释而丢失。导入端对每个条目都做 `is_file()` 存在性校验，因此无法凭空制造可播放文件，实际危害很小，但属于格式注入隐患，也应从源头消毒。

**建议**：导出前把 `artist` / `title` / `path` 中的 `\r`、`\n` 归一为空格，并对以 `#` 开头的路径加转义或前缀处理；同时给导入端解析器补一条「CR/LF 注入」测试用例。

### N-05（低）发布产物未配置 Authenticode 代码签名

**位置**：`src-tauri/tauri.conf.json:52`（`bundle.windows` 仅含 wix/nsis 语言配置，无 `signCommand`）

S-13 中的 CI 最小权限、Actions SHA 锁定、`cargo audit` 门禁均已落实，唯独「发布签名」未完成。未签名的 exe/msi 依赖 SmartScreen 信誉、用户无法验证发布者身份，供应链完整性链在「构建产物 → 用户」最后一公里是断开的。

**建议**：购置/托管代码签名证书（建议硬件令牌或受控 CI 密钥库），在 `bundle.windows.signCommand` 挂接 `signtool` 或等效方案，对安装器与主程序签名；证书私钥不得进入仓库或普通 CI 环境变量，配合 OIDC/短期证书方案更佳。

### N-06（低）未启用 `freezePrototype`

**位置**：`src-tauri/tauri.conf.json:29-34`

Tauri 2 提供 `app.security.freezePrototype`，默认关闭。当前 `script-src 'self'` 已把 XSS 概率压到极低，但若出现注入，冻结 `Object.prototype` 等内建原型可阻断一类原型污染利用链（污染 `__proto__`、覆写内建方法）。

**建议**：在 `security` 段加入 `"freezePrototype": true`，前端跑全量测试与手工冒烟确认无依赖原型扩展的第三方库。

### N-07（低）本地 ffmpeg/ffprobe 定位链无签名校验

**位置**：`crates/seraph-decoder/src/ffmpeg.rs`（`tool_candidates`）

定位顺序为：`SERAPH_FFMPEG_PATH` → app 管理目录（`app_data_dir/ffmpeg`）→ `SERAPH_FFMPEG_DIRS` → exe 目录 → PATH。下载自 gyan.dev 的副本虽然经过 SHA-256 校验（S-01），但落盘后没有任何防篡改标记；同用户权限的恶意进程（其他软件、宏文档等）可覆盖 `app_data_dir/ffmpeg/ffmpeg.exe` 或 PATH 上的同名文件，随后由解码器 `Command::new` 执行。这是典型的本机横向风险，不属于远程利用。

**建议**：

1. 对 app 管理目录下的 `ffmpeg.exe` / `ffprobe.exe` 在首次使用时做 Authenticode 签名校验（S-01 建议中的第 2 步），失败则拒绝执行并提示重新下载；
2. 默认禁用 PATH 回退，或把 PATH 回退做成显式设置项；环境变量覆盖项同理；
3. 至少对下载产物再存一份 SHA-256 记录文件，加载前快速比对防「同用户进程篡改」。

### N-08（信息）CSP `connect-src` 冗余第三方域名

**位置**：`src-tauri/tauri.conf.json:34`

`connect-src` 白名单含 `music.163.com`、`lyrics.kugou.com`、`c.y.qq.com`、`y.qq.com`，但所有出站 HTTP 均在 Rust 后端完成，前端无任何 `fetch`/`XHR` 到这些主机。从最小权限角度，这些条目应移除（保留 B 站域名与 `ipc:`）；若未来前端直连，再按需加回。

### N-09（信息）在线匹配功能的元数据外发（隐私）

**位置**：`src-tauri/src/ipc/library/online_lyrics.rs`、`online_covers.rs`

「在线匹配歌词/封面」会向网易云音乐、酷狗、QQ 音乐、iTunes 发送曲目标题与艺术家（用户主动触发，非静默）；启动更新检查会访问 GitHub API；登录态查询会访问 B 站 NAV 接口。均为功能必要行为，但属于用户数据外发，建议在设置页或首次使用时以一句话说明数据去向。

## 五、上一轮 S-01~S-15 修复复核（全部通过）

| 编号 | 结论 | 复核依据 |
| --- | --- | --- |
| S-01 FFmpeg 下载物完整性 | 已修复 | `constants.rs` 固定版本 + 官方 SHA-256，`ffmpeg.rs` 流式哈希校验后才解压 |
| S-02 头像 URL 携带 Cookie | 已修复 | 裸 client 下载 + `is_safe_avatar_url` https/域名白名单（`commands.rs`、`import_audio.rs`） |
| S-03 外部 JSON 无上限 | 已修复 | `http_util.rs` 统一 `read_bytes_capped` / `read_json_capped`（8 MB），全链路接入 |
| S-04 Release URL 前缀绕过 | 已修复 | `update.rs` `is_allowed_release_url` 逐要素校验 + 点段拒绝，含绕过用例矩阵 |
| S-05 Set-Cookie 属性未校验 | 已修复 | `session.rs` Cookie 名称白名单（`BILIBILI_COOKIE_ALLOWLIST`） |
| S-06 zip 解压炸弹 | 已修复 | `extract_ffmpeg_tools_with_limit` 单条目 512 MB 硬截断 + 半截输出清理 |
| S-07 内嵌封面无上限 | 已修复 | `MAX_EMBEDDED_COVER_BYTES`（32 MB）覆盖选图与落盘双入口 |
| S-08 dev 依赖 undici 漏洞 | 已修复 | undici 7.29.0、nanoid 3.3.18，CI 强制 `npm audit` 归零 |
| S-09 devCsp localhost 通配 | 已修复 | 生产 CSP 移除，仅 `devCsp` 保留 |
| S-10 CredReadW 空 blob UB | 已修复 | `session.rs:210` 判空后 `from_raw_parts` |
| S-11 session 临时文件权限窗口 | 已修复 | 先建空临时文件收窄 ACL，再写内容，rename 后幂等再收窄 |
| S-12 歌词条窗口全局 emit | 已修复 | capability 收窄为 `core:event:allow-emit-to`，前端 `emitTo("main")` |
| S-13 CI 供应链 | 部分修复 | Actions 全部 SHA 锁定、最小权限、`cargo audit` 门禁已落实；**代码签名未落实（转入 N-05）** |
| S-14 ProgramData 缓存目录 | 已修复 | 用户级 app_data 优先 + marker 机制 + 空目录校验 |
| S-15 短链重定向未复验 | 已修复 | `resolve_bvid` 跟随重定向后复验 `is_bilibili_host` |

## 六、已验证干净的领域（肯定清单）

- **前端 XSS 面**：无 `dangerouslySetInnerHTML`、`eval`、`new Function`、`window.open`；歌词/元数据一律经 React 默认转义渲染；`coverSrc` 对 URL scheme 有正则约束。
- **IPC 入参**：音量/seek/位置等数值有 NaN/Infinity/负值校验；`reveal_in_explorer` 拒绝相对路径并校验存在性；`open_release_page` URL 严格白名单。
- **子进程调用**：全部经 `Command::new` 参数化传参，无 shell 拼接；ffmpeg 输入路径对 `-` 前缀加 `file:` 前缀防选项注入；控制台窗口隐藏。
- **文件操作**：曲库/歌词/配置/设置全部原子写（唯一临时名 + rename）；曲库损坏时备份 `.corrupt` 并拒绝覆盖；缓存清理有 marker、递归深度、符号链接与扩展名白名单多层防护。
- **解析器健壮性**：歌词 zlib 解压 8 MB 炸弹防护；WAV/DSD ID3 解析 32 MB 上限与边界校验；`regex` crate 线性时间，无 ReDoS；外部 JSON 全部 capped。
- **unsafe 面**：`crates/` 零 `unsafe`（音频后端走 `wasapi`/`cpal` 安全封装）；`src-tauri` 内全部 29 处 `unsafe`（含函数声明）集中在任务栏窗口/凭据互操作，均配有判空、句柄生命周期与释放处理，未见跨线程裸指针或明显的生命周期缺口。
- **凭据管理**：Cookie 名称白名单 + Windows Credential Manager + 落盘文件 ACL 收窄三重防线；非 Windows 构建显式失败而非静默降级。
- **依赖来源**：Cargo 全部来自 crates.io registry，无 git 依赖；npm 使用 lockfile + `npm ci`；GitHub Actions 按 commit SHA 锁定。

## 七、依赖与供应链审计状态

- **npm**：CI（`ci.yml`）以 `npm audit --audit-level=moderate`、发布（`release.yml`）以 `--audit-level=low` 强制门禁；当前 lockfile 中 undici 7.29.0、nanoid 3.3.18，README 记录的「npm audit 归零」与版本一致。
- **crates.io**：发布与 CI 均强制 `cargo audit`，当前豁免 3 项（见 N-01）；plist 1.10.0 / quick-xml 0.41.0 / quinn-proto 0.11.16 为近期修复版，高危公告已清零。
- **第三方 Action**：checkout/setup-node/rust-toolchain/rust-cache/tauri-action 全部按 SHA 锁定；`permissions: contents: read`（CI）/ `contents: write`（发布）为最小授权。
- **运行时第三方二进制**：ffmpeg/ffprobe 固定版本 + SHA-256 校验（S-01），残余风险见 N-07。

## 八、修复优先级与行动计划

1. **立即（分钟级，低风险）**：N-01 升级 anyhow / event-listener 并删除过时 ignore；N-06 开启 `freezePrototype`。
2. **本轮迭代（小时级）**：N-02 / N-03 统一 URL 白名单校验；N-04 导出消毒；N-08 收紧 CSP。
3. **发布前（需外部资源）**：N-05 代码签名；N-07 ffmpeg 签名校验。
4. **持续**：N-09 隐私说明；三项 RUSTSEC ignore 月度复查；补充动态测试与模糊测试（未覆盖项）。

## 九、复测建议

- 构建一个恶意 `artworkUrl100`（指向本地 HTTP 服务）验证 N-03 修复后确实拒绝；同法验证 N-02 的 SMTC 封面白名单。
- 对 `import_playlist_m3u8` 投喂含 `\r\n` 与 `#` 的标题，验证导出→导入往返不产生幽灵条目。
- 升级 anyhow / event-listener 后运行 `cargo test --workspace`、`npm test`、`cargo clippy --workspace --all-targets -- -D warnings`，并确认 `cargo audit` 在移除两项 ignore 后仍全绿。
- 条件允许时引入 `cargo-fuzz` 对 `lyrics.rs`（KRC/QRC/压缩）与 WAV/DSD ID3 解析做短时模糊测试。
