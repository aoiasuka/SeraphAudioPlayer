//! 歌词简→繁转换（设置「更喜欢繁体中文」）。
//!
//! 用纯 Rust 的 `zhconv`（内置 OpenCC 词表，词级转换，不是逐字映射），
//! 在歌词**写入/返回前**转换：在线候选（预览与应用一致）、本地导入。
//! 已存在曲库里的歌词不回溯改写，关闭开关也不逆转——与 SPlayer 的
//! 「下一首生效」语义一致，且避免繁→简的有损往返。

use super::prelude::*;

pub(crate) fn to_traditional(text: &str) -> String {
    if text.is_empty() || !text.chars().any(is_cjk) {
        return text.to_string();
    }
    zhconv::zhconv(text, zhconv::Variant::ZhHant)
}

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F
    )
}

pub(crate) fn lyrics_to_traditional(mut lyrics: Vec<LyricLine>) -> Vec<LyricLine> {
    for line in &mut lyrics {
        line.text = to_traditional(&line.text);
        if let Some(translation) = line.translation.as_mut() {
            *translation = to_traditional(translation);
        }
        if let Some(words) = line.words.as_mut() {
            for word in words {
                word.text = to_traditional(&word.text);
            }
        }
    }
    lyrics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_simplified_to_traditional_and_leaves_others() {
        assert_eq!(to_traditional("简体中文歌词"), "簡體中文歌詞");
        assert_eq!(to_traditional("Hello world"), "Hello world");
        assert_eq!(to_traditional(""), "");
    }

    #[test]
    fn converts_text_translation_and_words() {
        let line = LyricLine {
            time: 0.0,
            text: "头发".into(),
            end: None,
            words: Some(vec![LyricWord {
                start: 0.0,
                end: 1.0,
                text: "头发".into(),
            }]),
            translation: Some("发展".into()),
            roman: Some("tou fa".into()),
            hidden: false,
        };
        let converted = lyrics_to_traditional(vec![line]);
        assert_eq!(converted[0].text, "頭髮");
        assert_eq!(converted[0].translation.as_deref(), Some("發展"));
        assert_eq!(converted[0].words.as_ref().unwrap()[0].text, "頭髮");
        // 音译不动
        assert_eq!(converted[0].roman.as_deref(), Some("tou fa"));
    }
}
