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

pub(crate) fn lyrics_to_traditional(mut lyrics: LyricDocument) -> LyricDocument {
    lines_to_traditional(&mut lyrics.lines);
    lyrics
}

/// 原文、译文、逐字音节都转；音译不动。
pub(crate) fn lines_to_traditional(lines: &mut [LyricLine]) {
    for line in lines {
        line.text = to_traditional(&line.text);
        for translation in &mut line.translations {
            translation.text = to_traditional(&translation.text);
            if let Some(words) = translation.words.as_mut() {
                for word in words {
                    word.text = to_traditional(&word.text);
                }
            }
        }
        if let Some(words) = line.words.as_mut() {
            for word in words {
                word.text = to_traditional(&word.text);
            }
        }
    }
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
            words: Some(vec![LyricWord::new(0, Some(1000), "头发")]),
            translations: vec![LyricText::new("发展")],
            roman: Some(LyricText::new("tou fa")),
            ..LyricLine::new(0, "头发")
        };
        let converted = lyrics_to_traditional(LyricDocument::from_lines(
            vec![line],
            LyricSource::default(),
        ));
        let converted = &converted.lines;
        assert_eq!(converted[0].text, "頭髮");
        assert_eq!(converted[0].translation_text(), Some("發展"));
        assert_eq!(converted[0].words.as_ref().unwrap()[0].text, "頭髮");
        // 音译不动
        assert_eq!(converted[0].roman_text(), Some("tou fa"));
    }
}
