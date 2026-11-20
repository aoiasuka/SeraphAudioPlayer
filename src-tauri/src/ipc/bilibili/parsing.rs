use super::prelude::*;

pub(crate) fn extract_bvid(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    trimmed
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .find(|part| {
            let part = part.as_bytes();
            part.len() >= 12
                && part[0].eq_ignore_ascii_case(&b'B')
                && part[1].eq_ignore_ascii_case(&b'V')
        })
        .map(|value| value.to_string())
}

/// BILI-05：输入里指定的分 P 序号（从 1 起）。认两种写法：
/// 链接查询参数 `p=N`（`…/video/BV…?p=3`、`BV…?p=3`），以及曲库来源 ID 的 `BV…-pN`。
/// 没有指定、不是正整数都返回 None。
pub(crate) fn extract_page_number(input: &str) -> Option<u32> {
    fn positive(digits: &str) -> Option<u32> {
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse::<u32>().ok().filter(|number| *number > 0)
    }

    let trimmed = input.trim();
    if let Some((_, rest)) = trimmed.split_once('?') {
        // 查询串到 `#` 或空白为止（分享文案里链接后面常跟着别的文字）
        let query = rest
            .split(|ch: char| ch == '#' || ch.is_whitespace())
            .next()
            .unwrap_or_default();
        return query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == "p")
            .and_then(|(_, value)| positive(value));
    }

    let (head, digits) = trimmed
        .rsplit_once("-p")
        .or_else(|| trimmed.rsplit_once("-P"))?;
    if extract_bvid(head).as_deref() != Some(head) {
        return None;
    }
    positive(digits)
}

pub(crate) fn extract_media_id(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) && !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }

    if let Some(start) = trimmed.find("/ml") {
        let value = trimmed[start + 3..]
            .chars()
            .take_while(|ch| ch.is_ascii_digit())
            .collect::<String>();
        if !value.is_empty() {
            return Some(value);
        }
    }

    for key in ["media_id", "fid"] {
        if let Some(value) = query_value(trimmed, key) {
            return Some(value);
        }
    }

    None
}

pub(crate) fn query_value(input: &str, key: &str) -> Option<String> {
    let marker = format!("{key}=");
    let start = input.find(&marker)? + marker.len();
    let value = input[start..].split(['&', '#', '?', '/']).next()?.trim();
    if value.chars().all(|ch| ch.is_ascii_digit()) && !value.is_empty() {
        Some(value.to_string())
    } else {
        None
    }
}

pub(crate) fn normalize_url(value: &str) -> String {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("//") {
        format!("https://{rest}")
    } else if let Some(rest) = value.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        value.to_string()
    }
}

pub(crate) fn sanitize_file_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>();
    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "bilibili-audio".into()
    } else {
        trimmed
    }
}

pub(crate) fn format_from_codec(codec: &str) -> &'static str {
    let codec = codec.to_ascii_lowercase();
    if codec.contains("flac") {
        "FLAC"
    } else if codec.contains("ec-3") || codec.contains("eac3") || codec.contains("ac-4") {
        "EAC3"
    } else if codec.contains("mp4a") {
        "M4A"
    } else if codec.contains("opus") {
        "OPUS"
    } else {
        "AUDIO"
    }
}

pub(crate) fn format_bilibili_quality(
    format: &str,
    bitrate: Option<u32>,
    kind: &AudioKind,
    remuxed: bool,
) -> String {
    let quality = match kind {
        AudioKind::DolbyAtmos => "Bilibili Dolby Atmos",
        AudioKind::Flac => {
            if remuxed {
                "Bilibili FLAC"
            } else {
                "Bilibili FLAC stream"
            }
        }
        AudioKind::Dolby => "Bilibili Dolby",
        AudioKind::Normal => "Bilibili",
    };

    match bitrate {
        Some(value) if value > 0 => format!("{format} {quality} / {value} kbps"),
        _ => format!("{format} {quality}"),
    }
}

// format_bitrate / format_file_size / color_pair 与本地曲库共用 `ipc::library` 的实现（此前各有一份）。
pub(crate) use crate::ipc::library::{color_pair, format_bitrate, format_file_size};

pub(crate) fn find_ffmpeg(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(app_dir) = app.path().app_data_dir() {
        seraph_decoder::configure_ffmpeg_search_dirs([app_dir.join("ffmpeg")]);
    }

    seraph_decoder::find_ffmpeg()
}

pub(crate) fn login_poll_message(code: i32) -> String {
    match code {
        86101 => "等待扫码".into(),
        86090 => "已扫码，等待手机确认".into(),
        86038 => "二维码已过期".into(),
        _ => format!("登录状态码 {code}"),
    }
}

impl<T> ApiResponse<T> {
    pub(crate) fn into_data(self, label: &str) -> Result<T, String> {
        if self.code != 0 {
            return Err(format!(
                "{label} request failed: {}",
                self.message
                    .unwrap_or_else(|| format!("code {}", self.code))
            ));
        }

        self.data
            .ok_or_else(|| format!("{label} response has no data"))
    }
}
