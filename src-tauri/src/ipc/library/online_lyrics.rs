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
        // L-3（2026-08-16 审查）：逐跳复验重定向——歌词源虽是固定 URL，
        // 上游被攻破/中间层 302 指向内网地址时不拦就是盲 SSRF 探针（F-01 同型）
        .redirect(guarded_redirect_policy(
            crate::ipc::url_guard::is_safe_lyrics_url,
        ))
        .build()
        .map_err(|err| format!("failed to create lyrics client: {err}"))
}

/// 三源聚合结果：candidates 为空时前端需区分“网络异常”与“确实没有”（M-12）。
pub(crate) struct OnlineLyricsFetch {
    pub candidates: Vec<OnlineLyricsCandidate>,
    /// 搜索请求即失败（网络/解析错误）的源数量
    pub failed_sources: usize,
}

/// 设置「歌词源优先级」。Auto = 三源并发、按接口顺序聚合；指定源 = 该源候选排最前
/// （仍并发请求其它源作兜底，避免首选源挂掉时用户空手而归）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LyricsSourcePriority {
    Auto,
    Netease,
    Kugou,
    Qq,
}

impl LyricsSourcePriority {
    pub(crate) fn parse(raw: &str) -> Self {
        match raw.trim() {
            "netease" => Self::Netease,
            "kugou" => Self::Kugou,
            "qq" => Self::Qq,
            _ => Self::Auto,
        }
    }
}

/// `title` / `artist` 分开传：网易云与 QQ 用 `online_lyrics_query` 拼成一句，酷狗歌词搜索
/// 对关键词格式敏感，另走 `kugou_lyrics_keyword`（手动搜索时前端把整句放在 title、artist 传空）。
pub(crate) async fn fetch_online_lyrics_from_sources(
    client: &Client,
    title: &str,
    artist: &str,
    duration: u64,
    priority: LyricsSourcePriority,
) -> OnlineLyricsFetch {
    let query = online_lyrics_query(title, artist);
    // 三源并发：此前串行 await，半死接口的超时会逐源叠加（最坏数分钟）
    let (netease, kugou, qq) = tokio::join!(
        fetch_netease_lyrics(client, &query, duration),
        fetch_kugou_lyrics(client, title, artist, duration),
        fetch_qq_lyrics(client, &query, duration),
    );
    let mut ordered = vec![
        (LyricsSourcePriority::Netease, netease),
        (LyricsSourcePriority::Kugou, kugou),
        (LyricsSourcePriority::Qq, qq),
    ];
    if priority != LyricsSourcePriority::Auto {
        // 稳定排序：首选源提前，其余保持原序
        ordered.sort_by_key(|(source, _)| *source != priority);
    }
    let mut candidates = Vec::new();
    let mut failed_sources = 0usize;
    for (_, outcome) in ordered {
        match outcome {
            Ok(list) => candidates.extend(list),
            Err(()) => failed_sources += 1,
        }
    }
    let candidates = rank_online_lyrics_candidates(
        dedupe_online_lyrics_candidates(candidates),
        duration,
        priority,
    );
    OnlineLyricsFetch {
        candidates,
        failed_sources,
    }
}

