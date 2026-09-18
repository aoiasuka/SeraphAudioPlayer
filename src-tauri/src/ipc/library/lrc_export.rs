//! 曲库歌词 → LRC 文本序列化（增强型 / 逐字 / 逐行），供 `export_track_lyrics` 命令使用。
//!
//! 三种格式共用一套音节模型，只差括号与摆法（对照 LDDC 的 `lyrics_line2str`，本文件为
//! 独立实现）：
//!
//! - 增强型（ESLyric）：`[s]<s>我<e1>的<e2>天<e3>` —— `<t>` 在音节前，共享标签只写一次
//! - 逐字：`[s]我[e1]的[e2]天[e3]` —— `[t]` 在音节后，首音节起点等于行起点时不重复写
//! - 逐行：`[s]我的天`
//!
//! 译文两种形态都能写出：TTML 的 `translation` / `roman` 字段各成一行（音译在原文前、
//! 译文在原文后，与 LDDC 默认 `roma, orig, ts` 顺序一致）；LRC 类来源的译文本来就是
//! 相邻同时间戳行，按顺序原样写出。所有时间已在解析时折进 offset，**不写 `[offset:]`**。
//!
//! 安全：LRC 是行敏感、括号敏感的文本格式。字段里的 CR / LF / U+2028 / U+2029 会伪造下一行，
//! `<` `>` `[` `]` 会被任何 LRC 解析器（包括本项目）当成标签边界，全部经 `sanitize_lrc_text`
//! 归一（换行 → 空格，半角括号 → 全角），回归闸是「导出 → 再解析」的 round-trip 测试。

use super::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LrcExportFormat {
    /// 增强型 LRC（ESLyric）：`<mm:ss.xxx>` 在音节前
    Enhanced,
    /// 逐字 LRC：`[mm:ss.xxx]` 在音节后
    Verbatim,
    /// 逐行 LRC
    Line,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcExportOptions {
    /// 毫秒位数：2（四舍五入到 10 ms）或 3；其它值按 3 处理
    #[serde(default = "default_ms_digits")]
    pub ms_digits: u8,
    #[serde(default = "default_true")]
    pub include_translation: bool,
    #[serde(default = "default_true")]
    pub include_roman: bool,
}

fn default_ms_digits() -> u8 {
    3
}

fn default_true() -> bool {
    true
}

impl Default for LrcExportOptions {
    fn default() -> Self {
        Self {
            ms_digits: 3,
            include_translation: true,
            include_roman: true,
        }
    }
}

/// 写进文件头 ID 标签的元数据；空字段不写。
#[derive(Clone, Debug, Default)]
pub struct LrcExportMeta<'a> {
    pub title: &'a str,
    pub artist: &'a str,
    pub album: &'a str,
    /// `[by:]` 内容（工具名 + 版本）
    pub tool: &'a str,
}

/// N-04 同型：换行类字符 → 空格；半角尖括号 / 方括号 → 全角，避免被当成时间标签边界。
pub(crate) fn sanitize_lrc_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => ' ',
            '<' => '＜',
            '>' => '＞',
            '[' => '［',
            ']' => '］',
            other => other,
        })
        .collect()
}

/// 秒 → 按位数取整的毫秒（2 位模式先四舍五入到 10 ms，时间轴仍单调）。
fn quantize_ms(seconds: f64, ms_digits: u8) -> u64 {
    // 先取整到毫秒，避免 1.005 × 1000 = 1004.999… 这类浮点误差影响 10 ms 四舍五入
    let ms = (seconds.max(0.0) * 1000.0).round();
    if ms_digits == 2 {
        (ms / 10.0).round() as u64 * 10
    } else {
        ms as u64
    }
}

fn format_ms(ms: u64, ms_digits: u8) -> String {
    let minutes = ms / 60_000;
    let seconds = (ms / 1000) % 60;
    let millis = ms % 1000;
    if ms_digits == 2 {
        format!("{minutes:02}:{seconds:02}.{:02}", millis / 10)
    } else {
        format!("{minutes:02}:{seconds:02}.{millis:03}")
    }
}

struct Formatter {
    ms_digits: u8,
}

impl Formatter {
    fn ms(&self, seconds: f64) -> u64 {
        quantize_ms(seconds, self.ms_digits)
    }

    fn line_tag(&self, ms: u64) -> String {
        format!("[{}]", format_ms(ms, self.ms_digits))
    }

    fn word_tag(&self, ms: u64, format: LrcExportFormat) -> String {
        match format {
            LrcExportFormat::Enhanced => format!("<{}>", format_ms(ms, self.ms_digits)),
            _ => format!("[{}]", format_ms(ms, self.ms_digits)),
        }
    }
}

