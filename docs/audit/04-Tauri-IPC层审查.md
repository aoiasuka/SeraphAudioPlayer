所有文件已完整读取,分析完毕。以下是最终审查报告。

---

# Seraph Audio Player — Tauri 后端 IPC 层深度审查报告

**审查范围**:`src-tauri` 全部 IPC 层源码(逐行通读,只读未改)。整体印象:项目经过多轮修复(代码内有 L-x/M-x 修复注释),缓存 marker、zip bomb 防御、递归防护、凭据入 Credential Manager 等都已做对。但仍发现以下问题。

---

## P0 — 安全漏洞 / 数据丢失

### [P0-1] Bilibili 登录 Cookie 泄漏到任意用户输入的 URL(凭据窃取)
- **位置**:`D:\_SeraphAudioPlayer\src-tauri\src\ipc\bilibili\import_audio.rs:50-56`(`resolve_bvid` 中 `client.get(trimmed).send()`),client 来源 `commands.rs:12-13`(`bilibili_client_for_app`),该 client 在 `session.rs:1-16` 构造时将 Cookie(含 SESSDATA)放入 **default_headers**。
- **问题**:导入命令接受任意字符串;当输入不含 BV 号但以 `http(s)://` 开头时,直接用带登录 Cookie 的 client 向该 URL 发起 GET。default_headers 中的 Cookie 会随**首个请求**发往任意主机(reqwest 仅在跨域重定向时剥离敏感头,首请求不剥离)。攻击场景:诱导用户把一条伪装的"B 站分享链接"(如 `https://evil.example/BV-share`)粘进导入框 → SESSDATA 直达攻击者服务器 → B 站账号完全接管。
- **修复**:解析短链/网页时改用**无 Cookie 的裸 client**;或在发送前校验 host 白名单:
```rust
fn is_bilibili_host(url: &reqwest::Url) -> bool {
    url.host_str().is_some_and(|h| {
        h == "b23.tv" || h == "acg.tv"
            || h.ends_with(".bilibili.com") || h == "bilibili.com"
    })
}
// resolve_bvid 中:
let url = reqwest::Url::parse(trimmed).map_err(|_| "无效链接")?;
if !is_bilibili_host(&url) { return Err("仅支持 B 站链接".into()); }
let bare = bilibili_client_with_cookie(None)?; // 解析页面用无 Cookie 客户端
let response = bare.get(url).send().await...;
```
- **置信度**:高

### [P0-2] `library-cache.json` 非原子写入 + 读损坏时静默清空 → 整个曲库丢失
- **位置**:`media_library.rs:66-68`(`fs::write` 直接覆盖写)、`media_library.rs:78` 与 `commands.rs:64`(`read_cached_tracks(&app).unwrap_or_default()`)。
- **问题**:两个缺陷叠加成数据丢失链:① `write_cached_tracks` 用 `fs::write` 直接覆盖,库大(含全部内嵌歌词,可达数 MB)时写一半崩溃/断电 → JSON 截断;② 下次任何导入操作里 `read_cached_tracks(...).unwrap_or_default()` 把解析失败当空库,`merge_cached_tracks` 后**用只含本次导入曲目的列表覆盖整个缓存文件**——用户全部曲库记录(含手动匹配的歌词)静默蒸发。
- **修复**:temp+rename 原子写,读失败时显式报错并备份损坏文件而不是当空库:
```rust
fn write_cached_tracks(app: &AppHandle, tracks: &[ImportedTrack]) -> Result<(), String> {
    let path = library_cache_path(app)?;
    // ...create_dir_all...
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(tracks).map_err(...)?).map_err(...)?;
    fs::rename(&tmp, &path).map_err(...)  // Windows 同卷 rename 原子
}
// merge_tracks_into_cache / import_tracks 中:
let cached = match read_cached_tracks(app) {
    Ok(t) => t,
    Err(e) if path.is_file() => return Err(format!("曲库缓存损坏,已中止写入: {e}")),
    _ => Vec::new(),
};
```
- **置信度**:高

---

## P1 — 功能错误

