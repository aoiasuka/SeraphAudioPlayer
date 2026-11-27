//! 本地歌词目录匹配（设置 → 歌词设置 → 本地歌词目录）。
//!
//! 目标文件形如 LDDC 导出的 `艺术家 - 曲名 (65923804).lrc`：按「艺术家 - 曲名」与曲目
//! 元数据归一化比对（去括号版本后缀、大小写、全半角、空白），括号里的纯数字是 LDDC 写的
//! **来源平台**歌曲 ID（网易云 / QQ / 酷狗都可能）→ 记为 `ncm-lyrics/<id>` 与 `qq-lyrics/<id>`
//! 两个查找键，供 AMLL TTML 直取，命中后由文件头元数据核对曲目身份。
//!
//! 扫顶层 + 直接子目录（深度 1，跳过符号链接目录）、只看歌词扩展名、文件大小沿用 4 MB
//! 上限；目录本身必须是用户在设置里选过的绝对路径且不含 `..` 点段。

use super::prelude::*;

const LYRIC_EXTENSIONS: &[&str] = &["lrc", "qrc", "krc", "yrc", "ttml"];
/// 单次扫描的目录条目上限，防止误选了超大目录把一次切歌拖成秒级。
const MAX_DIR_ENTRIES: usize = 20_000;
const MAX_LOCAL_LYRICS_BYTES: u64 = 4 * 1024 * 1024;
/// LLY-01：文件名索引最长沿用多久。目录修改时间没变也到点重建一次——个别文件系统
/// （部分网络共享）更新目录时间不及时，这是给它们的兜底。
const INDEX_MAX_AGE: Duration = Duration::from_secs(300);
/// LLY-01：目录的修改时间要比扫描时刻早这么多，建出来的索引才沿用（原因见
/// `FolderIndex::is_current_for`）。2 秒覆盖 FAT / 常见网络共享的时间戳粒度。
/// 代价只是：目录刚改动后的 2 秒内，每次查找仍然重新枚举（与改动前的行为相同）。
const INDEX_SETTLE_MARGIN: Duration = Duration::from_secs(2);

pub(crate) fn validate_lyrics_folder(raw: &str) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("歌词目录为空".into());
    }
    let path = PathBuf::from(trimmed);
    if !path.is_absolute() {
        return Err("歌词目录必须是绝对路径".into());
    }
    if path
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err("歌词目录不得包含 .. 点段".into());
    }
    if !path.is_dir() {
        return Err("歌词目录不存在".into());
    }
    Ok(path)
}

/// 归一化：NFKC 近似（全角 ASCII → 半角）、小写、去空白与常见标点。
pub(crate) fn normalize_for_match(value: &str) -> String {
    value
        .chars()
        .filter_map(|ch| {
            let ch = match ch as u32 {
                // 全角 ASCII 区
                0xFF01..=0xFF5E => char::from_u32(ch as u32 - 0xFF01 + 0x21).unwrap_or(ch),
                0x3000 => ' ',
                _ => ch,
            };
            if ch.is_whitespace()
                || ch.is_ascii_punctuation()
                || matches!(
                    ch,
                    '·' | '・'
                        | '、'
                        | '。'
                        | '，'
                        | '：'
                        | '；'
                        | '！'
                        | '？'
                        | '「'
                        | '」'
                        | '『'
                        | '』'
                )
            {
                None
            } else {
                Some(ch.to_lowercase().next().unwrap_or(ch))
            }
        })
        .collect()
}

/// 去掉尾部括号段（版本后缀 / ID），例如 `唯一 (国语)` → `唯一`，可反复剥。
pub(crate) fn strip_trailing_parens(value: &str) -> &str {
    let mut current = value.trim();
    loop {
        let Some(open) = current.rfind(['(', '（']) else {
            return current;
        };
        let tail = &current[open..];
        if tail.ends_with([')', '）']) {
            current = current[..open].trim_end();
        } else {
            return current;
        }
    }
}