/// 一行歌词（含音节）→ 一行 LRC 文本（不含换行）。
fn line_to_lrc(line: &LyricLine, format: LrcExportFormat, fmt: &Formatter) -> String {
    let words = line.words.as_deref().unwrap_or(&[]);
    // 行起点取首音节起点（有 words 时），否则行时间
    let start_ms = words
        .first()
        .map(|word| fmt.ms(word.start))
        .unwrap_or_else(|| fmt.ms(line.time));
    let line_end_ms = line
        .end
        .map(|end| fmt.ms(end))
        .filter(|end| *end > start_ms);

    let mut out = fmt.line_tag(start_ms);
    if matches!(format, LrcExportFormat::Line) || words.is_empty() {
        out.push_str(&sanitize_lrc_text(&line.text));
        if !matches!(format, LrcExportFormat::Line) {
            if let Some(end) = line_end_ms {
                out.push_str(&fmt.word_tag(end, format));
            }
        }
        return out;
    }

    // 逐字型首音节起点 = 行起点时不重复写；增强型首音节总要写起点
    let mut last_written_ms = match format {
        LrcExportFormat::Verbatim => Some(start_ms),
        _ => None,
    };
    for word in words {
        let word_start = fmt.ms(word.start);
        if last_written_ms != Some(word_start) {
            out.push_str(&fmt.word_tag(word_start, format));
        }
        out.push_str(&sanitize_lrc_text(&word.text));
        let word_end = fmt.ms(word.end);
        // 零时长音节 = 终点未知（解析占位），不写终点标签
        if word_end > word_start {
            out.push_str(&fmt.word_tag(word_end, format));
            last_written_ms = Some(word_end);
        } else {
            last_written_ms = Some(word_start);
        }
    }
    if let Some(end) = line_end_ms {
        if last_written_ms.is_none_or(|last| end > last) {
            out.push_str(&fmt.word_tag(end, format));
        }
    }
    out
}