### [P1-1] Bilibili 音频下载共用 30 秒总超时 client → 大文件导入必然失败
- **位置**:`session.rs:7-9`(`Client::builder().timeout(Duration::from_secs(30))`);该 client 被 `import_audio.rs:342-390`(`ensure_audio_file`→`download_audio_to_file`)用于下载最大 1.5 GB 的音频。
- **问题**:reqwest 的 `Client::timeout` 是**整个请求周期**(连接到 body 读完)的总超时。FLAC 流常见数十至数百 MB,普通带宽下 30 秒读不完 → `chunk()` 报 timeout → 换 backup URL 重试仍是同一 client 同样失败。表现为"大文件/慢网导入总是失败,小文件成功",且残留 `.download` 临时文件依赖 1 小时孤儿清扫。
- **修复**:下载专用 client 不设总超时,用 `connect_timeout` + `read_timeout`(reqwest 0.13 支持 `read_timeout`,针对 chunk 间隔):
```rust
fn bilibili_download_client(cookie: Option<&str>) -> Result<Client, String> {
    Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(30)) // 两次读之间的空闲超时
        .default_headers(bilibili_headers(cookie)?)
        .build().map_err(...)
}
```
- **置信度**:高

### [P1-2] `.eac3` 文件不在缓存受管扩展名中 → 配额统计/自动清理/clear_cache 全部漏掉
- **位置**:`cache.rs:462-472`(`is_managed_cache_file` 白名单 `"m4a"|"flac"|"opus"|"aac"|"mp3"|"download"|"tmp"`);而 `impls_and_tests.rs:38-48`(`output_extension`)对 Dolby/Atmos 流产出 `"eac3"` 扩展名。
- **问题**:杜比流缓存文件(往往还是最大的)不计入 `cache_size`、不参与 LRU 清理、`clear_cache` 也删不掉——缓存目录实际占用可无限超出用户设置的上限;"清空缓存"后 UI 显示 0 但 eac3 文件仍在。另外 `.{name}.ok` sentinel 文件也永不清理(体积为 0,影响小)。
- **修复**:白名单加入 `"eac3"`;删除音频文件时同步删除对应 sentinel:
```rust
matches!(ext, "m4a" | "flac" | "opus" | "aac" | "mp3" | "eac3" | "download" | "tmp")
// 删除处:
fs::remove_file(&entry.path)?;
let _ = fs::remove_file(ok_sentinel_path(&entry.path));
```
- **置信度**:高

### [P1-3] 曲库缓存读-改-写完全无锁,多命令并发丢更新
- **位置**:`commands.rs:45-74`(`import_tracks` 在 `spawn_blocking` 中读改写)、`commands.rs:14-28`(`delete_track`)、`commands.rs:76-117`(`save_track_lyrics`)、`media_library.rs:70-82`(`merge_tracks_into_cache`,被 bilibili 导入 async 调用)。
- **问题**:所有写路径都是"读全量 JSON → 内存合并 → 覆盖写",之间无任何互斥。真实触发:`import_bilibili_favorites` 批量循环导入(每首写一次缓存,`commands.rs(bilibili):34-44`)期间,用户在 UI 删除曲目或本地导入 → 后完成的写覆盖先完成的写:删除"复活"、或导入结果丢失。
- **修复**:在 `AppState`(或模块级 `static`)加一把覆盖"读+写"的锁:
```rust
static LIBRARY_LOCK: Mutex<()> = Mutex::new(()); // parking_lot
fn with_library<R>(f: impl FnOnce() -> Result<R, String>) -> Result<R, String> {
    let _g = LIBRARY_LOCK.lock();
    f()
}
```
所有 `read_cached_tracks→write_cached_tracks` 序列包进去(spawn_blocking 内也适用,parking_lot Mutex 非 async 持有,无跨 await 问题——bilibili 导入中把合并段放进 `spawn_blocking`)。
- **置信度**:高

