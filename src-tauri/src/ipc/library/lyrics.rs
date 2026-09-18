use super::prelude::*;

pub(crate) const QRC_MAGIC_HEADER: &[u8] = b"\x98%\xb0\xac\xe3\x02\x83h\xe8\xfcl";
pub(crate) const KRC_MAGIC_HEADER: &[u8] = b"krc18";
pub(crate) const QRC_KEY: &[u8] = b"!@#)(*$%123ZXC!@!@#)(NHL";
pub(crate) const KRC_KEY: &[u8] = b"@Gaw^2tGQ61-\xce\xd2ni";
pub(crate) const QMC1_PRIVKEY: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];

/// W-01：歌词总行数上限。原先只有 4 MB 总字节上限，几万行的 LRC 会让
/// LyricsPanel 一次性渲染同样多的 `<p>`（无虚拟化），主窗口直接卡死。
/// 正常整首歌不超过几百行，5000 行已是极宽裕的余量。
pub(crate) const MAX_LYRIC_LINES: usize = 5_000;
/// W-01：单行字符数上限。TypewriterText 按 30 ms/字符逐字 `slice`（每次 O(n) 重建），
/// 单行接近 2 MB 时既跑不完（>16 小时）又每 30 ms 重建一次巨串 → UI 永久冻结。
pub(crate) const MAX_LYRIC_LINE_CHARS: usize = 512;

pub(crate) fn parse_lyrics_bytes(bytes: &[u8]) -> Vec<LyricLine> {
    clamp_lyrics(parse_lyrics_bytes_inner(bytes))
}

/// W-01：所有歌词来源（本地导入 / 在线抓取 / 外部 .lrc）都经 `parse_lyrics_bytes`
/// 收口，所以上限只需在这一处施加。**新增歌词解析路径务必也走这里**。
pub(crate) fn clamp_lyrics(mut lyrics: Vec<LyricLine>) -> Vec<LyricLine> {
    lyrics.truncate(MAX_LYRIC_LINES);
    for line in &mut lyrics {
        // 按字符截断而不是字节，避免把多字节字符切成半个（中文歌词必踩）
        if line.text.chars().count() > MAX_LYRIC_LINE_CHARS {
            line.text = line.text.chars().take(MAX_LYRIC_LINE_CHARS).collect();
        }
    }
    lyrics
}

fn parse_lyrics_bytes_inner(bytes: &[u8]) -> Vec<LyricLine> {
    if bytes.starts_with(QRC_MAGIC_HEADER) {
        if let Some(lyrics) = parse_encrypted_qrc_lyrics(bytes) {
            return lyrics;
        }
    }

    if bytes.starts_with(KRC_MAGIC_HEADER) {
        if let Some(lyrics) = parse_encrypted_krc_lyrics(bytes) {
            return lyrics;
        }
    }

    let text = decode_lyric_bytes(bytes);

    // 手动导入（`save_track_lyrics`）只拿到裸字节没有扩展名：先按内容嗅探 TTML，
    // 命中且解析非空即返回；空则继续走三方/LRC 解析，不影响既有路径。
    if looks_like_ttml(&text) {
        let ttml_lyrics = super::ttml::parse_ttml_lyrics(&text);
        if !ttml_lyrics.is_empty() {
            return ttml_lyrics;
        }
    }

    let provider_lyrics = parse_provider_lyrics_text(&text);
    if !provider_lyrics.is_empty() {
        return provider_lyrics;
    }

    parse_lyrics_text(&text)
}

/// TTML 内容嗅探：去 BOM/前导空白后以 `<?xml` 或 `<tt` 开头，且开头一段里含 `<tt`
/// （大小写不敏感）。只看前 8 K 字符，避免为嗅探把 4 MB 文本整份小写化。
pub(crate) fn looks_like_ttml(text: &str) -> bool {
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let head = trimmed
        .chars()
        .take(8 * 1024)
        .collect::<String>()
        .to_ascii_lowercase();
    (head.starts_with("<?xml") || head.starts_with("<tt")) && head.contains("<tt")
}

/// 按扩展名分派的歌词文件解析：`.ttml` 走 TTML 解析（经 `clamp_lyrics` 收口），
/// 其余交给 `parse_lyrics_bytes`。同名 sidecar 与本地歌词目录两处复用。
pub(crate) fn parse_lyrics_file_bytes(path_ext: Option<&str>, bytes: &[u8]) -> Vec<LyricLine> {
    if path_ext.is_some_and(|ext| ext.eq_ignore_ascii_case("ttml")) {
        return clamp_lyrics(super::ttml::parse_ttml_lyrics(&decode_lyric_bytes(bytes)));
    }
    parse_lyrics_bytes(bytes)
}

