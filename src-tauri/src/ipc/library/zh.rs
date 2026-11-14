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
                words_to_traditional(words);
            }
        }
        if let Some(words) = line.words.as_mut() {
            words_to_traditional(words);
        }
    }
}

/// 逐字音节按**整行上下文**转换（L4-01）：中文逐字歌词通常一字一音节，逐音节转换
/// 会丢掉词语上下文（「头」「发」分开转得到「頭發」，整行却是「頭髮」）。
/// 做法是把音节拼成整串转换一次，再按各音节原有的字符数切回去；简繁转换绝大多数
/// 是一字对一字，转换前后字符数不等时（少见的一对多词条）退回逐音节转换。
fn words_to_traditional(words: &mut [LyricWord]) {
    if words.is_empty() {
        return;
    }
    let joined: String = words.iter().map(|word| word.text.as_str()).collect();
    if !joined.chars().any(is_cjk) {
        return;
    }
    let converted = to_traditional(&joined);
    if converted.chars().count() != joined.chars().count() {
        for word in words {
            word.text = to_traditional(&word.text);
        }
        return;
    }
    let mut chars = converted.chars();
    for word in words {
        let count = word.text.chars().count();
        word.text = chars.by_ref().take(count).collect();
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

    #[test]
    fn per_character_words_convert_with_line_context() {
        // 一字一音节：逐音节转换会把「发」按默认映射转成「發」，与整行「頭髮」不一致
        let text = "我的头发在后面";
        let words = text
            .chars()
            .enumerate()
            .map(|(index, ch)| {
                LyricWord::new(
                    index as u64 * 100,
                    Some(index as u64 * 100 + 100),
                    &ch.to_string(),
                )
            })
            .collect::<Vec<_>>();
        let line = LyricLine {
            words: Some(words),
            ..LyricLine::new(0, text)
        };
        let converted = lyrics_to_traditional(LyricDocument::from_lines(
            vec![line],
            LyricSource::default(),
        ));
        let line = &converted.lines[0];
        let joined: String = line
            .words
            .as_ref()
            .unwrap()
            .iter()
            .map(|word| word.text.as_str())
            .collect();
        assert_eq!(joined, line.text, "音节拼接必须等于整行");
        assert!(line.text.contains("頭髮"), "整行应按词转换：{}", line.text);
        // 音节数与各音节字符数不变
        assert_eq!(line.words.as_ref().unwrap().len(), text.chars().count());
        assert!(line
            .words
            .as_ref()
            .unwrap()
            .iter()
            .all(|w| w.text.chars().count() == 1));
    }

    #[test]
    fn words_without_cjk_are_untouched() {
        let mut words = vec![
            LyricWord::new(0, Some(100), "Hello "),
            LyricWord::new(100, Some(200), "world"),
        ];
        words_to_traditional(&mut words);
        assert_eq!(words[0].text, "Hello ");
        assert_eq!(words[1].text, "world");
    }
}