### [P1-4] ffmpeg.exe 无校验和验证,且含第三方代理镜像 → 供应链任意代码执行风险
- **位置**:`constants.rs:26-31`(`FFMPEG_DOWNLOAD_URLS` 含 `https://mirror.ghproxy.com/...`)、`ffmpeg.rs:36-58`(下载解压后直接落盘执行,无任何 hash/签名校验)。
- **问题**:`mirror.ghproxy.com` 是不受控的第三方代理,可对返回的 zip 内容做任意替换;下载的 `ffmpeg.exe` 随后被 `remux_audio`/解码器以用户权限执行。HTTPS 只保护传输,不约束镜像运营者。gyan.dev 的 `release-essentials.zip` 是滚动 latest,无法 pin 固定 hash,但至少第三方镜像必须去掉或校验。
- **修复**:优先固定版本 URL + 内置 SHA-256 校验(下载后 `sha2` 计算比对,不匹配即删除拒用);至少移除 ghproxy 镜像,或仅当官方源全部失败且用户显式确认后才使用。
- **置信度**:高(风险成立;被实际利用概率取决于镜像)

### [P1-5] Set-Cookie 的 `Expires` 解析必然失败(小写化后做大小写敏感月份匹配)
- **位置**:`session.rs:310`(`let lower = attr.to_ascii_lowercase();`)、`session.rs:322-330`(`lower.strip_prefix("expires=")` 后把**已小写**的日期传入)、`session.rs:369-373`(`parse_http_date_to_unix` 中 `match month_str { "Jan" => 1, ... }` 大小写敏感)。
- **问题**:`"expires=sun, 06 nov 1994 ..."` 中月份是 `"nov"`,匹配 `"Nov"` 失败 → 恒返回 `None`。所有只带 `Expires` 不带 `Max-Age` 的 cookie 被当作永不过期的 session cookie(`expire_at=None` 分支),`cookie_header()` 的过期过滤(`impls_and_tests.rs:57-67`)对它们永远不生效——已过期的登录态仍被发送,后端专门做的 L-11 修复(`types.rs:186-187`)对这类 cookie 形同虚设。
- **修复**:解析日期时用原始大小写(先在原 `attr` 上做 `strip_prefix` 的 ASCII 不敏感版本),或在 `parse_http_date_to_unix` 里对月份做 `eq_ignore_ascii_case` 匹配:
```rust
let month = ["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"]
    .iter().position(|m| month_str.eq_ignore_ascii_case(m))? as u32 + 1;
```
- **置信度**:高

---

## P2 — 边界条件 / 资源泄漏 / 竞态

### [P2-1] 并发导入同一 BV 视频:确定性临时文件路径互相覆盖 → 缓存文件损坏
- **位置**:`import_audio.rs:636-642`(`temp_download_path` = `{file}.download`,无唯一后缀)、`import_audio.rs:371-387`(下载→finalize)。
- **问题**:UI 双击/批量导入与单曲导入并发命中同一 BV+cid 时,两个任务对同一 `.download` 文件 `File::create`(截断)并交错写 chunk,随后各自 rename/remux → 产出损坏音频,且写入 `.ok` sentinel 后被永久视为有效缓存。
- **修复**:临时文件加唯一后缀 + 按目标路径去重的进行中任务表:
```rust
let temp_path = path.with_file_name(format!("{file_name}.{}.download", uuid_or_nanos));
// 或模块级 Mutex<HashSet<PathBuf>> 拒绝/等待重复目标的并发下载
```
- **置信度**:高

### [P2-2] 缓存清理中途失败即整体返回错误,已删文件不标记 `cache_missing`;正在播放的文件删除必失败
- **位置**:`cache.rs:104-109`(`clear_cache` 循环内 `fs::remove_file(...)?`)、`cache.rs:196-201`(`enforce_cache_limit_inner` 同样),`mark_tracks_cache_missing_by_paths` 在循环之后(`cache.rs:114`/`207`)。
- **问题**:Windows 上正在播放的缓存文件通常被独占句柄占用,`remove_file` 报 `Access is denied` → 命令提前返回 Err,**已经删掉的文件**永远不会被标记 `cache_missing`,曲库中留下指向不存在文件的曲目,播放报错且 UI 不显示"需重新下载"。
- **修复**:跳过失败项继续,最后统一标记与汇报:
```rust
let mut errors = Vec::new();
for entry in entries {
    match fs::remove_file(&entry.path) {
        Ok(()) => { removed_bytes += entry.size; removed_paths.push(entry.path); }
        Err(e) => errors.push(format!("{}: {e}", entry.path.display())),
    }
}
mark_tracks_cache_missing_by_paths(&app, &removed_paths)?;
// errors 非空时附加在结果里而不是整体失败
```
- **置信度**:高

