const QRC_MAGIC_HEADER: &[u8] = b"\x98%\xb0\xac\xe3\x02\x83h\xe8\xfcl";
const KRC_MAGIC_HEADER: &[u8] = b"krc18";
const QRC_KEY: &[u8] = b"!@#)(*$%123ZXC!@!@#)(NHL";
const KRC_KEY: &[u8] = b"@Gaw^2tGQ61-\xce\xd2ni";
const QMC1_PRIVKEY: [u8; 128] = [
    0xc3, 0x4a, 0xd6, 0xca, 0x90, 0x67, 0xf7, 0x52, 0xd8, 0xa1, 0x66, 0x62, 0x9f, 0x5b, 0x09, 0x00,
    0xc3, 0x5e, 0x95, 0x23, 0x9f, 0x13, 0x11, 0x7e, 0xd8, 0x92, 0x3f, 0xbc, 0x90, 0xbb, 0x74, 0x0e,
    0xc3, 0x47, 0x74, 0x3d, 0x90, 0xaa, 0x3f, 0x51, 0xd8, 0xf4, 0x11, 0x84, 0x9f, 0xde, 0x95, 0x1d,
    0xc3, 0xc6, 0x09, 0xd5, 0x9f, 0xfa, 0x66, 0xf9, 0xd8, 0xf0, 0xf7, 0xa0, 0x90, 0xa1, 0xd6, 0xf3,
    0xc3, 0xf3, 0xd6, 0xa1, 0x90, 0xa0, 0xf7, 0xf0, 0xd8, 0xf9, 0x66, 0xfa, 0x9f, 0xd5, 0x09, 0xc6,
    0xc3, 0x1d, 0x95, 0xde, 0x9f, 0x84, 0x11, 0xf4, 0xd8, 0x51, 0x3f, 0xaa, 0x90, 0x3d, 0x74, 0x47,
    0xc3, 0x0e, 0x74, 0xbb, 0x90, 0xbc, 0x3f, 0x92, 0xd8, 0x7e, 0x11, 0x13, 0x9f, 0x23, 0x95, 0x5e,
    0xc3, 0x00, 0x09, 0x5b, 0x9f, 0x62, 0x66, 0xa1, 0xd8, 0x52, 0xf7, 0x67, 0x90, 0xca, 0xd6, 0x4a,
];

fn parse_lyrics_bytes(bytes: &[u8]) -> Vec<LyricLine> {
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
    let provider_lyrics = parse_provider_lyrics_text(&text);
    if !provider_lyrics.is_empty() {
        return provider_lyrics;
    }

    parse_lyrics_text(&text)
}

fn parse_encrypted_qrc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_qrc(bytes).ok()?;
    let lyrics = parse_qrc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn parse_encrypted_krc_lyrics(bytes: &[u8]) -> Option<Vec<LyricLine>> {
    let text = decrypt_krc(bytes).ok()?;
    let lyrics = parse_krc_text(&text);
    (!lyrics.is_empty()).then_some(lyrics)
}

fn decrypt_qrc(bytes: &[u8]) -> Result<String, String> {
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
    for chunk in encrypted.chunks_exact(8) {
        let mut block = *GenericArray::from_slice(chunk);
        cipher.decrypt_block(&mut block);
        decrypted.extend_from_slice(&block);
    }

    inflate_zlib_utf8(&decrypted)
}

