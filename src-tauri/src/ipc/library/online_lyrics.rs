use super::prelude::*;

pub(crate) fn online_lyrics_query(title: &str, artist: &str) -> String {
    [title.trim(), artist.trim()]
        .into_iter()
        .filter(|value| !value.is_empty() && *value != "Unknown")
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn online_lyrics_client() -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
             (KHTML, like Gecko) Chrome/124.0 Safari/537.36",
        ),
    );

    Client::builder()
        .default_headers(headers)
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|err| format!("failed to create lyrics client: {err}"))
}

pub(crate) async fn fetch_online_lyrics_from_sources(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let mut candidates = Vec::new();
    candidates.extend(fetch_netease_lyrics(client, query, duration).await);
    candidates.extend(fetch_kugou_lyrics(client, query, duration).await);
    candidates.extend(fetch_qq_lyrics(client, query, duration).await);
    dedupe_online_lyrics_candidates(candidates)
}

pub(crate) async fn fetch_netease_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let response = client
        .get("https://music.163.com/api/search/get/web")
        .query(&[
            ("s", query),
            ("type", "1"),
            ("offset", "0"),
            ("limit", "5"),
            ("csrf_token", ""),
        ])
        .send()
        .await
        .ok()
        .and_then(|response| response.error_for_status().ok());

    let Some(response) = response else {
        return Vec::new();
    };

    let Ok(response) = response.json::<Value>().await else {
        return Vec::new();
    };

    let Some(songs) = response
        .get("result")
        .and_then(|value| value.get("songs"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for song in ranked_provider_items(songs, duration).into_iter().take(5) {
        let Some(song_id) = song.get("id").and_then(Value::as_u64) else {
            continue;
        };
        let Ok(lyric_data) = client
            .get("https://music.163.com/api/song/lyric")
            .query(&[
                ("id", song_id.to_string()),
                ("lv", "-1".to_string()),
                ("kv", "-1".to_string()),
                ("tv", "-1".to_string()),
                ("yv", "-1".to_string()),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };
        let Some(lyrics) = parse_netease_lyric_payload(&lyric_data) else {
            continue;
        };

        results.push(OnlineLyricsCandidate {
            id: format!("netease-{song_id}"),
            source: "网易云音乐".into(),
            title: value_string(song, "name").unwrap_or_else(|| query.into()),
            artist: netease_artists(song),
            album: song
                .get("album")
                .and_then(|album| value_string(album, "name")),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

pub(crate) async fn fetch_kugou_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let duration_ms = duration.saturating_mul(1000).to_string();
    let Ok(response) = client
        .get("https://lyrics.kugou.com/search")
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "pc"),
            ("keyword", query),
            ("duration", duration_ms.as_str()),
            ("hash", ""),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    else {
        return Vec::new();
    };

    let Ok(response) = response.json::<Value>().await else {
        return Vec::new();
    };

    let Some(candidates) = response.get("candidates").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for candidate in ranked_provider_items(candidates, duration)
        .into_iter()
        .take(5)
    {
        let Some(id) = candidate.get("id").and_then(Value::as_u64) else {
            continue;
        };
        let Some(access_key) = candidate.get("accesskey").and_then(Value::as_str) else {
            continue;
        };
        let id = id.to_string();
        let Ok(lyric_data) = client
            .get("https://lyrics.kugou.com/download")
            .query(&[
                ("ver", "1"),
                ("client", "pc"),
                ("id", id.as_str()),
                ("accesskey", access_key),
                ("fmt", "krc"),
                ("charset", "utf8"),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };

        let Some(content) = lyric_data.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(decoded) = BASE64_STANDARD.decode(content) else {
            continue;
        };
        let lyrics = parse_lyrics_bytes(&decoded);
        if lyrics.is_empty() {
            continue;
        }

        let title = value_string(candidate, "song")
            .or_else(|| value_string(candidate, "filename"))
            .unwrap_or_else(|| query.into());
        results.push(OnlineLyricsCandidate {
            id: format!("kugou-{id}"),
            source: "酷狗音乐".into(),
            title,
            artist: value_string(candidate, "singer").unwrap_or_default(),
            album: value_string(candidate, "album"),
            duration: provider_duration_ms(candidate).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

pub(crate) async fn fetch_qq_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Vec<OnlineLyricsCandidate> {
    let Ok(search_data) = client
        .get("https://c.y.qq.com/soso/fcgi-bin/client_search_cp")
        .query(&[
            ("format", "json"),
            ("p", "1"),
            ("n", "5"),
            ("w", query),
            ("cr", "1"),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    else {
        return Vec::new();
    };

    let Ok(search_data) = search_data.json::<Value>().await else {
        return Vec::new();
    };

    let Some(songs) = search_data
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for song in ranked_provider_items(songs, duration).into_iter().take(5) {
        let Some(song_mid) = song
            .get("songmid")
            .or_else(|| song.get("mid"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        let Ok(lyric_data) = client
            .get("https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg")
            .header(REFERER, "https://y.qq.com/")
            .query(&[("format", "json"), ("nobase64", "1"), ("songmid", song_mid)])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = lyric_data.json::<Value>().await else {
            continue;
        };
        let Some(lyrics) = parse_qq_lyric_payload(&lyric_data) else {
            continue;
        };

        results.push(OnlineLyricsCandidate {
            id: format!("qq-{song_mid}"),
            source: "QQ音乐".into(),
            title: value_string(song, "songname")
                .or_else(|| value_string(song, "title"))
                .unwrap_or_else(|| query.into()),
            artist: qq_singers(song),
            album: value_string(song, "albumname"),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics,
        });
    }

    results
}

pub(crate) fn parse_netease_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    if let Some(yrc) = payload
        .get("yrc")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        let lyrics = parse_lyrics_bytes(yrc.as_bytes());
        if !lyrics.is_empty() {
            return Some(lyrics);
        }
    }

    let mut lyrics = Vec::new();
    if let Some(lrc) = payload
        .get("lrc")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
    {
        lyrics.extend(parse_lyrics_text(lrc));
    }
    if let Some(tlyric) = payload
        .get("tlyric")
        .and_then(|value| value.get("lyric"))
        .and_then(Value::as_str)
    {
        lyrics.extend(parse_lyrics_text(tlyric));
    }
    normalize_lyric_lines(lyrics)
}

pub(crate) fn parse_qq_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    let mut lyrics = Vec::new();
    if let Some(lyric) = payload.get("lyric").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(lyric));
    }
    if let Some(trans) = payload.get("trans").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(trans));
    }
    normalize_lyric_lines(lyrics)
}

pub(crate) fn parse_online_lyric_text(value: &str) -> Vec<LyricLine> {
    let compact = value.trim();
    if compact.contains('[') && compact.contains(']') {
        let lyrics = parse_lyrics_text(compact);
        if !lyrics.is_empty() {
            return lyrics;
        }
    }

    let Ok(decoded) = BASE64_STANDARD.decode(compact) else {
        return parse_lyrics_text(compact);
    };
    let text = decode_lyric_bytes(&decoded);
    parse_lyrics_text(&text)
}

pub(crate) fn normalize_lyric_lines(mut lyrics: Vec<LyricLine>) -> Option<Vec<LyricLine>> {
    lyrics.retain(|line| !line.text.trim().is_empty());
    lyrics.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lyrics.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub(crate) fn ranked_provider_items(items: &[Value], duration: u64) -> Vec<&Value> {
    let target_ms = duration.saturating_mul(1000);
    let mut ranked = items.iter().collect::<Vec<_>>();
    ranked.sort_by_key(|item| {
        provider_duration_ms(item)
            .map(|item_ms| item_ms.abs_diff(target_ms))
            .unwrap_or(u64::MAX)
    });
    ranked
}

pub(crate) fn dedupe_online_lyrics_candidates(
    candidates: Vec<OnlineLyricsCandidate>,
) -> Vec<OnlineLyricsCandidate> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();

    for candidate in candidates {
        if candidate.lyrics.is_empty() {
            continue;
        }

        // L-5: 用「行数 + 总字符 + 前 3 行 hash + 时长」作为指纹，
        // 同首歌不同来源即使翻译字段不同也能识别为同一份。
        let mut hasher = DefaultHasher::new();
        candidate.lyrics.len().hash(&mut hasher);
        let total_chars: usize = candidate
            .lyrics
            .iter()
            .map(|l| l.text.chars().count())
            .sum();
        total_chars.hash(&mut hasher);
        for line in candidate.lyrics.iter().take(3) {
            normalize_text(&line.text).hash(&mut hasher);
        }
        candidate.duration.unwrap_or_default().hash(&mut hasher);
        let key = hasher.finish();
        if seen.insert(key) {
            deduped.push(candidate);
        }
    }

    deduped
}

pub(crate) fn normalize_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_punctuation())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

pub(crate) fn value_string(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn netease_artists(song: &Value) -> String {
    song.get("artists")
        .and_then(Value::as_array)
        .map(|artists| {
            artists
                .iter()
                .filter_map(|artist| value_string(artist, "name"))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub(crate) fn qq_singers(song: &Value) -> String {
    song.get("singer")
        .and_then(Value::as_array)
        .map(|singers| {
            singers
                .iter()
                .filter_map(|singer| value_string(singer, "name"))
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub(crate) fn provider_duration_ms(item: &Value) -> Option<u64> {
    for key in ["duration", "interval", "dt", "song_duration"] {
        if let Some(value) = item.get(key).and_then(Value::as_u64) {
            return Some(if value < 10_000 { value * 1000 } else { value });
        }
        // 审2-S7：字符串值解析失败时继续尝试下一个候选键；
        // 原先的 `?` 会让单个坏键（如 "N/A"）直接放弃全部剩余候选。
        if let Some(parsed) = item
            .get(key)
            .and_then(Value::as_str)
            .and_then(|value| value.parse::<u64>().ok())
        {
            return Some(if parsed < 10_000 {
                parsed * 1000
            } else {
                parsed
            });
        }
    }
    None
}