### [P2-3] 缓存目录递归扫描无 junction/symlink 环与逃逸防护
- **位置**:`cache.rs:432-460`(`collect_cache_files_inner` 无 `visited` 集合、无深度上限,`path.is_dir()` 跟随重解析点)。
- **问题**:曲库导入端已修复此问题(`media_library.rs:11-19`,L-14),但缓存扫描端没有对齐:① 缓存目录内出现指向祖先的 junction → 无限递归栈溢出崩溃;② junction 指向用户音乐目录 → **清理时按扩展名删除缓存目录之外的真实 mp3/flac 文件**(marker 只检查顶层)。
- **修复**:复用 L-14 方案——`fs::canonicalize` 去重 + 深度上限;或用 `entry.file_type()?.is_symlink()` 跳过重解析点(Windows junction 亦被 `is_symlink` 报告);更严格可要求删除目标 canonicalize 后必须以缓存根为前缀。
- **置信度**:中高(代码事实确凿,触发需目录内出现链接)

### [P2-4] remux 失败时把原始 m4s 数据改名为 `.flac`/`.eac3` 并写 sentinel → 永久缓存可能不可播的文件
- **位置**:`import_audio.rs:521-542`(`finalize_audio_file`:remux Err 分支落到 `fs::rename(temp_path, path)`),`import_audio.rs:376-380`(随后写 `.ok` sentinel)。
- **问题**:FLAC-in-fMP4 或 EAC3 流 remux 失败(ffmpeg 异常/磁盘满)时,原始 fMP4 字节被冠以 `.flac`/`.eac3` 扩展名保存并标记"缓存有效",此后 `ensure_audio_file` 永远直接复用;EAC3 场景下 Symphonia 无法解码原始容器,曲目永久不可播且不会重下。
- **修复**:remux 失败时区分流类型——对必须 remux 的流(EAC3)直接返回 Err 不落盘;fallback rename 时应使用与实际容器一致的扩展名(如 `.m4a`)并据此生成 track。
- **置信度**:中

### [P2-5] `cache-settings.json` 损坏后所有缓存相关功能瘫痪且不自愈
- **位置**:`cache.rs:258-261`(解析失败直接 `Err`)、`cache.rs:275-285`(`fs::write` 非原子)。
- **问题**:与 P0-2 同源的非原子写;但这里读失败不会丢数据,而是让 `load_cache_settings` 报错 → `cache_dir()`/`get_cache_status`/所有 bilibili 导入全部失败,用户只能手删文件恢复。设置文件损坏时应回退默认值重建。
- **修复**:解析失败时记录警告、备份坏文件、用 `default_cache_settings` 重建;写入改 temp+rename。
- **置信度**:高

### [P2-6] icacls 权限收紧因引号包裹用户名而必然失败(仅记 warn)
- **位置**:`session.rs:246-262`(`format!("\"{name}\":F")`)。
- **问题**:`std::process::Command` 在 Windows 上会把参数中的 `"` 转义为 `\"`,icacls 实际收到的账户名是**带字面引号的** `"user"`,账户解析失败 → 整条 icacls 命令失败,session 文件保持默认继承 ACL。注释"用引号包裹更稳妥"的假设不成立(Command 参数传递不经 shell,无需引号;含空格的参数 std 会自动加引号)。实际泄露面小(cookie 已移入 Credential Manager,文件只剩 username/mid/face),故列 P2。
- **修复**:去掉手动引号:`let grant_arg = format!("{name}:F");`
- **置信度**:中(基于 std 引号转义规则推断,建议实测验证)

### [P2-7] `set_volume`/`seek` 参数未做范围/NaN 校验
- **位置**:`playback.rs:87-91`(`seek(seconds: f64)`)、`playback.rs:108-115`(`set_volume(volume: f32)`)。
- **问题**:前端(或被 XSS 的 webview)可传 `NaN`/`Infinity`/负值/`1e9`,直接透传给音频引擎。若 `seraph-audio` 内部无 clamp,音量增益异常可能产生爆音(损伤听力/设备)。IPC 层应做最后防线。
- **修复**:
```rust
if !volume.is_finite() { return Err("invalid volume".into()); }
let volume = volume.clamp(0.0, 1.0);
if !seconds.is_finite() || seconds < 0.0 { return Err("invalid seek position".into()); }
```
- **置信度**:中(引擎内部是否 clamp 未读到,IPC 层缺校验是事实)