/// 判定一组歌词是否是「纯文本合成」的假时间轴：`parse_lyrics_text` 对无时间戳歌词
/// 按 `index * 4s` 铺时间，因此全部行恰为 4 秒等差、且没有 words/end 即视为未同步。
/// 空列表返回 false；单行 `[00:00.00]` 的真 LRC 与单行纯文本无法区分，一律按未同步算。
// 供后续「纯文本歌词不滚动/提示」逻辑调用，目前只有测试引用。
#[allow(dead_code)]
pub(crate) fn lyrics_are_unsynced(lyrics: &[LyricLine]) -> bool {
    !lyrics.is_empty()
        && lyrics.iter().enumerate().all(|(index, line)| {
            (line.time - index as f64 * 4.0).abs() < 1e-6
                && line.words.is_none()
                && line.end.is_none()
        })
}

pub(crate) fn parse_encrypted_qrc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_qrc(bytes).ok()?;
    let lyrics = parse_qrc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub(crate) fn parse_encrypted_krc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_krc(bytes).ok()?;
    let lyrics = parse_krc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub(crate) fn decrypt_qrc(bytes: &[u8]) -> Result<String, String> {
    let mut data = bytes.to_vec();
    qmc1_decrypt(&mut data);
    let encrypted = data
        .get(QRC_MAGIC_HEADER.len()..)
        .ok_or_else(|| "invalid qrc data".to_string())?;
    if encrypted.len() % 8 != 0 {
        return Err("invalid qrc block length".into());
    }

    let cipher = TdesEde3::new_from_slice(QRC_KEY).map_err(|err| err.to_string())?;
    let mut decrypted = Vec::with_capacity(encrypted.len());
    let (blocks, _remainder) = encrypted.as_chunks::<8>();
    for chunk in blocks {
        let mut block = *GenericArray::from_slice(chunk);
        cipher.decrypt_block(&mut block);
        decrypted.extend_from_slice(&block);
    }

    inflate_zlib_utf8(&decrypted)
}

pub(crate) fn decrypt_krc(bytes: &[u8]) -> Result<String, String> {
    let encrypted = bytes
        .get(4..)
        .ok_or_else(|| "invalid krc data".to_string())?;
    let decrypted = encrypted
        .iter()
        .enumerate()
        .map(|(index, value)| value ^ KRC_KEY[index % KRC_KEY.len()])
        .collect::<Vec<_>>();

    inflate_zlib_utf8(&decrypted)
}

pub(crate) fn qmc1_decrypt(data: &mut [u8]) {
    for (index, value) in data.iter_mut().enumerate() {
        let key_index = if index > 0x7fff {
            (index % 0x7fff) & 0x7f
        } else {
            index & 0x7f
        };
        *value ^= QMC1_PRIVKEY[key_index];
    }
}

pub(crate) fn inflate_zlib_utf8(bytes: &[u8]) -> Result<String, String> {
    // 防御 zlib bomb：解压超过 8MB 即视为异常输入。
    // 正常歌词解压后通常 < 100 KB；保留一个安全余量。
    const MAX_INFLATED_BYTES: u64 = 8 * 1024 * 1024;
    let decoder = ZlibDecoder::new(bytes);
    let mut limited = decoder.take(MAX_INFLATED_BYTES);
    let mut text = String::new();
    limited
        .read_to_string(&mut text)
        .map_err(|err| err.to_string())?;
    // 命中上限：极有可能是 zlib bomb，拒绝继续。
    if text.len() as u64 >= MAX_INFLATED_BYTES {
        return Err(format!(
            "lyrics inflated payload exceeds {MAX_INFLATED_BYTES} bytes; rejected"
        ));
    }
    Ok(text)
}

pub(crate) fn parse_provider_lyrics_text(text: &str) -> Vec<LyricLine> {
    let qrc_lyrics = parse_qrc_text(text);
    if !qrc_lyrics.is_empty() {
        return qrc_lyrics;
    }

    if text.contains("<") {
        let krc_lyrics = parse_krc_text(text);
        if !krc_lyrics.is_empty() {
            return krc_lyrics;
        }
    }

    if contains_tuple_marker(text, '(', ')', 3) {
        let yrc_lyrics = parse_yrc_text(text);
        if !yrc_lyrics.is_empty() {
            return yrc_lyrics;
        }
    }

    if contains_tuple_marker(text, '(', ')', 2) {
        let qrc_content_lyrics = provider_lines_to_lyrics(parse_qrc_content(text));
        if !qrc_content_lyrics.is_empty() {
            return qrc_content_lyrics;
        }
    }

    Vec::new()
}