/// 候选能力分档（越小越好）：逐字 + 译文 < 逐字 < 译文 < 逐行（LDDC `auto_fetch` 的取舍顺序）。
/// 译文两种形态都算：`translations` 字段或相邻同时间戳的两条 **Main** 行（和声 / 制作信息行
/// 不参与这个启发式）。
pub(crate) fn candidate_capability_tier(lyrics: &[LyricLine]) -> u8 {
    let word_synced = lyrics.iter().any(|line| line.words.is_some());
    let has_translation = lyrics.iter().any(|line| !line.translations.is_empty())
        || lyrics.windows(2).any(|pair| {
            pair[0].role == LyricRole::Main
                && pair[1].role == LyricRole::Main
                && pair[0].start_ms.abs_diff(pair[1].start_ms) < 10
                && pair[0].text != pair[1].text
        });
    match (word_synced, has_translation) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

/// 与本地时长相差超过这么多秒的候选（Live / 试听片段 / 另一版本）沉底；LDDC 直接丢弃，
/// 这里保留在列表末尾供手动挑选，但不会被自动选中。
pub(crate) const CANDIDATE_DURATION_TOLERANCE_SECONDS: u64 = 4;

/// 三源候选排序（稳定，源内原序不变）：时长明显不符的沉底；「自动」优先级下按能力分档，
/// 逐字候选排最前；用户指定了首选源时尊重设置——源顺序优先、档次其次。
pub(crate) fn rank_online_lyrics_candidates(
    mut candidates: Vec<OnlineLyricsCandidate>,
    duration: u64,
    priority: LyricsSourcePriority,
) -> Vec<OnlineLyricsCandidate> {
    let mismatch = |candidate: &OnlineLyricsCandidate| -> bool {
        match candidate.duration {
            Some(theirs) if duration > 0 && theirs > 0 => {
                theirs.abs_diff(duration) > CANDIDATE_DURATION_TOLERANCE_SECONDS
            }
            _ => false,
        }
    };
    let source_rank = |candidate: &OnlineLyricsCandidate| -> u8 {
        let source = match candidate.id.split('-').next() {
            Some("netease") => LyricsSourcePriority::Netease,
            Some("kugou") => LyricsSourcePriority::Kugou,
            Some("qq") => LyricsSourcePriority::Qq,
            _ => LyricsSourcePriority::Auto,
        };
        u8::from(priority != LyricsSourcePriority::Auto && source != priority)
    };
    candidates.sort_by_key(|candidate| {
        (
            mismatch(candidate),
            source_rank(candidate),
            candidate_capability_tier(&candidate.lyrics.lines),
        )
    });
    candidates
}

/// Err(()) = 搜索请求本身失败（网络/HTTP/解析错误）；
/// Ok(空) = 接口正常返回但没有匹配（真·未找到）。逐候选拉词失败仍算部分成功。
pub(crate) async fn fetch_netease_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Result<Vec<OnlineLyricsCandidate>, ()> {
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
        return Err(());
    };

    let Ok(response) = read_json_capped::<Value>(response, MAX_EXTERNAL_JSON_BYTES).await else {
        return Err(());
    };

    let Some(songs) = response
        .get("result")
        .and_then(|value| value.get("songs"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };

    let mut results = Vec::new();
    for song in ranked_provider_items(songs, duration).into_iter().take(5) {
        let Some(song_id) = song.get("id").and_then(Value::as_u64) else {
            continue;
        };
        // lv/tv/yv = 逐行 / 译文 / 逐字（yrc）；rv/ytv/yrv = 音译 / 与 yrc 对齐的译文 / 与 yrc
        // 对齐的音译（2026-09-19 实测 Web 端匿名可取）。
        let Ok(lyric_data) = client
            .get("https://music.163.com/api/song/lyric")
            .query(&[
                ("id", song_id.to_string()),
                ("lv", "-1".to_string()),
                ("kv", "-1".to_string()),
                ("tv", "-1".to_string()),
                ("rv", "-1".to_string()),
                ("yv", "-1".to_string()),
                ("ytv", "-1".to_string()),
                ("yrv", "-1".to_string()),
            ])
            .send()
            .await
            .and_then(|response| response.error_for_status())
        else {
            continue;
        };
        let Ok(lyric_data) = read_json_capped::<Value>(lyric_data, MAX_EXTERNAL_JSON_BYTES).await
        else {
            continue;
        };
        let Some(lyrics) = parse_netease_lyric_payload(&lyric_data) else {
            continue;
        };

        let ttml_lookup_keys = vec![format!("ncm-lyrics/{song_id}")];
        results.push(OnlineLyricsCandidate {
            id: format!("netease-{song_id}"),
            source: "网易云音乐".into(),
            title: value_string(song, "name").unwrap_or_else(|| query.into()),
            artist: netease_artists(song),
            album: song
                .get("album")
                .and_then(|album| value_string(album, "name")),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics: LyricDocument::from_lines(
                lyrics,
                LyricSource::online("netease", song_id.to_string())
                    .with_lookup_keys(ttml_lookup_keys.clone()),
            ),
            ttml_lookup_keys,
        });
    }

    Ok(results)
}

/// 酷狗歌词搜索关键词。`lyrics.kugou.com/search` 对格式极其敏感（2026-09-19 实测）：
/// `唯一 王力宏` 返回 0 候选，`王力宏 - 唯一`（LDDC `get_lyricslist` 的拼法）返回 11 个，
/// 中 / 英 / 日三种语言同样规律——酷狗是逐字 KRC 覆盖最广的源，此前几乎从不命中。
/// 多艺术家按 `、` 连接（LDDC 口径）；艺术家为空 / Unknown 时只用曲名。
pub(crate) fn kugou_lyrics_keyword(title: &str, artist: &str) -> String {
    let title = title.trim();
    let artists = artist
        .split(['/', '&', ',', '，', '、', ';'])
        .map(str::trim)
        .filter(|part| {
            !part.is_empty()
                && !matches!(
                    part.to_ascii_lowercase().as_str(),
                    "unknown" | "unknown artist"
                )
        })
        .collect::<Vec<_>>();
    if artists.is_empty() || title.is_empty() {
        return title.to_string();
    }
    format!("{} - {title}", artists.join("、"))
}

/// 酷狗候选排序：接口自带 `score` 降序（LDDC 直接取首个 = 最高分）；同分按与本地时长的
/// 接近度；时长未知（0）时不参与（M-13 同型）。稳定排序，其余保持接口原序。
pub(crate) fn ranked_kugou_candidates(items: &[Value], duration: u64) -> Vec<&Value> {
    let target_ms = duration.saturating_mul(1000);
    let mut ranked = items.iter().collect::<Vec<_>>();
    ranked.sort_by_key(|item| {
        let score = item
            .get("score")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
            })
            .unwrap_or(0);
        let diff = if duration == 0 {
            0
        } else {
            provider_duration_ms(item)
                .map(|item_ms| item_ms.abs_diff(target_ms))
                .unwrap_or(u64::MAX)
        };
        (std::cmp::Reverse(score), diff)
    });
    ranked
}