### [P2-8] `bilibili_poll_login` 依赖 `error_for_status` 前的 header 克隆,但登录成功前提是 `api.code==0`;而轮询与 `bilibili_login_status` 每次都全量重写 Credential Manager + session 文件
- **位置**:`commands.rs(bilibili):88-97`、`150-164`(`bilibili_login_status` 中 `save_session` 每次调用都执行,且 `resolve_avatar_data_url` 每次重新下载头像并 base64 内嵌)。
- **问题**:前端若周期性调用登录状态(常见做法),每次都触发:一次 NAV 请求 + 一次头像下载(最大 512KB)+ 一次 CredWriteW + 一次 icacls 子进程。资源浪费且 Credential Manager 频繁写。
- **修复**:仅当 `username/mid/face` 变化时才 `save_session`;头像 data URL 按 URL 缓存。
- **置信度**:高(行为确凿,危害为资源浪费)

---

## P3 — 性能 / 代码质量

### [P3-1] `get_playlist`/`get_track_info` 每次全量读盘+解析+序列化含歌词的大 JSON
- **位置**:`commands.rs:1-11`;歌词内嵌于每条 track(`types.rs:36`)。
- **问题**:几千首曲库、每首几百行歌词时,library-cache.json 可达数十 MB;每次 IPC(含 `get_track_info` 查一首)都全量 `serde_json::from_slice` + 全量返回。建议内存缓存(带文件 mtime 失效)、歌词分文件存储、`get_playlist` 返回不含歌词的精简结构。置信度:高。

### [P3-2] `find_lyrics_file` 导致大目录导入 O(n²) 目录扫描
- **位置**:`media_library.rs:718-751`——每个音频文件都 `fs::read_dir(parent)` 全目录遍历找同名歌词。1 万文件的目录 = 1 亿次 entry 比较。建议按目录缓存一次 `read_dir` 结果(stem→lyrics path 的 HashMap)。置信度:高。

### [P3-3] `extract_qrc_lyric_content` 每次调用重新编译正则
- **位置**:`lyrics.rs:259-266`(`Regex::new` per call)。批量导入/在线歌词候选逐条解析时重复编译。用 `std::sync::OnceLock<Regex>`。置信度:高。

### [P3-4] `fetch_online_lyrics_from_sources` 三个源串行、每源最多 5 次歌词详情请求串行
- **位置**:`online_lyrics.rs:26-36` 及各 fetch 内循环。最坏 3×(1+5) 次请求 × 12s 超时串行。用 `tokio::join!` 并发三源,候选详情用 `futures::stream::iter(...).buffer_unordered(3)`。置信度:高。

### [P3-5] `import_bilibili_favorites` 长任务无进度事件、无取消、失败无退避
- **位置**:`commands.rs(bilibili):16-47`。200 首收藏夹串行导入可能跑几十分钟,前端只能干等一个 Promise;也无请求间隔(好在串行本身即限速)。建议按 ffmpeg 下载的模式 emit 进度事件 + 支持取消 flag。置信度:高。

### [P3-6] `handle_playback_ended` 非 loop 模式下队尾回绕到 0,整个队列无限循环
- **位置**:`state.rs:113-122` + `state.rs:209-214`(`(current_index+1) % len`)。`loop_mode=false` 时播完最后一首会从头继续,永不停止;若产品语义中 `loop_mode` 是"单曲循环"、默认行为即"列表循环",则非 bug。请与前端语义核对。置信度:低。

### [P3-7] 死代码与重复分支
- `session.rs:305-309`:`strip_prefix(|c| c.eq_ignore_ascii_case(&'M'))` + `let _ = rest;` 是无效残留代码,且会误剥非 Max-Age 属性的首字符后丢弃(无实际影响但迷惑读者),应删除。
- `impls_and_tests.rs:44-45`:`"EAC3" if remuxed => "eac3", "EAC3" => "eac3"` 两臂相同,guard 无意义。
- 置信度:高。