pub(crate) fn parse_qrc_text(text: &str) -> Vec<LyricLine> {
    let Some(content) = extract_qrc_lyric_content(text) else {
        return Vec::new();
    };
    provider_lines_to_lyrics(parse_qrc_content(&decode_xml_entities(&content)))
}

/// 三方（QRC / KRC / YRC）歌词行：`base` 是行级起点与清洗后的整行文本，
/// `end_ms` 来自行头 `[start,dur]`，`words` 是逐字音节（已换算为秒）。
/// `ProviderLyricLine` 定义在 types.rs，这里以包装方式扩展而不改动它。
#[derive(Debug, Clone)]
pub(crate) struct ProviderWordLine {
    pub(crate) base: ProviderLyricLine,
    pub(crate) end_ms: Option<u64>,
    pub(crate) words: Vec<LyricWord>,
}

/// 行体拆分结果：拼接后的整行原文（尚未 `clean_lyric_text`）+ 逐字音节。
pub(crate) struct ProviderBody {
    pub(crate) text: String,
    pub(crate) words: Vec<LyricWord>,
}

pub(crate) fn parse_qrc_content(text: &str) -> Vec<ProviderWordLine> {
    parse_timed_provider_lines(text, |body, _| qrc_body(body))
}

pub(crate) fn parse_krc_text(text: &str) -> Vec<LyricLine> {
    let mut language_tag = None;
    let mut original = Vec::new();

    for raw_line in normalized_lyric_lines(text) {
        let line = raw_line.trim();
        if line.is_empty() || !line.starts_with('[') {
            continue;
        }

        if let Some((key, value)) = split_metadata_tag(line) {
            if key.eq_ignore_ascii_case("language") {
                language_tag = Some(value.to_string());
            }
            continue;
        }

        let Some((start_ms, duration_ms, body)) = split_provider_timed_line(line) else {
            continue;
        };
        // 审2-S6：清洗失败（如纯间奏/空白行）的行以空文本占位保留在 original
        // 中，保证译文按行号对齐时索引空间不塌缩；占位行在输出前被过滤。
        // KRC 音节标签 `<offset,dur,0>` 的 offset 相对行 start，这里换算成绝对毫秒。
        let parsed = tagged_body(body, '<', '>', 3, start_ms);
        let text = clean_lyric_text(&parsed.text).unwrap_or_default();
        original.push(ProviderWordLine {
            base: ProviderLyricLine { start_ms, text },
            end_ms: Some(start_ms.saturating_add(duration_ms)),
            words: parsed.words,
        });
    }

    // 审2-S6：主歌词输出前过滤空占位行；译文对齐（下方）用完整 original。
    let mut lyrics = provider_lines_to_lyrics(
        original
            .iter()
            .filter(|line| !line.base.text.is_empty())
            .cloned()
            .collect(),
    );
    if let Some(language_tag) = language_tag {
        // 译文行只需行级时间，不带 words
        let bases = original
            .iter()
            .map(|line| line.base.clone())
            .collect::<Vec<_>>();
        lyrics.extend(parse_krc_translation_lines(&language_tag, &bases));
        lyrics.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    }

    lyrics
}

pub(crate) fn parse_yrc_text(text: &str) -> Vec<LyricLine> {
    // YRC 音节标签 `(start,dur,0)` 是绝对毫秒，行 start 只用于行级时间
    provider_lines_to_lyrics(parse_timed_provider_lines(text, |body, _| {
        tagged_body(body, '(', ')', 3, 0)
    }))
}

/// 逐行解析 `[start,dur]行体`，`body_to_parts(body, start_ms)` 负责拆出整行文本与音节。
/// 整行清洗为空的行丢弃；有音节但整行为空不会发生（音节文本非空即整行非空）。
pub(crate) fn parse_timed_provider_lines(
    text: &str,
    body_to_parts: impl Fn(&str, u64) -> ProviderBody,
) -> Vec<ProviderWordLine> {
    normalized_lyric_lines(text)
        .filter_map(|raw_line| {
            let line = raw_line.trim();
            let (start_ms, duration_ms, body) = split_provider_timed_line(line)?;
            let parsed = body_to_parts(body, start_ms);
            clean_lyric_text(&parsed.text).map(|text| ProviderWordLine {
                base: ProviderLyricLine { start_ms, text },
                end_ms: Some(start_ms.saturating_add(duration_ms)),
                words: parsed.words,
            })
        })
        .collect()
}