/// 整篇歌词 → LRC 文本（UTF-8，`\n` 换行，无 BOM）。空文本行跳过。
pub(crate) fn lyrics_to_lrc(
    lines: &[LyricLine],
    format: LrcExportFormat,
    options: &LrcExportOptions,
    meta: &LrcExportMeta<'_>,
) -> String {
    let fmt = Formatter {
        ms_digits: if options.ms_digits == 2 { 2 } else { 3 },
    };
    let mut out = String::new();
    for (key, value) in [
        ("ti", meta.title),
        ("ar", meta.artist),
        ("al", meta.album),
        ("by", meta.tool),
    ] {
        let value = sanitize_lrc_text(value.trim());
        if !value.is_empty() {
            out.push_str(&format!("[{key}:{value}]\n"));
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }

    for line in lines {
        if line.text.trim().is_empty() {
            continue;
        }
        let start_ms = line
            .words
            .as_deref()
            .and_then(|words| words.first())
            .map(|word| fmt.ms(word.start))
            .unwrap_or_else(|| fmt.ms(line.time));
        if options.include_roman {
            if let Some(roman) = line
                .roman
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                out.push_str(&fmt.line_tag(start_ms));
                out.push_str(&sanitize_lrc_text(roman.trim()));
                out.push('\n');
            }
        }
        out.push_str(&line_to_lrc(line, format, &fmt));
        out.push('\n');
        if options.include_translation {
            if let Some(translation) = line
                .translation
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                out.push_str(&fmt.line_tag(start_ms));
                out.push_str(&sanitize_lrc_text(translation.trim()));
                out.push('\n');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(start: f64, end: f64, text: &str) -> LyricWord {
        LyricWord {
            start,
            end,
            text: text.to_string(),
        }
    }

    fn word_line(time: f64, end: f64, text: &str, words: Vec<LyricWord>) -> LyricLine {
        let mut line = LyricLine::new(time, text);
        line.end = Some(end);
        line.words = Some(words);
        line
    }

    fn sample() -> Vec<LyricLine> {
        vec![
            word_line(
                18.459,
                23.097,
                "我的 天",
                vec![
                    word(18.459, 18.814, "我"),
                    word(18.814, 19.284, "的 "),
                    word(19.284, 23.097, "天"),
                ],
            ),
            LyricLine::new(26.712, "普通行"),
        ]
    }

    const NO_META: LrcExportMeta<'static> = LrcExportMeta {
        title: "",
        artist: "",
        album: "",
        tool: "",
    };

    #[test]
    fn enhanced_output_shares_adjacent_tags() {
        let text = lyrics_to_lrc(
            &sample(),
            LrcExportFormat::Enhanced,
            &LrcExportOptions::default(),
            &NO_META,
        );
        assert_eq!(
            text,
            "[00:18.459]<00:18.459>我<00:18.814>的 <00:19.284>天<00:23.097>\n[00:26.712]普通行\n"
        );
    }

    #[test]
    fn verbatim_output_omits_first_start_tag() {
        let text = lyrics_to_lrc(
            &sample(),
            LrcExportFormat::Verbatim,
            &LrcExportOptions::default(),
            &NO_META,
        );
        assert_eq!(
            text,
            "[00:18.459]我[00:18.814]的 [00:19.284]天[00:23.097]\n[00:26.712]普通行\n"
        );
    }

    #[test]
    fn line_output_drops_word_tags_and_writes_header() {
        let meta = LrcExportMeta {
            title: "唯一",
            artist: "王力宏",
            album: "",
            tool: "Seraph Audio Player 0.6.1",
        };
        let text = lyrics_to_lrc(
            &sample(),
            LrcExportFormat::Line,
            &LrcExportOptions::default(),
            &meta,
        );
        assert_eq!(
            text,
            "[ti:唯一]\n[ar:王力宏]\n[by:Seraph Audio Player 0.6.1]\n\n[00:18.459]我的 天\n[00:26.712]普通行\n"
        );
    }

    #[test]
    fn two_digit_mode_rounds_to_centiseconds() {
        let options = LrcExportOptions {
            ms_digits: 2,
            ..LrcExportOptions::default()
        };
        let text = lyrics_to_lrc(&sample(), LrcExportFormat::Enhanced, &options, &NO_META);
        assert_eq!(
            text,
            "[00:18.46]<00:18.46>我<00:18.81>的 <00:19.28>天<00:23.10>\n[00:26.71]普通行\n"
        );
        assert_eq!(quantize_ms(1.005, 2), 1010);
        assert_eq!(quantize_ms(1.004, 2), 1000);
        assert_eq!(quantize_ms(1.010, 2), 1010);
    }

    #[test]
    fn translation_and_roman_fields_become_adjacent_lines() {
        let mut line = word_line(1.0, 2.0, "Hello", vec![word(1.0, 2.0, "Hello")]);
        line.translation = Some("你好".into());
        line.roman = Some("ha-ro".into());
        let text = lyrics_to_lrc(
            &[line.clone()],
            LrcExportFormat::Enhanced,
            &LrcExportOptions::default(),
            &NO_META,
        );
        assert_eq!(
            text,
            "[00:01.000]ha-ro\n[00:01.000]<00:01.000>Hello<00:02.000>\n[00:01.000]你好\n"
        );
        let options = LrcExportOptions {
            include_translation: false,
            include_roman: false,
            ..LrcExportOptions::default()
        };
        let text = lyrics_to_lrc(&[line], LrcExportFormat::Enhanced, &options, &NO_META);
        assert_eq!(text, "[00:01.000]<00:01.000>Hello<00:02.000>\n");
    }

    #[test]
    fn unknown_word_end_is_left_untagged_and_line_end_wins() {
        // 零时长末音节（解析占位）不写终点；行 end 大于最后写出的标签才补
        let line = word_line(
            1.0,
            3.0,
            "ab",
            vec![word(1.0, 1.5, "a"), word(1.5, 1.5, "b")],
        );
        let text = lyrics_to_lrc(
            &[line],
            LrcExportFormat::Enhanced,
            &LrcExportOptions::default(),
            &NO_META,
        );
        assert_eq!(text, "[00:01.000]<00:01.000>a<00:01.500>b<00:03.000>\n");
    }

    #[test]
    fn injected_newlines_and_brackets_are_neutralized() {
        let line = word_line(
            1.0,
            2.0,
            "x",
            vec![word(1.0, 2.0, "a\n<00:99.00>b[00:10.00]c")],
        );
        let meta = LrcExportMeta {
            title: "t]\n[00:00.00]evil",
            ..NO_META
        };
        let text = lyrics_to_lrc(
            &[line],
            LrcExportFormat::Enhanced,
            &LrcExportOptions::default(),
            &meta,
        );
        assert_eq!(text.lines().count(), 3, "{text}");
        let parsed = parse_lyrics_text(&text);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "a ＜00:99.00＞b［00:10.00］c");
    }

    #[test]
    fn round_trip_preserves_word_timing() {
        let original = sample();
        for format in [LrcExportFormat::Enhanced, LrcExportFormat::Verbatim] {
            let text = lyrics_to_lrc(&original, format, &LrcExportOptions::default(), &NO_META);
            let parsed = parse_lyrics_text(&text);
            assert_eq!(parsed.len(), 2, "{format:?}: {text}");
            assert_eq!(parsed[0].text, "我的 天");
            let words = parsed[0].words.as_ref().expect("words survive round trip");
            let expected = original[0].words.as_ref().unwrap();
            assert_eq!(words.len(), expected.len());
            for (got, want) in words.iter().zip(expected) {
                assert_eq!(got.text, want.text);
                assert!((got.start - want.start).abs() < 0.0015, "{format:?}");
                assert!((got.end - want.end).abs() < 0.0015, "{format:?}");
            }
            assert!((parsed[0].end.unwrap() - 23.097).abs() < 0.0015);
            assert!(parsed[1].words.is_none());
        }
    }
}