### [P3-8] `pseudo_random_index` 用纳秒时间戳取模做 shuffle
- **位置**:`state.rs:269-278`。快速连点"下一首"时间接近可能选中相同索引;`recent_track_ids` 机制部分缓解。建议 `fastrand` 或简单 xorshift 状态。置信度:高(质量问题,非错误)。

### [P3-9] CSP `img-src https:` 放开全部 HTTPS 图源
- **位置**:`tauri.conf.json:30`。封面/头像已转 data URL,`https:` 通配可收紧为 `https://*.hdslb.com`。capabilities(`capabilities/default.json`)整体较紧,`core:webview:allow-get-all-webviews` 若前端未用可移除。置信度:中。

### [P3-10] 更换缓存目录不迁移旧文件,旧目录成为不受配额管理的孤儿
- **位置**:`cache.rs:62-90`(`update_cache_settings` 只改路径)。旧缓存文件仍被曲目引用可播,但永不参与新目录的配额清理。建议提示用户或提供迁移。置信度:高。

### [P3-11] `import_tracks` 单个子目录读失败即中止整个导入
- **位置**:`media_library.rs:21-26`(`fs::read_dir(...)?`、`entry.map_err(...)?` 向上冒泡)。遇到一个无权限子目录,整批导入失败。建议跳过并累积警告。置信度:高。

---

## 排查点核对小结(未发现问题的项)
- **路径遍历/删除源文件**:`delete_track` 只改缓存 JSON、不删磁盘文件——安全;`clear_cache` 有 marker+扩展名双重防护(除 P2-3 的 junction 例外)。
- **命令注入**:ffmpeg/icacls 均用 `Command::arg` 传参,无 shell 拼接;`hide_console_window` 正确。
- **凭据存储**:cookie 已入 Windows Credential Manager,session 文件不落 cookie(`write_session_file` 恒传 `include_cookies=false`),含旧文件迁移逻辑——设计良好(仅 P2-6 的 ACL 瑕疵)。
- **证书校验**:reqwest 默认校验,未发现 `danger_accept_invalid_certs`。
- **歌词解析**:BOM/UTF-16/GBK/畸形时间戳/zlib bomb 防御完善;`inflate_zlib_utf8` 上限判断用 `text.len()>=MAX` 略有 off-by-one 风格问题但无实害。
- **异步阻塞**:`import_tracks` 已用 `spawn_blocking`(L-18);bilibili 下载为流式 async;未发现跨 `await` 持有 Mutex(parking_lot 锁均在同步块内释放)。
- **B 站 API 解析**:全部 `Option`+`filter_map`,无字段缺失 panic;`parse_json_response` 错误带 240 字符预览,不泄漏本地路径。

---

## 完整已读文件清单
1. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\library\media_library.rs`(751 行)
2. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\library\lyrics.rs`(747 行)
3. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\library\online_lyrics.rs`(453 行)
4. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\library\commands.rs` / `metadata.rs` / `types.rs` / `imports.rs` / `tests.rs`
5. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\library.rs`(include! 汇总)
6. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\bilibili\import_audio.rs`(643 行)
7. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\bilibili\session.rs`(394 行)
8. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\bilibili\commands.rs` / `impls_and_tests.rs` / `types.rs` / `ffmpeg.rs` / `parsing.rs` / `imports.rs` / `constants.rs`
9. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\bilibili.rs`(include! 汇总)
10. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\cache.rs`(518 行)
11. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\playback.rs`(136 行)
12. `D:\_SeraphAudioPlayer\src-tauri\src\ipc\events.rs`、`src\ipc\mod.rs`
13. `D:\_SeraphAudioPlayer\src-tauri\src\state.rs`(279 行)
14. `D:\_SeraphAudioPlayer\src-tauri\src\lib.rs`、`src\main.rs`
15. `D:\_SeraphAudioPlayer\src-tauri\Cargo.toml`、`tauri.conf.json`、`capabilities\default.json`

**优先修复顺序建议**:P0-1(Cookie 泄漏)→ P0-2(曲库丢失链)→ P1-1(30s 超时,用户可感知度最高)→ P1-2/P1-3 → P1-4/P1-5 → P2 系列。