pub(crate) fn provider_lines_to_lyrics(mut lines: Vec<ProviderWordLine>) -> Vec<LyricLine> {
    lines.sort_by_key(|line| line.base.start_ms);
    let mut lyrics = lines
        .into_iter()
        .map(|line| {
            let mut lyric = LyricLine::new(line.base.start_ms as f64 / 1000.0, line.base.text);
            // dur 为 0 的行头（部分源用 0 占位）不给 end，交给前端按下一行推算
            lyric.end = line
                .end_ms
                .filter(|end| *end > line.base.start_ms)
                .map(|end| end as f64 / 1000.0);
            if !line.words.is_empty() {
                lyric.words = Some(line.words);
            }
            lyric
        })
        .collect::<Vec<_>>();
    lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    lyrics
}

/// 把 `(起始毫秒, 时长毫秒, 原文片段)` 序列收成音节：纯空白片段并入前一音节
/// （词间空格属前一音节，与 TTML / 增强 LRC 口径一致；开头的空白直接丢弃），
/// 文本经 `strip_inline_time_tags` + 全角空格归一，音节全部无效时返回空。
fn collect_provider_words(segments: Vec<(u64, u64, &str)>) -> Vec<LyricWord> {
    let mut words: Vec<LyricWord> = Vec::new();
    for (start_ms, duration_ms, raw) in segments {
        let text = strip_inline_time_tags(raw).replace(['\u{3000}', '\t'], " ");
        if text.trim().is_empty() {
            if let Some(last) = words.last_mut() {
                if !text.is_empty() && !last.text.ends_with(' ') {
                    last.text.push(' ');
                }
                // 空白音节的时间并入前一音节，保证时间轴连续
                last.end = last
                    .end
                    .max(start_ms.saturating_add(duration_ms) as f64 / 1000.0);
            }
            continue;
        }
        words.push(LyricWord {
            start: start_ms as f64 / 1000.0,
            end: start_ms.saturating_add(duration_ms) as f64 / 1000.0,
            text,
        });
    }
    words
}

pub(crate) fn normalized_lyric_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .flat_map(|line| line.split('\r'))
        .map(|line| line.trim_start_matches('\u{feff}'))
}

pub(crate) fn split_provider_timed_line(line: &str) -> Option<(u64, u64, &str)> {
    let stripped = line.strip_prefix('[')?;
    let end = stripped.find(']')?;
    let (start, duration) = stripped[..end].split_once(',')?;
    if !start.chars().all(|ch| ch.is_ascii_digit())
        || !duration.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }

    Some((
        start.parse().ok()?,
        duration.parse().ok()?,
        &stripped[end + 1..],
    ))
}

pub(crate) fn split_metadata_tag(line: &str) -> Option<(&str, &str)> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if key.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '_') {
        Some((key.trim(), value.trim()))
    } else {
        None
    }
}