/// 一次酷狗歌词搜索；Err = 请求 / 解析失败，Ok(空) = 正常返回但没有候选。
async fn kugou_search(client: &Client, keyword: &str, duration: u64) -> Result<Vec<Value>, ()> {
    let duration_ms = duration.saturating_mul(1000).to_string();
    let Ok(response) = client
        .get("https://lyrics.kugou.com/search")
        .query(&[
            ("ver", "1"),
            ("man", "yes"),
            ("client", "pc"),
            ("keyword", keyword),
            ("duration", duration_ms.as_str()),
            ("hash", ""),
        ])
        .send()
        .await
        .and_then(|response| response.error_for_status())
    else {
        return Err(());
    };

    let Ok(response) = read_json_capped::<Value>(response, MAX_EXTERNAL_JSON_BYTES).await else {
        return Err(());
    };

    Ok(response
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub(crate) async fn fetch_kugou_lyrics(
    client: &Client,
    title: &str,
    artist: &str,
    duration: u64,
) -> Result<Vec<OnlineLyricsCandidate>, ()> {
    let keyword = kugou_lyrics_keyword(title, artist);
    if keyword.is_empty() {
        return Ok(Vec::new());
    }
    let mut candidates = kugou_search(client, &keyword, duration).await?;
    // 「艺术家 - 曲名」零命中（艺术家写法与酷狗库不一致）时退回只用曲名；时长照传，
    // 不传时长的纯曲名搜索会撞到同名歌手 / 无关短片段
    let title_only = title.trim();
    if candidates.is_empty() && !title_only.is_empty() && keyword != title_only {
        candidates = kugou_search(client, title_only, duration).await?;
    }

    let mut results = Vec::new();
    for candidate in ranked_kugou_candidates(&candidates, duration)
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
        let Ok(lyric_data) = read_json_capped::<Value>(lyric_data, MAX_EXTERNAL_JSON_BYTES).await
        else {
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
            .unwrap_or_else(|| keyword.clone());
        results.push(OnlineLyricsCandidate {
            id: format!("kugou-{id}"),
            source: "酷狗音乐".into(),
            title,
            artist: value_string(candidate, "singer").unwrap_or_default(),
            album: value_string(candidate, "album"),
            duration: provider_duration_ms(candidate).map(|ms| ms / 1000),
            lyrics: LyricDocument::from_lines(lyrics, LyricSource::online("kugou", id.clone())),
            ttml_lookup_keys: Vec::new(),
        });
    }

    Ok(results)
}

pub(crate) async fn fetch_qq_lyrics(
    client: &Client,
    query: &str,
    duration: u64,
) -> Result<Vec<OnlineLyricsCandidate>, ()> {
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
        return Err(());
    };

    let Ok(search_data) = read_json_capped::<Value>(search_data, MAX_EXTERNAL_JSON_BYTES).await
    else {
        return Err(());
    };

    let Some(songs) = search_data
        .get("data")
        .and_then(|value| value.get("song"))
        .and_then(|value| value.get("list"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
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
        // 数字 songid：客户端取词接口与 AMLL qq-lyrics 目录都按它索引（非 songmid）
        let song_id = song
            .get("songid")
            .or_else(|| song.get("id"))
            .and_then(Value::as_u64);
        // 逐字 QRC + 译文 + 音译走客户端取词接口；请求失败或没内容时退回网页端逐行接口，
        // 保证不比此前差
        let lyrics = match song_id {
            Some(song_id) => fetch_qq_play_lyric(client, song_id).await,
            None => None,
        };
        let lyrics = match lyrics {
            Some(lyrics) => lyrics,
            None => match fetch_qq_web_lyric(client, song_mid).await {
                Some(lyrics) => lyrics,
                None => continue,
            },
        };

        let ttml_lookup_keys = song_id
            .map(|id| vec![format!("qq-lyrics/{id}")])
            .unwrap_or_default();
        results.push(OnlineLyricsCandidate {
            id: format!("qq-{song_mid}"),
            source: "QQ音乐".into(),
            title: value_string(song, "songname")
                .or_else(|| value_string(song, "title"))
                .unwrap_or_else(|| query.into()),
            artist: qq_singers(song),
            album: value_string(song, "albumname"),
            duration: provider_duration_ms(song).map(|ms| ms / 1000),
            lyrics: LyricDocument::from_lines(
                lyrics,
                LyricSource::online(
                    "qq",
                    song_id
                        .map(|id| id.to_string())
                        .unwrap_or_else(|| song_mid.to_string()),
                )
                .with_lookup_keys(ttml_lookup_keys.clone()),
            ),
            ttml_lookup_keys,
        });
    }

    Ok(results)
}

/// QQ 客户端取词接口 `music.musichallSong.PlayLyricInfo`（2026-09-19 实测匿名可用）：
/// 网页端 `fcg_query_lyric_new.fcg` 只给逐行 LRC，逐字 QRC、译文与音译只有这里有。
/// 返回体三段都是 hex 密文（见 `decrypt_qrc_cloud`）。失败 / 无内容返回 None，由调用方退回旧接口。
/// 同一 `online_lyrics_client`（逐跳白名单 `.qq.com` 已覆盖 `u.y.qq.com`），响应经 `read_json_capped`。
async fn fetch_qq_play_lyric(client: &Client, song_id: u64) -> Option<Vec<LyricLine>> {
    let body = serde_json::json!({
        "comm": { "ct": 19, "cv": "2111" },
        "req": {
            "method": "GetPlayLyricInfo",
            "module": "music.musichallSong.PlayLyricInfo",
            "param": {
                "songID": song_id,
                "crypt": 1,
                "qrc": 1,
                "qrc_t": 0,
                "roma": 1,
                "roma_t": 0,
                "trans": 1,
                "trans_t": 0,
                "lrc_t": 0,
                "ct": 19,
                "cv": 2111,
                "type": 0
            }
        }
    });
    let response = client
        .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
        .header(REFERER, "https://y.qq.com/")
        .json(&body)
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .ok()?;
    let payload = read_json_capped::<Value>(response, MAX_EXTERNAL_JSON_BYTES)
        .await
        .ok()?;
    let data = payload.get("req")?.get("data")?;
    parse_qq_play_lyric_payload(data)
}

/// 网页端逐行接口（旧路径，作为客户端接口失败时的回退）。
async fn fetch_qq_web_lyric(client: &Client, song_mid: &str) -> Option<Vec<LyricLine>> {
    let response = client
        .get("https://c.y.qq.com/lyric/fcgi-bin/fcg_query_lyric_new.fcg")
        .header(REFERER, "https://y.qq.com/")
        .query(&[("format", "json"), ("nobase64", "1"), ("songmid", song_mid)])
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .ok()?;
    let payload = read_json_capped::<Value>(response, MAX_EXTERNAL_JSON_BYTES)
        .await
        .ok()?;
    parse_qq_lyric_payload(&payload)
}

/// `GetPlayLyricInfo` 的 `data`：`lyric` / `trans` / `roma` 是 hex 密文（空字符串 = 无该项）。
/// 原文通常是 `<Lyric_1 …>` QRC 容器（逐字），也可能是普通 LRC，交给 `parse_lyrics_bytes` 嗅探；
/// 译文是逐行 LRC，无原文的行写 `//` 占位、头部带 QQ 私有 `[kana:…]` 注音标签，都剔除；
/// 音译是逐字 QRC，按行取音节拼接后写进原文行的 `roman` 字段。
pub(crate) fn parse_qq_play_lyric_payload(data: &Value) -> Option<Vec<LyricLine>> {
    let decrypted = |key: &str| -> Option<String> {
        let hex = data
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        match decrypt_qrc_cloud(hex) {
            Ok(text) => Some(text),
            Err(err) => {
                tracing::debug!(key, %err, "QQ 歌词字段解密失败");
                None
            }
        }
    };
    let original = decrypted("lyric")
        .map(|text| parse_lyrics_bytes(text.as_bytes()))
        .unwrap_or_default();
    let translations = decrypted("trans")
        .map(|text| parse_qq_translation_text(&text))
        .unwrap_or_default();
    let romans = decrypted("roma")
        .map(|text| parse_lyrics_bytes(text.as_bytes()))
        .unwrap_or_default();
    normalize_lyric_lines(attach_lyric_tracks(original, translations, romans)).map(clamp_lyrics)
}

/// QQ `trans`（解密后）：普通 LRC，但带 `[kana:…]` 注音行，没有原文的行写 `//` 占位
/// （LDDC `has_content` 同样把 `//` 视为空）。
pub(crate) fn parse_qq_translation_text(text: &str) -> Vec<LyricLine> {
    let cleaned = text
        .lines()
        .filter(|line| {
            !line
                .trim_start_matches('\u{feff}')
                .trim_start()
                .starts_with("[kana:")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut lines = parse_lyrics_bytes(cleaned.as_bytes());
    lines.retain(|line| line.text.trim() != "//");
    lines
}

/// 译文 / 音译轨与原文对齐的最大起点差（毫秒）：同一份歌词的不同轨起点通常完全相等或只差
/// 几十毫秒（网易云 tlyric 与 yrc 差 ~40 ms），1 s 已足够宽松，又不会把相邻两句串起来。
pub(crate) const TRACK_ALIGN_TOLERANCE_MS: u64 = 1000;

/// 把独立的译文轨与音译轨并入原文：按「起点差最小的一对一匹配」（LDDC `find_closest_match`
/// 的语义，本项目独立实现，每行只看最近的两个原文起点，O(n log n)）。匹配上的译文写进原文行的
/// `translations` 字段、音译写进 `roman` 字段（模型重构后译文统一是字段，不再拼相邻行）。
/// 差距超过容差或原文已被占用的译文行按原时间作为独立行保留（不丢内容），落单的音译行丢弃。
pub(crate) fn attach_lyric_tracks(
    mut original: Vec<LyricLine>,
    translations: Vec<LyricLine>,
    romans: Vec<LyricLine>,
) -> Vec<LyricLine> {
    if translations.is_empty() && romans.is_empty() {
        return original;
    }
    original.sort_by_key(|line| line.start_ms);

    for (roman_index, matched) in match_tracks_by_start(&original, &romans)
        .into_iter()
        .enumerate()
    {
        if let Some(original_index) = matched {
            original[original_index].roman = Some(LyricText::new(romans[roman_index].text.clone()));
        }
    }

    let matches = match_tracks_by_start(&original, &translations);
    let mut standalone = Vec::new();
    for (translation, matched) in translations.into_iter().zip(matches) {
        match matched {
            Some(original_index) => original[original_index]
                .translations
                .push(LyricText::new(translation.text)),
            None => standalone.push(translation),
        }
    }
    if !standalone.is_empty() {
        original.extend(standalone);
        // 稳定排序：落单译文按自身时间归位
        original.sort_by_key(|line| line.start_ms);
    }
    original
}

/// 返回 `tracks[j]` 匹配到的 `original` 下标（None = 落单）。候选对只取每行前后最近的两个
/// 原文起点，按差值升序贪心一对一分配；`original` 须已按时间排序。
fn match_tracks_by_start(original: &[LyricLine], tracks: &[LyricLine]) -> Vec<Option<usize>> {
    let mut pairs: Vec<(u64, usize, usize)> = Vec::new();
    for (track_index, line) in tracks.iter().enumerate() {
        let upper = original.partition_point(|candidate| candidate.start_ms < line.start_ms);
        let nearest = [
            upper.checked_sub(1),
            (upper < original.len()).then_some(upper),
        ];
        for original_index in nearest.into_iter().flatten() {
            let diff = original[original_index].start_ms.abs_diff(line.start_ms);
            if diff <= TRACK_ALIGN_TOLERANCE_MS {
                pairs.push((diff, original_index, track_index));
            }
        }
    }
    pairs.sort();

    let mut taken = vec![false; original.len()];
    let mut matched = vec![None; tracks.len()];
    for (_, original_index, track_index) in pairs {
        if taken[original_index] || matched[track_index].is_some() {
            continue;
        }
        taken[original_index] = true;
        matched[track_index] = Some(original_index);
    }
    matched
}

/// 网易云 `song/lyric`：原文优先 `yrc`（逐字），空则 `lrc`（逐行）。译文 / 音译不再因为拿到
/// yrc 就丢弃（此前 yrc 命中即返回，bad guy 这类同时有 yrc + ytlrc + tlyric 的歌只剩原文）：
/// 逐字来源优先与 yrc 行对齐的 `ytlrc` / `yromalrc`，其次 `tlyric` / `romalrc`（与 lrc 对齐，
/// 起点可能差几十毫秒，由 `attach_lyric_tracks` 按最近起点并入）。没有时间戳的译文轨是
/// 4 秒等差的假时间轴，对不上任何原文，直接丢弃。
pub(crate) fn parse_netease_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    let field = |key: &str| {
        payload
            .get(key)
            .and_then(|value| value.get("lyric"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
    };
    let yrc = field("yrc")
        .map(|yrc| parse_lyrics_bytes(yrc.as_bytes()))
        .filter(|lines| !lines.is_empty());
    let word_synced = yrc.is_some();
    let original = yrc.unwrap_or_else(|| field("lrc").map(parse_lyrics_text).unwrap_or_default());
    let side_track = |aligned: &str, plain: &str| {
        word_synced
            .then(|| field(aligned))
            .flatten()
            .or_else(|| field(plain))
            .map(parse_lyrics_text)
            .filter(|lines| !lyrics_are_unsynced(lines))
            .unwrap_or_default()
    };
    let translations = side_track("ytlrc", "tlyric");
    let romans = side_track("yromalrc", "romalrc");
    normalize_lyric_lines(attach_lyric_tracks(original, translations, romans)).map(clamp_lyrics)
}

pub(crate) fn parse_qq_lyric_payload(payload: &Value) -> Option<Vec<LyricLine>> {
    let mut lyrics = Vec::new();
    if let Some(lyric) = payload.get("lyric").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(lyric));
    }
    if let Some(trans) = payload.get("trans").and_then(Value::as_str) {
        lyrics.extend(parse_online_lyric_text(trans));
    }
    normalize_lyric_lines(lyrics).map(clamp_lyrics)
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
    lyrics.sort_by_key(|line| line.start_ms);
    lyrics.dedup_by(|a, b| a.start_ms.abs_diff(b.start_ms) < 10 && a.text == b.text);
    (!lyrics.is_empty()).then_some(lyrics)
}

pub(crate) fn ranked_provider_items(items: &[Value], duration: u64) -> Vec<&Value> {
    let mut ranked = items.iter().collect::<Vec<_>>();
    // M-13：本地时长探测失败（duration=0）时保持接口原始相关度排序——
    // 否则 abs_diff(0) 退化成按候选时长升序，30 秒试听版会排第一并被自动选中。
    if duration == 0 {
        return ranked;
    }
    let target_ms = duration.saturating_mul(1000);
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
            .lines
            .iter()
            .map(|l| l.text.chars().count())
            .sum();
        total_chars.hash(&mut hasher);
        for line in candidate.lyrics.lines.iter().take(3) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn texts(lines: &[LyricLine]) -> Vec<&str> {
        lines.iter().map(|line| line.text.as_str()).collect()
    }

    fn translation_texts(line: &LyricLine) -> Vec<&str> {
        line.translations
            .iter()
            .map(|text| text.text.as_str())
            .collect()
    }

    fn candidate(id: &str, duration: Option<u64>, lyrics: Vec<LyricLine>) -> OnlineLyricsCandidate {
        OnlineLyricsCandidate {
            id: id.into(),
            source: String::new(),
            title: String::new(),
            artist: String::new(),
            album: None,
            duration,
            lyrics: LyricDocument::from_lines(lyrics, LyricSource::default()),
            ttml_lookup_keys: Vec::new(),
        }
    }

    fn word_line(start_ms: u64, text: &str) -> LyricLine {
        let mut line = LyricLine::new(start_ms, text);
        line.words = Some(vec![LyricWord::new(start_ms, Some(start_ms + 1000), text)]);
        line
    }

    #[test]
    fn capability_tier_orders_word_and_translation_forms() {
        let plain = vec![LyricLine::new(1000, "a"), LyricLine::new(2000, "b")];
        assert_eq!(candidate_capability_tier(&plain), 3);
        // LRC 形态译文：相邻同时间戳行
        let bilingual = vec![LyricLine::new(1000, "a"), LyricLine::new(1000, "甲")];
        assert_eq!(candidate_capability_tier(&bilingual), 2);
        assert_eq!(candidate_capability_tier(&[word_line(1000, "a")]), 1);
        let mut ttml = word_line(1000, "a");
        ttml.translations.push(LyricText::new("甲"));
        assert_eq!(candidate_capability_tier(&[ttml]), 0);
        // 同时间戳但同文本（去重残留）不算译文
        let dup = vec![LyricLine::new(1000, "a"), LyricLine::new(1000, "a")];
        assert_eq!(candidate_capability_tier(&dup), 3);
        // 同起点的和声行不是译文
        let mut background = LyricLine::new(1000, "(oh)");
        background.role = LyricRole::Background;
        assert_eq!(
            candidate_capability_tier(&[LyricLine::new(1000, "a"), background]),
            3
        );
    }

    #[test]
    fn ranking_sinks_duration_mismatch_and_prefers_word_synced_in_auto_mode() {
        let list = vec![
            candidate("netease-1", Some(262), vec![LyricLine::new(1000, "line")]),
            candidate("kugou-2", Some(261), vec![word_line(1000, "word")]),
            candidate("qq-3", Some(30), vec![word_line(1000, "snippet")]),
            candidate(
                "qq-4",
                None,
                vec![LyricLine::new(1000, "a"), LyricLine::new(1000, "甲")],
            ),
        ];
        let ids = |ranked: &[OnlineLyricsCandidate]| {
            ranked.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
        };
        // 自动：逐字 > 译文 > 逐行；30 秒片段沉底但不丢
        let ranked = rank_online_lyrics_candidates(list.clone(), 262, LyricsSourcePriority::Auto);
        assert_eq!(ids(&ranked), ["kugou-2", "qq-4", "netease-1", "qq-3"]);
        // 指定网易云优先：源顺序压过档次
        let ranked =
            rank_online_lyrics_candidates(list.clone(), 262, LyricsSourcePriority::Netease);
        assert_eq!(ids(&ranked), ["netease-1", "kugou-2", "qq-4", "qq-3"]);
        // 本地时长未知：不做时长判断
        let ranked = rank_online_lyrics_candidates(list, 0, LyricsSourcePriority::Auto);
        assert_eq!(ids(&ranked), ["kugou-2", "qq-3", "qq-4", "netease-1"]);
    }

    #[test]
    fn kugou_keyword_is_artist_dash_title() {
        assert_eq!(kugou_lyrics_keyword("唯一", "王力宏"), "王力宏 - 唯一");
        // 多艺术家：任一常见分隔符 → 顿号（LDDC 口径）
        assert_eq!(
            kugou_lyrics_keyword("唯一", "王力宏 / 张三 & 李四"),
            "王力宏、张三、李四 - 唯一"
        );
        // 艺术家缺失 / Unknown → 只用曲名；手动搜索把整句放在 title、artist 传空
        assert_eq!(kugou_lyrics_keyword("唯一", ""), "唯一");
        assert_eq!(kugou_lyrics_keyword("唯一", "Unknown"), "唯一");
        assert_eq!(kugou_lyrics_keyword(" 王力宏 唯一 ", ""), "王力宏 唯一");
        assert_eq!(kugou_lyrics_keyword("", "王力宏"), "");
    }

    fn ids<'a>(ranked: &[&'a Value]) -> Vec<&'a str> {
        ranked
            .iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .collect()
    }

    #[test]
    fn kugou_candidates_rank_by_score_then_duration() {
        let items = vec![
            json!({ "id": "a", "score": 40, "duration": 262_000 }),
            json!({ "id": "b", "score": 60, "duration": 30_000 }),
            json!({ "id": "c", "score": 60, "duration": 262_000 }),
            json!({ "id": "d", "score": "50", "duration": 262_000 }),
        ];
        // 分数优先；同分按时长接近度；字符串分数也认
        assert_eq!(
            ids(&ranked_kugou_candidates(&items, 262)),
            ["c", "b", "d", "a"]
        );
        // 时长未知：同分保持接口原序
        assert_eq!(
            ids(&ranked_kugou_candidates(&items, 0)),
            ["b", "c", "d", "a"]
        );
    }

    #[test]
    fn attach_puts_translation_and_roman_into_fields_of_nearest_original() {
        let mut first = LyricLine::new(14_100, "White shirt");
        first.end_ms = Some(17_670);
        first.words = Some(vec![LyricWord::new(14_100, Some(17_670), "White shirt")]);
        let original = vec![first, LyricLine::new(17_670, "Sleeping")];
        // 译文起点与原文差 40 ms / 10 ms；音译只给第一句
        let translations = vec![
            LyricLine::new(14_060, "白色的衬衫"),
            LyricLine::new(17_680, "沉睡着"),
        ];
        let romans = vec![LyricLine::new(14_100, "waito shaatsu")];

        let merged = attach_lyric_tracks(original, translations, romans);
        assert_eq!(texts(&merged), ["White shirt", "Sleeping"]);
        assert_eq!(translation_texts(&merged[0]), ["白色的衬衫"]);
        assert_eq!(translation_texts(&merged[1]), ["沉睡着"]);
        assert_eq!(merged[0].roman_text(), Some("waito shaatsu"));
        assert!(merged[1].roman.is_none());
        // 原文自身的 words / end 原样保留
        assert_eq!(merged[0].words.as_ref().map(Vec::len), Some(1));
        assert_eq!(merged[0].end_ms, Some(17_670));
    }

    #[test]
    fn attach_keeps_unmatched_translation_standalone_and_drops_unmatched_roman() {
        let original = vec![LyricLine::new(10_000, "a"), LyricLine::new(12_000, "b")];
        // 50.0 s 离任何原文都超过容差；两条 10.0 s 译文只有一条能配上，另一条按自身时间独立存在
        let translations = vec![
            LyricLine::new(50_000, "far"),
            LyricLine::new(10_000, "t1"),
            LyricLine::new(10_000, "t2"),
        ];
        let romans = vec![LyricLine::new(30_000, "orphan")];
        let merged = attach_lyric_tracks(original, translations, romans);
        assert_eq!(texts(&merged), ["a", "t2", "b", "far"]);
        assert_eq!(translation_texts(&merged[0]), ["t1"]);
        assert_eq!(merged[3].start_ms, 50_000);
        assert!(merged.iter().all(|line| line.roman.is_none()));
        // 没有副轨时原样返回
        let plain = vec![LyricLine::new(1000, "x")];
        assert_eq!(
            attach_lyric_tracks(plain.clone(), Vec::new(), Vec::new()),
            plain
        );
    }

    #[test]
    fn netease_payload_keeps_translation_and_roman_alongside_yrc() {
        let payload = json!({
            "lrc": { "lyric": "[00:14.06]White shirt\n[00:17.68]Sleeping\n" },
            "yrc": { "lyric": "[14100,3570](14100,420,0)White (14520,480,0)shirt\n[17670,3600](17670,600,0)Sleeping\n" },
            "ytlrc": { "lyric": "[00:14.100]白色的衬衫\n[00:17.670]沉睡着\n" },
            "tlyric": { "lyric": "[by:某人]\n[00:14.06]旧译文\n" },
            "romalrc": { "lyric": "[00:14.06]waito shaatsu\n" }
        });
        let lines = parse_netease_lyric_payload(&payload).expect("lyrics");
        assert_eq!(texts(&lines), ["White shirt", "Sleeping"]);
        // 原文来自 yrc（逐字）；译文优先 ytlrc（与 yrc 对齐），tlyric 不再重复并入
        assert_eq!(lines[0].words.as_ref().map(Vec::len), Some(2));
        assert_eq!(translation_texts(&lines[0]), ["白色的衬衫"]);
        assert_eq!(translation_texts(&lines[1]), ["沉睡着"]);
        // 只有 romalrc（对齐 lrc，差 40 ms）时也能并到 yrc 行
        assert_eq!(lines[0].roman_text(), Some("waito shaatsu"));
    }

    #[test]
    fn netease_payload_line_level_still_pairs_lrc_with_tlyric() {
        let payload = json!({
            "lrc": { "lyric": "[00:00.000] 作词 : 某人\n[00:30.542]故事的小黄花\n[00:34.000]从出生那年就飘着\n" },
            "tlyric": { "lyric": "[00:00.950]\n[00:30.542]Little yellow flower\n" }
        });
        let lines = parse_netease_lyric_payload(&payload).expect("lyrics");
        assert_eq!(
            texts(&lines),
            ["作词 : 某人", "故事的小黄花", "从出生那年就飘着"]
        );
        assert_eq!(translation_texts(&lines[1]), ["Little yellow flower"]);
        assert!(lines[0].translations.is_empty() && lines[2].translations.is_empty());
        assert!(lines.iter().all(|line| line.words.is_none()));
    }

    #[test]
    fn netease_payload_drops_unsynced_translation_track() {
        let payload = json!({
            "lrc": { "lyric": "[00:01.00]a\n[00:05.00]b\n" },
            "tlyric": { "lyric": "无时间戳的译文\n第二行\n" }
        });
        let lines = parse_netease_lyric_payload(&payload).expect("lyrics");
        assert_eq!(texts(&lines), ["a", "b"]);
        assert!(lines.iter().all(|line| line.translations.is_empty()));
    }

    #[test]
    fn qq_translation_text_drops_kana_tag_and_placeholder_lines() {
        let text = "[ti:水中リフレクション]\n[offset:0]\n[kana:1す(201,159)い(360,121)1ちゅう]\n[00:00.20]TME享有本翻译作品的著作权\n[00:02.16]//\n[00:29.44]朝着遥远深邃之处缓缓下沉\n";
        let lines = parse_qq_translation_text(text);
        assert_eq!(
            texts(&lines),
            ["TME享有本翻译作品的著作权", "朝着遥远深邃之处缓缓下沉"]
        );
        assert_eq!(lines[1].start_ms, 29_440);
        // 只有 kana 与标签、没有任何歌词行 → 空，而不是把注音串当歌词
        assert!(parse_qq_translation_text("[ti:x]\n[kana:1す(201,159)]\n").is_empty());
    }

    // 三段向量由独立 3DES 实现生成（明文 → zlib → 3DES-ECB(QRC_KEY) → hex）：
    // lyric = QRC 容器 `[1000,2000]he(1000,500)llo(1500,500)` / `[3000,1000]world(3000,1000)`
    // trans = `[kana:…]` + `[00:01.00]你好` + `[00:03.00]//`
    // roma  = QRC 容器 `he (1000,500)llo (1500,500)` / `wo (3000,500)rld (3500,500)`
    const QQ_LYRIC_HEX: &str = "0C8D67DD3E549974B64ED2680459F13881AA15D10DB4CC8324B86311D0D741BD6AF5D8724F2B75716C3A763AFD2E1295440B85EA0FE0BC84E3E9E35CF02D8CD9378E8568C45FC144C4C8D9B70CB163DA9D4A809AAEFBF861B197F5DCA6E03F41736731C3D41C7E266E5814A0D03DE379888D35B887555E4B0071C0D3B0E905C62A3F74AD8F130BFB27146FA8698F33C051931B38F140BAC2E68F117802E7391771B7F807A965691B30A74940C000AD63";
    const QQ_TRANS_HEX: &str = "5DCFF376CA238C449DE4FDFD218DED2DC7021FC49908B1705AEFCD2F5DC3BE203D86DAAA19A8A112FAF102C0711469CEA9FB2C68ACBB21FE6F91F68DF6EC9B76C28178FBC9F3F6306B45F5D20B39CDCCC9A522C835CA20C0B7C6B0DBE1505374";
    const QQ_ROMA_HEX: &str = "C90DB2E3F6940A43538B45865EB6753863C981F936A71A093B450246D48B65F00E18C3F4862A65A8A2740777BCE486B09E770AF59C690B9B68D63C8D2E4A5A8C137A4C08D9091DA0C893EB6BD001D984D54352C2541EABA651FE5BC686117BF7";

    #[test]
    fn qq_play_lyric_payload_yields_words_translation_and_roman() {
        let data = json!({
            "lyric": QQ_LYRIC_HEX, "qrc": 1, "qrc_t": 1571572791, "lrc_t": 0,
            "trans": QQ_TRANS_HEX, "trans_t": 1571572791,
            "roma": QQ_ROMA_HEX, "roma_t": 1466496774
        });
        let lines = parse_qq_play_lyric_payload(&data).expect("lyrics");
        assert_eq!(texts(&lines), ["hello", "world"]);
        let words = lines[0].words.as_ref().expect("qrc words");
        assert_eq!(
            words
                .iter()
                .map(|word| (word.text.as_str(), word.start_ms, word.end_ms))
                .collect::<Vec<_>>(),
            [("he", 1000, Some(1500)), ("llo", 1500, Some(2000))]
        );
        assert_eq!(lines[0].end_ms, Some(3000));
        // 译文进原文行的字段；`//` 占位与 [kana:] 已剔除
        assert_eq!(translation_texts(&lines[0]), ["你好"]);
        assert!(lines[1].translations.is_empty());
        // 音译按行拼音节写进 roman
        assert_eq!(lines[0].roman_text(), Some("he llo"));
        assert_eq!(lines[1].roman_text(), Some("wo rld"));

        // 空字段 / 坏密文只让该项缺席，不影响其余
        let partial = json!({ "lyric": QQ_LYRIC_HEX, "trans": "", "roma": "ZZZZ" });
        let lines = parse_qq_play_lyric_payload(&partial).expect("lyrics");
        assert_eq!(texts(&lines), ["hello", "world"]);
        assert!(lines.iter().all(|line| line.roman.is_none()));
        assert!(parse_qq_play_lyric_payload(&json!({ "lyric": "" })).is_none());
    }
}