/// 文件名 stem → (艺术家, 曲名, 网易云 ID)。
pub(crate) fn parse_lyrics_file_stem(
    stem: &str,
) -> Option<(Option<String>, String, Option<String>)> {
    // 尾部 `(数字)` 是平台 ID
    let mut ncm_id = None;
    let mut base = stem.trim();
    if let Some(open) = base.rfind(['(', '（']) {
        let inner = base[open..]
            .trim_start_matches(['(', '（'])
            .trim_end_matches([')', '）'])
            .trim();
        if !inner.is_empty()
            && inner.chars().all(|ch| ch.is_ascii_digit())
            && base.ends_with([')', '）'])
        {
            ncm_id = Some(inner.to_string());
            base = base[..open].trim_end();
        }
    }
    if base.is_empty() {
        return None;
    }
    match base.split_once(" - ") {
        Some((artist, title)) => Some((
            Some(artist.trim().to_string()),
            title.trim().to_string(),
            ncm_id,
        )),
        None => Some((None, base.to_string(), ncm_id)),
    }
}

/// 曲目侧的归一化结果，整个目录扫描只算一次（此前每个候选文件都重算 4 次曲名 / 艺术家归一化，
/// 2 万条目的目录就是 8 万次分配）。
struct TrackKey {
    title_full: String,
    title_base: String,
    artist_norm: String,
    artist_parts: Vec<String>,
}

impl TrackKey {
    fn new(track_title: &str, track_artist: &str) -> Self {
        Self {
            title_full: normalize_for_match(track_title),
            title_base: normalize_for_match(strip_trailing_parens(track_title)),
            artist_norm: normalize_for_match(track_artist),
            artist_parts: track_artist
                .split(['/', '&', ',', '，', '、', ';'])
                .map(normalize_for_match)
                .collect(),
        }
    }

    fn artist_matches(&self, file_artist: &str) -> bool {
        let file = normalize_for_match(file_artist);
        if file.is_empty() {
            return true;
        }
        let track = &self.artist_norm;
        if track.is_empty() || track == "unknown" {
            return true;
        }
        // 多艺术家 `A / B`、`A&B` 任一匹配即可
        *track == file
            || track.contains(&file)
            || file.contains(track.as_str())
            || self.artist_parts.contains(&file)
    }

    /// 匹配分：0 = 不匹配；越高越优先。
    #[cfg(test)]
    fn score(&self, file_artist: Option<&str>, file_title: &str) -> u8 {
        self.score_normalized(
            file_artist,
            &normalize_for_match(file_title),
            &normalize_for_match(strip_trailing_parens(file_title)),
        )
    }

    /// 同 [`Self::score`]，文件侧的两种曲名归一化结果由调用方给出（索引里建好的）。
    fn score_normalized(&self, file_artist: Option<&str>, file_full: &str, file_base: &str) -> u8 {
        if self.title_full.is_empty() || file_full.is_empty() {
            return 0;
        }
        let title_score = if file_full == self.title_full {
            3
        } else if file_base == self.title_base {
            2
        } else {
            return 0;
        };
        match file_artist {
            Some(artist) if self.artist_matches(artist) => title_score + 2,
            // 文件名带艺术家但对不上 → 不认
            Some(_) => 0,
            None => title_score,
        }
    }
}

/// 索引里的一个歌词文件：文件名解析与归一化在建索引时做完，查找时只比较。
struct IndexedLyricsFile {
    path: PathBuf,
    is_ttml: bool,
    artist: Option<String>,
    title_full: String,
    title_base: String,
    platform_id: Option<String>,
}

/// 一个歌词目录的文件名索引（LLY-01）。此前每次查找——前端在当前曲目没有歌词时调用，
/// 也就是每次切到没歌词的歌——都把顶层和所有直接子目录枚举一遍（上限 2 万条）并逐个
/// 解析文件名。现在枚举一次，之后只要顶层和各子目录的修改时间没变就沿用。
///
/// 索引只记文件名，不记内容：歌词文件被改写后读到的仍是新内容。目录里增删、改名文件会
/// 更新该目录的修改时间，索引随之重建；另有 [`INDEX_MAX_AGE`] 兜底。
struct FolderIndex {
    folder: PathBuf,
    /// 顶层目录与各直接子目录在建索引时的修改时间
    stamps: Vec<(PathBuf, Option<std::time::SystemTime>)>,
    /// 开始枚举时的系统时间，用来判断各目录在那之前是否已经「静置」够久
    scanned_at: std::time::SystemTime,
    built: std::time::Instant,
    /// 顶层文件在前、子目录文件在后（同分时先见者胜）
    files: Vec<IndexedLyricsFile>,
}