pub(crate) fn extract_qrc_lyric_content(text: &str) -> Option<String> {
    // P3-3：正则只编译一次，批量解析歌词候选时避免每次重新编译。
    static QRC_CONTENT_PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let pattern = QRC_CONTENT_PATTERN.get_or_init(|| {
        Regex::new(r#"(?s)<Lyric_1\s+[^>]*LyricContent="(?P<content>.*?)"[^>]*/?>"#)
            .expect("valid qrc content regex")
    });
    pattern
        .captures(text)
        .and_then(|captures| captures.name("content"))
        .map(|content| content.as_str().to_string())
}

/// QRC 行体：`文本(start,dur)文本(start,dur)…`，标签在文本**后**，start 为绝对毫秒。
/// 没有任何 2 元组标签时整行按行级输出（words 为空）。
pub(crate) fn qrc_body(body: &str) -> ProviderBody {
    let mut segments: Vec<(u64, u64, &str)> = Vec::new();
    let mut output = String::new();
    let mut cursor = 0;
    let mut matched = false;

    while let Some(relative_open) = body[cursor..].find('(') {
        let open = cursor + relative_open;
        let Some(relative_close) = body[open + 1..].find(')') else {
            break;
        };
        let close = open + 1 + relative_close;
        let token = &body[open + 1..close];
        if let Some(numbers) = parse_numeric_tuple(token, 2) {
            let content = strip_provider_prefix_timestamp(&body[cursor..open]);
            output.push_str(content);
            segments.push((numbers[0], numbers[1], content));
            matched = true;
            cursor = close + 1;
        } else {
            cursor = open + 1;
        }
    }

    if matched {
        ProviderBody {
            text: output,
            words: collect_provider_words(segments),
        }
    } else {
        ProviderBody {
            text: body.to_string(),
            words: Vec::new(),
        }
    }
}

/// KRC / YRC 行体：`<offset,dur,0>文本` 或 `(start,dur,0)文本`，标签在文本**前**，
/// 标签到下一个标签之间是一个音节；`base_ms` 加到首元素上（KRC 传行 start，YRC 传 0）。
/// 没有标签时整行按行级输出（words 为空）。
pub(crate) fn tagged_body(
    body: &str,
    open: char,
    close: char,
    tuple_len: usize,
    base_ms: u64,
) -> ProviderBody {
    let markers = find_tuple_markers(body, open, close, tuple_len);
    if markers.is_empty() {
        return ProviderBody {
            text: body.to_string(),
            words: Vec::new(),
        };
    }

    let mut output = String::new();
    let mut segments: Vec<(u64, u64, &str)> = Vec::new();
    for (index, (marker_start, marker_end)) in markers.iter().enumerate() {
        let content_start = *marker_end;
        let content_end = markers
            .get(index + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(body.len());
        let content = &body[content_start..content_end];
        output.push_str(content);
        let token = &body[marker_start + open.len_utf8()..marker_end - close.len_utf8()];
        if let Some(numbers) = parse_numeric_tuple(token, tuple_len) {
            segments.push((base_ms.saturating_add(numbers[0]), numbers[1], content));
        }
    }

    ProviderBody {
        text: output,
        words: collect_provider_words(segments),
    }
}

/// `is_numeric_tuple` 的取值版：形如 `a,b[,c]` 的纯数字元组解析成数字；
/// 任一段溢出 u64 视为无效（不让畸形输入 panic 或绕过）。
pub(crate) fn parse_numeric_tuple(token: &str, expected_len: usize) -> Option<Vec<u64>> {
    if !is_numeric_tuple(token, expected_len) {
        return None;
    }
    token
        .split(',')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

pub(crate) fn find_tuple_markers(
    value: &str,
    open: char,
    close: char,
    tuple_len: usize,
) -> Vec<(usize, usize)> {
    let mut markers = Vec::new();
    let mut cursor = 0;
    let open_len = open.len_utf8();
    let close_len = close.len_utf8();

    while let Some(relative_open) = value[cursor..].find(open) {
        let start = cursor + relative_open;
        let token_start = start + open_len;
        let Some(relative_close) = value[token_start..].find(close) else {
            break;
        };
        let end = token_start + relative_close;
        if is_numeric_tuple(&value[token_start..end], tuple_len) {
            markers.push((start, end + close_len));
            cursor = end + close_len;
        } else {
            cursor = token_start;
        }
    }

    markers
}

pub(crate) fn is_numeric_tuple(token: &str, expected_len: usize) -> bool {
    let parts = token.split(',').collect::<Vec<_>>();
    parts.len() == expected_len
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

pub(crate) fn contains_tuple_marker(
    value: &str,
    open: char,
    close: char,
    tuple_len: usize,
) -> bool {
    !find_tuple_markers(value, open, close, tuple_len).is_empty()
}

pub(crate) fn strip_provider_prefix_timestamp(value: &str) -> &str {
    let trimmed = value.trim_start();
    let Some(stripped) = trimmed.strip_prefix('[') else {
        return value;
    };
    let Some(end) = stripped.find(']') else {
        return value;
    };
    if is_numeric_tuple(&stripped[..end], 2) {
        stripped[end + 1..].trim_start()
    } else {
        value
    }
}

pub(crate) fn parse_krc_translation_lines(
    language_tag: &str,
    original: &[ProviderLyricLine],
) -> Vec<LyricLine> {
    let Ok(decoded) = BASE64_STANDARD.decode(language_tag.trim()) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_slice::<Value>(&decoded) else {
        return Vec::new();
    };

    json.get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|language| language.get("type").and_then(Value::as_i64) == Some(1))
        .flat_map(|language| {
            language
                .get("lyricContent")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, line)| {
                    let original_line = original.get(index)?;
                    let text = line
                        .as_array()?
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" ");
                    clean_lyric_text(&text)
                        .map(|text| LyricLine::new(original_line.start_ms as f64 / 1000.0, text))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(crate) fn decode_xml_entities(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        let after_amp = &rest[start + 1..];
        let Some(end) = after_amp.find(';') else {
            output.push_str(&rest[start..]);
            return output;
        };
        let entity = &after_amp[..end];
        if let Some(decoded) = decode_xml_entity(entity) {
            output.push(decoded);
        } else {
            output.push('&');
            output.push_str(entity);
            output.push(';');
        }
        rest = &after_amp[end + 1..];
    }

    output.push_str(rest);
    output
}

pub(crate) fn decode_xml_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => {
            u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32)
        }
        _ if entity.starts_with('#') => entity[1..].parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

pub(crate) fn decode_lyric_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_le(bytes) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_be(bytes) {
        let units = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_be_bytes(*pair))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

pub(crate) fn looks_like_utf16_le(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 1)
}

pub(crate) fn looks_like_utf16_be(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 0)
}

pub(crate) fn looks_like_utf16(bytes: &[u8], zero_offset: usize) -> bool {
    if bytes.len() < 8 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    let pairs = bytes.len() / 2;
    let zero_count = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| pair[zero_offset] == 0)
        .count();

    zero_count * 100 / pairs >= 60
}

