//! 本地歌词目录匹配（设置 → 歌词设置 → 本地歌词目录）。
//!
//! 目标文件形如 LDDC 导出的 `艺术家 - 曲名 (65923804).lrc`：按「艺术家 - 曲名」与曲目
//! 元数据归一化比对（去括号版本后缀、大小写、全半角、空白），括号里的纯数字视为
//! 网易云歌曲 ID → 记为 `ncm-lyrics/<id>` 查找键，供 AMLL TTML 直取。
//!
//! 只扫一层目录、只看歌词扩展名、文件大小沿用 4 MB 上限；目录本身必须是用户在设置里
//! 选过的绝对路径且不含 `..` 点段。

use super::prelude::*;

const LYRIC_EXTENSIONS: &[&str] = &["lrc", "qrc", "krc", "yrc", "ttml"];
/// 单次扫描的目录条目上限，防止误选了超大目录把一次切歌拖成秒级。
const MAX_DIR_ENTRIES: usize = 20_000;
const MAX_LOCAL_LYRICS_BYTES: u64 = 4 * 1024 * 1024;

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
fn strip_trailing_parens(value: &str) -> &str {
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

fn artist_matches(file_artist: &str, track_artist: &str) -> bool {
    let file = normalize_for_match(file_artist);
    if file.is_empty() {
        return true;
    }
    let track = normalize_for_match(track_artist);
    if track.is_empty() || track == "unknown" {
        return true;
    }
    // 多艺术家 `A / B`、`A&B` 任一匹配即可
    track == file
        || track.contains(&file)
        || file.contains(&track)
        || track_artist
            .split(['/', '&', ',', '，', '、', ';'])
            .any(|part| normalize_for_match(part) == file)
}

/// 匹配分：0 = 不匹配；越高越优先。
fn match_score(
    file_artist: Option<&str>,
    file_title: &str,
    track_title: &str,
    track_artist: &str,
) -> u8 {
    let title_full = normalize_for_match(track_title);
    let title_base = normalize_for_match(strip_trailing_parens(track_title));
    let file_full = normalize_for_match(file_title);
    let file_base = normalize_for_match(strip_trailing_parens(file_title));
    if title_full.is_empty() || file_full.is_empty() {
        return 0;
    }
    let title_score = if file_full == title_full {
        3
    } else if file_base == title_base {
        2
    } else {
        0
    };
    if title_score == 0 {
        return 0;
    }
    match file_artist {
        Some(artist) if artist_matches(artist, track_artist) => title_score + 2,
        // 文件名带艺术家但对不上 → 不认
        Some(_) => 0,
        None => title_score,
    }
}

/// 在目录里找最匹配的歌词文件。
pub(crate) fn find_in_folder(
    folder: &Path,
    track_title: &str,
    track_artist: &str,
) -> Option<(PathBuf, Vec<String>)> {
    let entries = fs::read_dir(folder).ok()?;
    let mut best: Option<(u8, PathBuf, Vec<String>)> = None;
    for entry in entries.flatten().take(MAX_DIR_ENTRIES) {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if !LYRIC_EXTENSIONS
            .iter()
            .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some((artist, title, ncm_id)) = parse_lyrics_file_stem(stem) else {
            continue;
        };
        let score = match_score(artist.as_deref(), &title, track_title, track_artist);
        if score == 0 {
            continue;
        }
        // 同分时优先 .ttml（逐字），其次先见者
        let ext_bonus = u8::from(ext.eq_ignore_ascii_case("ttml"));
        let total = score * 2 + ext_bonus;
        if best.as_ref().is_none_or(|(current, ..)| total > *current) {
            let keys = ncm_id
                .map(|id| vec![format!("ncm-lyrics/{id}")])
                .unwrap_or_default();
            best = Some((total, path, keys));
        }
    }
    best.map(|(_, path, keys)| (path, keys))
}

pub(crate) fn read_local_lyrics(path: &Path) -> Option<Vec<LyricLine>> {
    if fs::metadata(path)
        .map(|meta| meta.len() > MAX_LOCAL_LYRICS_BYTES)
        .unwrap_or(true)
    {
        return None;
    }
    let bytes = fs::read(path).ok()?;
    let is_ttml = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ttml"));
    let lyrics = if is_ttml {
        clamp_lyrics(super::ttml::parse_ttml_lyrics(&decode_lyric_bytes(&bytes)))
    } else {
        parse_lyrics_bytes(&bytes)
    };
    (!lyrics.is_empty()).then_some(lyrics)
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
        assert_eq!(keys, ["ncm-lyrics/65923804"]);
        assert!(find_in_folder(&dir, "别的歌", "王力宏").is_none());

        let lyrics = read_local_lyrics(&path).unwrap();
        assert_eq!(lyrics[0].text, "a");
        let _ = fs::remove_dir_all(&dir);
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