fn dir_modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

impl FolderIndex {
    /// 扫顶层 + 直接子目录（深度 1，不再往下），跳过符号链接目录（不跟着链接跑出用户选的
    /// 目录），总条目受 `MAX_DIR_ENTRIES` 封顶。顶层读不了返回 `None`。
    fn build(folder: &Path) -> Option<Self> {
        Self::build_at(folder, std::time::SystemTime::now())
    }

    /// `scanned_at` 是「现在」的系统时间；单独成参数只为让测试能模拟「过了一阵才扫描」。
    fn build_at(folder: &Path, scanned_at: std::time::SystemTime) -> Option<Self> {
        // 修改时间在枚举**之前**取：枚举期间目录又变了的话，下次比较必然不等，会重建
        let mut stamps = vec![(folder.to_path_buf(), dir_modified(folder))];
        let entries = fs::read_dir(folder).ok()?;
        let mut files = Vec::new();
        let mut budget = MAX_DIR_ENTRIES;
        let mut subdirs: Vec<PathBuf> = Vec::new();

        for entry in entries.flatten() {
            if budget == 0 {
                break;
            }
            budget -= 1;
            // `entry.file_type()` 不跟随符号链接：链接目录直接跳过，链接文件也不认
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                subdirs.push(entry.path());
                continue;
            }
            files.extend(index_lyrics_file(entry.path()));
        }

        for subdir in subdirs {
            if budget == 0 {
                break;
            }
            let stamp = dir_modified(&subdir);
            let Ok(entries) = fs::read_dir(&subdir) else {
                continue;
            };
            stamps.push((subdir, stamp));
            for entry in entries.flatten() {
                if budget == 0 {
                    break;
                }
                budget -= 1;
                // 子目录里只看普通文件，不再下钻
                if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                files.extend(index_lyrics_file(entry.path()));
            }
        }

        Some(Self {
            folder: folder.to_path_buf(),
            stamps,
            scanned_at,
            built: std::time::Instant::now(),
            files,
        })
    }

    /// 这份索引对 `folder` 是否仍然可用：没过期，每个目录的修改时间都与建索引时相同，
    /// **而且**那个修改时间比扫描时刻早至少 [`INDEX_SETTLE_MARGIN`]。
    ///
    /// 最后一条不能省。文件时间戳的粒度有限（Windows 按系统时钟节拍打时间，约 1～16 ms；
    /// FAT 与部分网络共享是 1～2 秒）：扫描之后、同一个节拍之内又有文件加进来时，目录的
    /// 修改时间与扫描时记下的完全相同，光比较时间会把一份已经过时的索引当成有效的。
    /// 所以扫描时目录「刚刚动过」的索引不沿用，下次查找重建；目录静置超过这段余量之后
    /// 建的索引才算数。取不到修改时间的目录（已被删掉、一时读不了）一律算变了；
    /// 修改时间比本机时钟还晚的（网络共享与本机时钟不一致）同样不沿用。
    fn is_current_for(&self, folder: &Path) -> bool {
        self.folder == folder
            && self.built.elapsed() < INDEX_MAX_AGE
            && self.stamps.iter().all(|(path, stamp)| {
                let Some(stamp) = *stamp else {
                    return false;
                };
                dir_modified(path) == Some(stamp)
                    && stamp
                        .checked_add(INDEX_SETTLE_MARGIN)
                        .is_some_and(|settled| settled <= self.scanned_at)
            })
    }

    fn best_match(&self, key: &TrackKey) -> Option<(PathBuf, Vec<String>)> {
        let mut best: Option<(u8, &IndexedLyricsFile)> = None;
        for file in &self.files {
            let score =
                key.score_normalized(file.artist.as_deref(), &file.title_full, &file.title_base);
            if score == 0 {
                continue;
            }
            // 同分时优先 .ttml（逐字），其次先见者（顶层先于子目录）
            let total = score * 2 + u8::from(file.is_ttml);
            if best.is_none_or(|(current, _)| total > current) {
                best = Some((total, file));
            }
        }
        best.map(|(_, file)| {
            (
                file.path.clone(),
                lookup_keys_for_platform_id(file.platform_id.as_deref()),
            )
        })
    }
}