fn decrypt_krc(bytes: &[u8]) -> Result<String, String> {
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

fn qmc1_decrypt(data: &mut [u8]) {
    for (index, value) in data.iter_mut().enumerate() {
        let key_index = if index > 0x7fff {
            (index % 0x7fff) & 0x7f
        } else {
            index & 0x7f
        };
        *value ^= QMC1_PRIVKEY[key_index];
    }
}

fn inflate_zlib_utf8(bytes: &[u8]) -> Result<String, String> {
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

fn parse_provider_lyrics_text(text: &str) -> Vec<LyricLine> {
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

fn parse_qrc_text(text: &str) -> Vec<LyricLine> {
    let Some(content) = extract_qrc_lyric_content(text) else {
        return Vec::new();
    };
    provider_lines_to_lyrics(parse_qrc_content(&decode_xml_entities(&content)))
}

fn parse_qrc_content(text: &str) -> Vec<ProviderLyricLine> {
    parse_timed_provider_lines(text, qrc_line_text)
}

fn parse_krc_text(text: &str) -> Vec<LyricLine> {
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

        let Some((start_ms, _, body)) = split_provider_timed_line(line) else {
            continue;
        };
        if let Some(text) = clean_lyric_text(&tagged_line_text(body, '<', '>', 3)) {
            original.push(ProviderLyricLine { start_ms, text });
        }
    }

    let mut lyrics = provider_lines_to_lyrics(original.clone());
    if let Some(language_tag) = language_tag {
        lyrics.extend(parse_krc_translation_lines(&language_tag, &original));
        lyrics.sort_by(|a, b| {
            a.time
                .partial_cmp(&b.time)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    }

    lyrics
}

fn parse_yrc_text(text: &str) -> Vec<LyricLine> {
    provider_lines_to_lyrics(parse_timed_provider_lines(text, |body| {
        tagged_line_text(body, '(', ')', 3)
    }))
}

fn parse_timed_provider_lines(
    text: &str,
    body_to_text: impl Fn(&str) -> String,
) -> Vec<ProviderLyricLine> {
    normalized_lyric_lines(text)
        .filter_map(|raw_line| {
            let line = raw_line.trim();
            let (start_ms, _, body) = split_provider_timed_line(line)?;
            clean_lyric_text(&body_to_text(body)).map(|text| ProviderLyricLine { start_ms, text })
        })
        .collect()
}

fn provider_lines_to_lyrics(mut lines: Vec<ProviderLyricLine>) -> Vec<LyricLine> {
    lines.sort_by_key(|line| line.start_ms);
    let mut lyrics = lines
        .into_iter()
        .map(|line| LyricLine {
            time: line.start_ms as f64 / 1000.0,
            text: line.text,
        })
        .collect::<Vec<_>>();
    lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    lyrics
}

fn normalized_lyric_lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .flat_map(|line| line.split('\r'))
        .map(|line| line.trim_start_matches('\u{feff}'))
}

fn split_provider_timed_line(line: &str) -> Option<(u64, u64, &str)> {
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

fn split_metadata_tag(line: &str) -> Option<(&str, &str)> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if key.chars().all(|ch| ch.is_ascii_alphabetic() || ch == '_') {
        Some((key.trim(), value.trim()))
    } else {
        None
    }
}

fn extract_qrc_lyric_content(text: &str) -> Option<String> {
    let pattern =
        Regex::new(r#"(?s)<Lyric_1\s+[^>]*LyricContent="(?P<content>.*?)"[^>]*/?>"#).ok()?;
    pattern
        .captures(text)
        .and_then(|captures| captures.name("content"))
        .map(|content| content.as_str().to_string())
}

fn qrc_line_text(body: &str) -> String {
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
        if is_numeric_tuple(token, 2) {
            let content = strip_provider_prefix_timestamp(&body[cursor..open]);
            output.push_str(content);
            matched = true;
            cursor = close + 1;
        } else {
            cursor = open + 1;
        }
    }

    if matched {
        output
    } else {
        body.to_string()
    }
}

fn tagged_line_text(body: &str, open: char, close: char, tuple_len: usize) -> String {
    let markers = find_tuple_markers(body, open, close, tuple_len);
    if markers.is_empty() {
        return body.to_string();
    }

    let mut output = String::new();
    for (index, (_, marker_end)) in markers.iter().enumerate() {
        let content_start = *marker_end;
        let content_end = markers
            .get(index + 1)
            .map(|(next_start, _)| *next_start)
            .unwrap_or(body.len());
        output.push_str(&body[content_start..content_end]);
    }

    output
}

fn find_tuple_markers(
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

fn is_numeric_tuple(token: &str, expected_len: usize) -> bool {
    let parts = token.split(',').collect::<Vec<_>>();
    parts.len() == expected_len
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn contains_tuple_marker(value: &str, open: char, close: char, tuple_len: usize) -> bool {
    !find_tuple_markers(value, open, close, tuple_len).is_empty()
}

fn strip_provider_prefix_timestamp(value: &str) -> &str {
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

fn parse_krc_translation_lines(
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
                    clean_lyric_text(&text).map(|text| LyricLine {
                        time: original_line.start_ms as f64 / 1000.0,
                        text,
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn decode_xml_entities(value: &str) -> String {
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

fn decode_xml_entity(entity: &str) -> Option<char> {
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

fn decode_lyric_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_le(bytes) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if looks_like_utf16_be(bytes) {
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }

    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }

    let (text, _, _) = GBK.decode(bytes);
    text.into_owned()
}

fn looks_like_utf16_le(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 1)
}

fn looks_like_utf16_be(bytes: &[u8]) -> bool {
    looks_like_utf16(bytes, 0)
}

fn looks_like_utf16(bytes: &[u8], zero_offset: usize) -> bool {
    if bytes.len() < 8 || !bytes.len().is_multiple_of(2) {
        return false;
    }

    let pairs = bytes.len() / 2;
    let zero_count = bytes
        .chunks_exact(2)
        .filter(|pair| pair[zero_offset] == 0)
        .count();

    zero_count * 100 / pairs >= 60
}

fn lyrics_from_tags(tags: &[Tag]) -> Vec<LyricLine> {
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

fn parse_lyrics_text(text: &str) -> Vec<LyricLine> {
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
                for time in times {
                    // L-9：LRC 通行约定——正 offset 让歌词提前显示（time - offset）。
                    let shifted = ((time * 1000.0).round() as i64 - offset_ms).max(0);
                    timed.push(LyricLine {
                        time: shifted as f64 / 1000.0,
                        text: text.clone(),
                    });
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
        .map(|(index, text)| LyricLine {
            time: index as f64 * 4.0,
            text,
        })
        .collect()
}

fn split_lrc_time_tags(line: &str) -> (Vec<f64>, &str) {
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

fn parse_lrc_offset(line: &str) -> Option<i64> {
    let content = lrc_tag_content(line)?;
    let (key, value) = content.split_once(':')?;
    if !key.trim().eq_ignore_ascii_case("offset") {
        return None;
    }

    value.trim().parse::<i64>().ok()
}

fn parse_lrc_time_token(token: &str) -> Option<f64> {
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
        [minutes, seconds] => (0, minutes.parse::<u64>().ok()?, seconds),
        [hours, minutes, seconds] => (
            hours.parse::<u64>().ok()?,
            minutes.parse::<u64>().ok()?,
            seconds,
        ),
        _ => return None,
    };

    let seconds = seconds.parse::<f64>().ok()?;
    if seconds.is_nan() || seconds.is_sign_negative() {
        return None;
    }

    Some(hours as f64 * 3600.0 + minutes as f64 * 60.0 + seconds)
}

fn parse_millisecond_lrc_token(token: &str) -> Option<f64> {
    let (start_ms, _) = token.split_once(',')?;
    if start_ms.is_empty() || !start_ms.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(start_ms.parse::<u64>().ok()? as f64 / 1000.0)
}

fn is_lrc_metadata_line(line: &str) -> bool {
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

fn clean_lyric_text(value: &str) -> Option<String> {
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

fn lrc_tag_content(line: &str) -> Option<&str> {
    line.trim()
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .map(str::trim)
}

fn strip_inline_time_tags(value: &str) -> String {
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

fn find_next_time_tag_open(value: &str) -> Option<(usize, char, char)> {
    match (value.find('['), value.find('<')) {
        (Some(square), Some(angle)) if square <= angle => Some((square, '[', ']')),
        (Some(_), Some(angle)) => Some((angle, '<', '>')),
        (Some(square), None) => Some((square, '[', ']')),
        (None, Some(angle)) => Some((angle, '<', '>')),
        (None, None) => None,
    }
}