pub(crate) fn lyrics_from_tags(tags: &[Tag]) -> Vec<LyricLine> {
    for tag in tags {
        for key in [ItemKey::Lyrics, ItemKey::UnsyncLyrics] {
            for value in tag.get_strings(key) {
                let lyrics = parse_lyrics_text(value);
                if !lyrics.is_empty() {
                    return lyrics;
                }
            }
        }
    }

    Vec::new()
}

pub(crate) fn parse_lyrics_text(text: &str) -> Vec<LyricLine> {
    let normalized = text
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n");
    let mut offset_ms = 0_i64;
    let mut timed = Vec::new();
    let mut unsynced = Vec::new();

    for raw_line in normalized.lines() {
        let line = raw_line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }

        if let Some(offset) = parse_lrc_offset(line) {
            offset_ms = offset;
            continue;
        }

        let (times, body) = split_lrc_time_tags(line);
        if !times.is_empty() {
            if let Some(text) = clean_lyric_text(body) {
                // 增强型 LRC（LDDC / 网易云导出）：行内 `<mm:ss.xxx>` 是逐字时间，
                // 行尾孤立标签是行结束时间；与行时间同受 offset 校正。
                let enhanced = parse_enhanced_lrc_words(body, offset_ms);
                for time in times {
                    // L-9：LRC 通行约定——正 offset 让歌词提前显示（time - offset）。
                    let shifted = ((time * 1000.0).round() as i64 - offset_ms).max(0);
                    let mut lyric = LyricLine::new(shifted as f64 / 1000.0, text.clone());
                    if let Some((words, end)) = enhanced.clone() {
                        lyric.words = Some(words);
                        lyric.end = end;
                    }
                    timed.push(lyric);
                }
            }
            continue;
        }

        if !is_lrc_metadata_line(line) {
            if let Some(text) = clean_lyric_text(line) {
                unsynced.push(text);
            }
        }
    }

    if !timed.is_empty() {
        timed.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        timed.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
        return timed;
    }

    unsynced
        .into_iter()
        .enumerate()
        .map(|(index, text)| LyricLine::new(index as f64 * 4.0, text))
        .collect()
}

pub(crate) fn split_lrc_time_tags(line: &str) -> (Vec<f64>, &str) {
    let mut rest = line.trim_start();
    let mut times = Vec::new();

    while let Some(stripped) = rest.strip_prefix('[') {
        let Some(end) = stripped.find(']') else {
            break;
        };
        let token = &stripped[..end];
        let Some(time) = parse_lrc_time_token(token) else {
            break;
        };

        times.push(time);
        rest = stripped[end + 1..].trim_start();
    }

    (times, rest)
}

/// 增强型 LRC 行体 → 逐字音节 + 行结束时间。
///
/// 形如 `<00:18.459>我<00:18.814>的<00:19.284>天<00:23.097>`：每个 `<t>` 到下一个 `<t>` 之间
/// 的文本是一个音节，起止即两标签时间；末尾没有文本的标签是行结束。少于一个有效音节
/// 或标签不是时间（如 `<br>`）时返回 None，行按普通 LRC 处理。
pub(crate) fn parse_enhanced_lrc_words(
    body: &str,
    offset_ms: i64,
) -> Option<(Vec<LyricWord>, Option<f64>)> {
    let shift =
        |seconds: f64| ((seconds * 1000.0).round() as i64 - offset_ms).max(0) as f64 / 1000.0;

    // 收集 (时间, 标签结束偏移, 标签起始偏移)
    let mut tags: Vec<(f64, usize, usize)> = Vec::new();
    let mut cursor = 0;
    while let Some(relative_open) = body[cursor..].find('<') {
        let open = cursor + relative_open;
        let Some(relative_close) = body[open + 1..].find('>') else {
            break;
        };
        let close = open + 1 + relative_close;
        match parse_lrc_time_token(&body[open + 1..close]) {
            Some(time) => {
                tags.push((time, close + 1, open));
                cursor = close + 1;
            }
            None => cursor = open + 1,
        }
    }
    if tags.is_empty() {
        return None;
    }

    let mut words = Vec::new();
    for (index, (time, text_start, _)) in tags.iter().enumerate() {
        let text_end = tags
            .get(index + 1)
            .map(|(_, _, open)| *open)
            .unwrap_or(body.len());
        let raw = &body[*text_start..text_end];
        // 音节内保留原始空白（词间空格属于前一个音节），只丢掉纯空白音节
        if raw.trim().is_empty() {
            continue;
        }
        let end = tags.get(index + 1).map(|(next, ..)| *next).unwrap_or(*time);
        words.push(LyricWord {
            start: shift(*time),
            end: shift(end.max(*time)),
            text: strip_inline_time_tags(raw).replace(['\u{3000}', '\t'], " "),
        });
    }
    if words.is_empty() {
        return None;
    }

    // 行尾孤立标签 = 行结束；否则用最后一个音节的结束
    let last_tag = tags.last().map(|(time, ..)| *time)?;
    let trailing_text_empty = body[tags.last().map(|(_, s, _)| *s).unwrap_or(body.len())..]
        .trim()
        .is_empty();
    let end = if trailing_text_empty {
        Some(shift(last_tag))
    } else {
        words.last().map(|word| word.end)
    };
    Some((words, end))
}