/// 单个文件：扩展名过滤 → 文件名解析 → 归一化。不是歌词文件返回 `None`。
fn index_lyrics_file(path: PathBuf) -> Option<IndexedLyricsFile> {
    let ext = path.extension().and_then(|value| value.to_str())?;
    if !LYRIC_EXTENSIONS
        .iter()
        .any(|allowed| ext.eq_ignore_ascii_case(allowed))
    {
        return None;
    }
    let is_ttml = ext.eq_ignore_ascii_case("ttml");
    let stem = path.file_stem().and_then(|value| value.to_str())?;
    let (artist, title, platform_id) = parse_lyrics_file_stem(stem)?;
    Some(IndexedLyricsFile {
        is_ttml,
        artist,
        title_full: normalize_for_match(&title),
        title_base: normalize_for_match(strip_trailing_parens(&title)),
        platform_id,
        path,
    })
}

/// 最近用过的那个歌词目录的索引（设置里只有一个歌词目录）。
static FOLDER_INDEX: parking_lot::Mutex<Option<std::sync::Arc<FolderIndex>>> =
    parking_lot::Mutex::new(None);

/// 取 `folder` 的索引：手里的还能用就直接用，否则重建。
fn folder_index(folder: &Path) -> Option<std::sync::Arc<FolderIndex>> {
    let cached = FOLDER_INDEX.lock().clone();
    if let Some(index) = cached.filter(|index| index.is_current_for(folder)) {
        return Some(index);
    }
    // 重建在锁外做（可能要枚举上万个目录项）；两个线程同时重建时各建各的，后写的留下
    let index = std::sync::Arc::new(FolderIndex::build(folder)?);
    *FOLDER_INDEX.lock() = Some(std::sync::Arc::clone(&index));
    Some(index)
}

/// 匹配分：0 = 不匹配；越高越优先（测试入口；扫描路径用 `TrackKey` 复用归一化结果）。
#[cfg(test)]
fn match_score(
    file_artist: Option<&str>,
    file_title: &str,
    track_title: &str,
    track_artist: &str,
) -> u8 {
    TrackKey::new(track_title, track_artist).score(file_artist, file_title)
}

/// 在目录里找最匹配的歌词文件（顶层 + 直接子目录）。文件名索引按目录修改时间缓存，
/// 见 [`FolderIndex`]。
pub(crate) fn find_in_folder(
    folder: &Path,
    track_title: &str,
    track_artist: &str,
) -> Option<(PathBuf, Vec<String>)> {
    folder_index(folder)?.best_match(&TrackKey::new(track_title, track_artist))
}

/// 文件名括号里的数字是 LDDC `%<id>`——**歌词来源平台**的歌曲 ID，文件头不记来源：
/// 同一目录里既有网易云 id、也有 QQ songid 与酷狗 album_audio_id（2026-09-19 用户目录实测两份
/// 文件分别是酷狗与 QQ 的 ID）。所以对 AMLL 两个目录各生成一个查找键，命中后由
/// `amll::ttml_meta_matches` 用文件头曲名 / 艺术家核对，错平台的键要么 404 要么被拒。
/// 酷狗 ID 在 AMLL 没有目录，只能靠标题匹配。
pub(crate) fn lookup_keys_for_platform_id(id: Option<&str>) -> Vec<String> {
    id.map(|id| vec![format!("ncm-lyrics/{id}"), format!("qq-lyrics/{id}")])
        .unwrap_or_default()
}

pub(crate) fn read_local_lyrics(path: &Path) -> Option<LyricDocument> {
    if fs::metadata(path)
        .map(|meta| meta.len() > MAX_LOCAL_LYRICS_BYTES)
        .unwrap_or(true)
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let ext = path.extension().and_then(|value| value.to_str());
    let (lyrics, offset_ms) = parse_lyrics_file_bytes_with_offset(ext, &bytes);
    (!lyrics.is_empty()).then(|| {
        LyricDocument::from_lines(lyrics, LyricSource::of(LyricSourceKind::Folder))
            .with_offset(offset_ms)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lddc_file_stem() {
        assert_eq!(
            parse_lyrics_file_stem("王力宏 - 唯一 (65923804)"),
            Some((
                Some("王力宏".into()),
                "唯一".into(),
                Some("65923804".into())
            ))
        );
        assert_eq!(
            parse_lyrics_file_stem("美波 - 水中リフレクション (546585623)"),
            Some((
                Some("美波".into()),
                "水中リフレクション".into(),
                Some("546585623".into())
            ))
        );
        // 括号里不是纯数字 → 保留为标题一部分
        assert_eq!(
            parse_lyrics_file_stem("王力宏 - 唯一 (国语)"),
            Some((Some("王力宏".into()), "唯一 (国语)".into(), None))
        );
        assert_eq!(
            parse_lyrics_file_stem("唯一"),
            Some((None, "唯一".into(), None))
        );
        assert_eq!(parse_lyrics_file_stem("  "), None);
    }

    #[test]
    fn scoring_tolerates_version_suffix_fullwidth_and_multi_artist() {
        assert!(match_score(Some("王力宏"), "唯一", "唯一 (国语)", "王力宏") > 0);
        assert!(match_score(Some("王力宏"), "唯一", "唯　一", "王力宏 / 张三") > 0);
        assert!(
            match_score(
                Some("美波"),
                "水中リフレクション",
                "水中リフレクション",
                "Unknown"
            ) > 0
        );
        assert!(match_score(None, "唯一", "唯一", "王力宏") > 0);
        // 艺术家不符 / 曲名不符 → 0
        assert_eq!(match_score(Some("周杰伦"), "唯一", "唯一", "王力宏"), 0);
        assert_eq!(match_score(Some("王力宏"), "唯二", "唯一", "王力宏"), 0);
        // 全匹配 > 去后缀匹配
        assert!(
            match_score(Some("王力宏"), "唯一 (国语)", "唯一 (国语)", "王力宏")
                > match_score(Some("王力宏"), "唯一", "唯一 (国语)", "王力宏")
        );
    }

    #[test]
    fn finds_best_file_and_extracts_lookup_key() {
        let dir = std::env::temp_dir().join(format!("seraph-lyrics-folder-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("王力宏 - 唯一 (65923804).lrc"), "[00:01.00]a").unwrap();
        fs::write(dir.join("周杰伦 - 唯一.lrc"), "[00:01.00]b").unwrap();
        fs::write(dir.join("readme.txt"), "x").unwrap();

        let (path, keys) = find_in_folder(&dir, "唯一 (国语)", "王力宏").expect("match");
        assert!(path.ends_with("王力宏 - 唯一 (65923804).lrc"));
        // 括号数字的平台未知：网易云 / QQ 两个目录各一个键（酷狗 ID 无目录）
        assert_eq!(keys, ["ncm-lyrics/65923804", "qq-lyrics/65923804"]);
        assert!(find_in_folder(&dir, "别的歌", "王力宏").is_none());

        let lyrics = read_local_lyrics(&path).unwrap();
        assert_eq!(lyrics.lines[0].text, "a");
        assert_eq!(lyrics.source.kind, LyricSourceKind::Folder);

        // 直接子目录命中（深度 1）；孙目录不扫
        fs::create_dir_all(dir.join("sub").join("deeper")).unwrap();
        fs::write(
            dir.join("sub").join("周杰伦 - 晴天 (186016).lrc"),
            "[00:02.00]c",
        )
        .unwrap();
        fs::write(
            dir.join("sub").join("deeper").join("周杰伦 - 稻香.lrc"),
            "[00:03.00]d",
        )
        .unwrap();
        let (path, keys) = find_in_folder(&dir, "晴天", "周杰伦").expect("subdir match");
        assert!(path.ends_with("周杰伦 - 晴天 (186016).lrc"));
        assert_eq!(keys, ["ncm-lyrics/186016", "qq-lyrics/186016"]);
        assert!(find_in_folder(&dir, "稻香", "周杰伦").is_none());
        // 顶层与子目录同分时顶层先见者胜
        fs::write(dir.join("sub").join("王力宏 - 唯一.lrc"), "[00:01.00]e").unwrap();
        let (path, _) = find_in_folder(&dir, "唯一", "王力宏").expect("match");
        assert!(path.ends_with("王力宏 - 唯一 (65923804).lrc"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_index_is_reused_until_a_directory_changes() {
        // LLY-01：目录没变时不再重新枚举；增删文件（含子目录里的）后重建。
        // 这里直接用 `FolderIndex`，不经进程级缓存——别的测试并行时也在用那个缓存。
        let dir = std::env::temp_dir().join(format!("seraph-lyrics-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("王力宏 - 唯一.lrc"), "[00:01.00]a").unwrap();
        fs::write(dir.join("cover.jpg"), "x").unwrap();

        // 「目录静置了一阵之后才扫描」：把扫描时刻拨到 10 秒后来模拟，不真的等
        let later = || std::time::SystemTime::now() + Duration::from_secs(10);

        let index = FolderIndex::build_at(&dir, later()).unwrap();
        assert_eq!(index.files.len(), 1, "只索引歌词文件");
        assert_eq!(index.stamps.len(), 2, "顶层与一个子目录");
        assert!(index.is_current_for(&dir), "目录没变，索引可以沿用");
        assert!(!index.is_current_for(&dir.join("sub")), "不用于别的目录");
        let key = TrackKey::new("晴天", "周杰伦");
        assert!(index.best_match(&key).is_none());

        // 目录刚刚动过就扫描出来的索引不沿用：同一个时钟节拍内再加文件，修改时间看不出变化
        let racy = FolderIndex::build(&dir).unwrap();
        assert!(!racy.is_current_for(&dir), "扫描时目录还没静置够久");

        // 隔一小会儿再写，让时间戳确实前进
        std::thread::sleep(Duration::from_millis(50));
        fs::write(dir.join("sub").join("周杰伦 - 晴天.ttml"), "<tt/>").unwrap();
        assert!(!index.is_current_for(&dir), "子目录里多了文件");
        let index = FolderIndex::build_at(&dir, later()).unwrap();
        let (path, _) = index.best_match(&key).expect("重建后命中新文件");
        assert!(path.ends_with("周杰伦 - 晴天.ttml"));
        assert!(index.is_current_for(&dir));

        std::thread::sleep(Duration::from_millis(50));
        fs::remove_file(dir.join("王力宏 - 唯一.lrc")).unwrap();
        assert!(!index.is_current_for(&dir), "顶层少了文件");

        // 经公开入口、不留任何间隔地连续增删：每一步都立即反映出来。
        // （正是这种「建完索引马上又改目录」的序列，只比较修改时间时会读到旧结果。）
        assert!(find_in_folder(&dir, "唯一", "王力宏").is_none());
        assert!(find_in_folder(&dir, "晴天", "周杰伦").is_some());
        for round in 0..20 {
            let name = format!("歌手 - 曲目{round}.lrc");
            fs::write(dir.join(&name), "[00:01.00]x").unwrap();
            let title = format!("曲目{round}");
            assert!(find_in_folder(&dir, &title, "歌手").is_some(), "{name}");
            fs::remove_file(dir.join(&name)).unwrap();
            assert!(find_in_folder(&dir, &title, "歌手").is_none(), "{name}");
        }

        // 目录整个没了：索引失效，查找返回 None 而不是旧结果
        let stale = FolderIndex::build_at(&dir, later()).unwrap();
        assert!(stale.is_current_for(&dir));
        fs::remove_dir_all(&dir).unwrap();
        assert!(!stale.is_current_for(&dir));
        assert!(find_in_folder(&dir, "晴天", "周杰伦").is_none());
    }

    #[test]
    fn folder_validation_rejects_relative_dotdot_and_missing() {
        assert!(validate_lyrics_folder("").is_err());
        assert!(validate_lyrics_folder("Lyrics").is_err());
        assert!(validate_lyrics_folder(r"C:\a\..\b").is_err());
        assert!(validate_lyrics_folder(r"C:\definitely\missing\dir\seraph").is_err());
        assert!(validate_lyrics_folder(&std::env::temp_dir().to_string_lossy()).is_ok());
    }
}