pub(crate) fn parse_lrc_offset(line: &str) -> Option<i64> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if !key.trim().eq_ignore_ascii_case("offset") {
        return None;
    }

    value.trim().parse::<i64>().ok()
}

pub(crate) fn parse_lrc_time_token(token: &str) -> Option<f64> {
    let token = token.trim();
    if token.is_empty() {
        return None;
    }

    if !token.contains(':') {
        return parse_millisecond_lrc_token(token);
    }

    let normalized = token.replace(',', ".");
    let parts = normalized.split(':').collect::<Vec<_>>();
    let (hours, minutes, seconds) = match parts.as_slice() {
        [minutes, seconds] => (0, minutes.parse::<u64>().ok()?, (*seconds).to_string()),
        [first, second, third] => {
            // M-11：`[mm:ss:cc]`（千千静听时代的冒号百分秒变体）不能按
            // hh:mm:ss 解析——`[00:29:26]` 是 29.26s 而不是 1766s，否则歌词
            // 导入成功但全程不滚动。第三段为 1-2 位纯数字时判定为百分秒；
            // 含小数点或超两位才按真 hh:mm:ss 处理。
            let is_centiseconds =
                (1..=2).contains(&third.len()) && third.chars().all(|ch| ch.is_ascii_digit());
            if is_centiseconds {
                (0, first.parse::<u64>().ok()?, format!("{second}.{third}"))
            } else {
                (
                    first.parse::<u64>().ok()?,
                    second.parse::<u64>().ok()?,
                    (*third).to_string(),
                )
            }
        }
        _ => return None,
    };

    let seconds = seconds.parse::<f64>().ok()?;
    if seconds.is_nan() || seconds.is_sign_negative() {
        return None;
    }

    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

pub(crate) fn parse_millisecond_lrc_token(token: &str) -> Option<f64> {
    let (start_ms, _) = token.split_once(',')?;
    if start_ms.is_empty() || !start_ms.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(start_ms.parse::<u64>().ok()? as f64 / 1000.0)
}

pub(crate) fn is_lrc_metadata_line(line: &str) -> bool {
    let Some(content) = lrc_tag_content(line) else {
        return false;
    };
    let Some((key, _)) = content.split_once(':') else {
        return false;
    };

    matches!(
        key.trim().to_ascii_lowercase().as_str(),
        "al" | "ar" | "au" | "by" | "length" | "offset" | "re" | "ti" | "ve"
    )
}

pub(crate) fn clean_lyric_text(value: &str) -> Option<String> {
    let text = strip_inline_time_tags(value)
        .replace(['\u{3000}', '\t'], " ")
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ")
        .trim_matches('\0')
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    (!text.is_empty()).then_some(text)
}

pub(crate) fn lrc_tag_content(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
}

pub(crate) fn strip_inline_time_tags(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut rest = value;

    while let Some((start, open, close)) = find_next_time_tag_open(rest) {
        output.push_str(&rest[..start]);
        let after_open = start + open.len_utf8();

        let Some(close_at) = rest[after_open..].find(close) else {
            output.push(open);
            rest = &rest[after_open..];
            continue;
        };

        let token = &rest[after_open..after_open + close_at];
        let after_close = after_open + close_at + close.len_utf8();
        if parse_lrc_time_token(token).is_some() {
            rest = &rest[after_close..];
            continue;
        }

        output.push(open);
        rest = &rest[after_open..];
    }

    output.push_str(rest);
    output
}

pub(crate) fn find_next_time_tag_open(value: &str) -> Option<(usize, char, char)> {
    match (value.find('['), value.find('<')) {
        (Some(square), Some(angle)) if square <= angle => Some((square, '[', ']')),
        (Some(_), Some(angle)) => Some((angle, '<', '>')),
        (Some(square), None) => Some((square, '[', ']')),
        (None, Some(angle)) => Some((angle, '<', '>')),
        (None, None) => None,
    }
}

#[cfg(test)]
mod enhanced_lrc_tests {
    use super::*;

    #[test]
    fn parses_lddc_enhanced_lrc_into_words_with_offset() {
        let text = "[offset:-196]\n[00:18.459]<00:18.459>我<00:18.814>的 <00:19.284>天<00:23.097>\n[00:24.116]<00:24.116>透<00:24.622>明\n[00:26.712]普通行\n";
        let lines = parse_lyrics_text(text);
        assert_eq!(lines.len(), 3);

        let first = &lines[0];
        assert_eq!(first.text, "我的 天");
        // 负 offset → 全部时间后移 196ms
        assert!((first.time - 18.655).abs() < 1e-6);
        assert_eq!(first.end.map(|v| (v * 1000.0).round() as i64), Some(23_293));
        let words = first.words.as_ref().expect("words");
        assert_eq!(
            words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>(),
            ["我", "的 ", "天"]
        );
        assert!((words[0].start - 18.655).abs() < 1e-6 && (words[0].end - 19.010).abs() < 1e-6);
        assert!((words[2].end - 23.293).abs() < 1e-6);

        // 末尾没有孤立标签：行结束 = 最后音节结束（零时长）
        let second = &lines[1];
        assert_eq!(second.words.as_ref().unwrap().len(), 2);
        assert_eq!(
            second.end,
            second.words.as_ref().unwrap().last().map(|w| w.end)
        );

        // 普通行没有 words
        assert!(lines[2].words.is_none());
        assert!(lines[2].end.is_none());
    }

    #[test]
    fn plain_lrc_and_html_like_tags_are_untouched() {
        assert!(parse_enhanced_lrc_words("hello <br> world", 0).is_none());
        assert!(parse_enhanced_lrc_words("no tags", 0).is_none());
        let lines = parse_lyrics_text("[00:01.00]a<br>b\n");
        assert_eq!(lines[0].text, "a b");
        assert!(lines[0].words.is_none());
    }

    #[test]
    fn bilingual_enhanced_lrc_keeps_same_timestamp_pairs() {
        let text = "[00:49.161]<00:49.161>ど<00:49.501>う<00:53.184>\n[00:49.161]<00:49.161>反正也不愿<00:55.550>\n";
        let lines = parse_lyrics_text(text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].time, lines[1].time);
        assert_eq!(lines[1].words.as_ref().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod lyrics_limit_tests {
    use super::*;

    #[test]
    fn clamps_line_count_and_line_length() {
        // W-01：恶意 LRC —— 单行近 2 MB + 几万行
        let long_line = "字".repeat(600_000);
        let mut text = String::new();
        for index in 0..6_000 {
            text.push_str(&format!("[00:{:02}.00]line {index}\n", index % 60));
        }
        text.push_str(&format!("[01:00.00]{long_line}\n"));

        let lyrics = parse_lyrics_bytes(text.as_bytes());
        assert!(
            lyrics.len() <= MAX_LYRIC_LINES,
            "line count must be capped, got {}",
            lyrics.len()
        );
        for line in &lyrics {
            assert!(
                line.text.chars().count() <= MAX_LYRIC_LINE_CHARS,
                "single line must be capped, got {} chars",
                line.text.chars().count()
            );
        }
    }

    #[test]
    fn clamp_truncates_on_char_boundary() {
        // 中文歌词按字节截断会切出半个字符 → 必须按 char 截
        let lyrics = clamp_lyrics(vec![LyricLine::new(
            0.0,
            "中".repeat(MAX_LYRIC_LINE_CHARS + 10),
        )]);
        assert_eq!(lyrics[0].text.chars().count(), MAX_LYRIC_LINE_CHARS);
        assert!(lyrics[0].text.chars().all(|ch| ch == '中'));
    }

    #[test]
    fn ordinary_lyrics_are_untouched() {
        let text = "[00:01.00]第一行\n[00:05.00]第二行\n";
        let lyrics = parse_lyrics_bytes(text.as_bytes());
        assert_eq!(lyrics.len(), 2);
        assert_eq!(lyrics[0].text, "第一行");
        assert_eq!(lyrics[1].text, "第二行");
    }
}
