//! AMLL / Apple Music 风格 TTML 逐字歌词解析。
//!
//! 只认我们要的子集：`<p begin end>` 是一行，直接子 `<span begin end>` 是一个
//! 音节，`ttm:role="x-translation"` / `x-roman` 是译文与音译，`x-bg` 是和声
//! （其内层音节并入本行、原样保留括号）。命名空间前缀不可靠（有的文件写
//! `ttm:role`，有的写默认前缀），一律按本地名比对。
//!
//! 输出仍走 `clamp_lyrics` 收口（行数/单行长度上限），并按行起始时间排序。

use super::prelude::*;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

/// 单个 TTML 文件的读取上限：逐字歌词通常 50~300 KB，2 MB 已远超正常上限。
pub(crate) const MAX_TTML_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpanRole {
    Word,
    Translation,
    Roman,
    Background,
    /// 未知角色：文本并入所在层级，不单独处理
    Other,
}

struct LineBuilder {
    begin: Option<f64>,
    end: Option<f64>,
    words: Vec<LyricWord>,
    /// 无逐字 span 的行，文本直接累积在这里
    plain: String,
    translation: String,
    roman: String,
}

impl LineBuilder {
    fn new(begin: Option<f64>, end: Option<f64>) -> Self {
        Self {
            begin,
            end,
            words: Vec::new(),
            plain: String::new(),
            translation: String::new(),
            roman: String::new(),
        }
    }

    fn finish(self) -> Option<LyricLine> {
        let word_text = self
            .words
            .iter()
            .map(|word| word.text.as_str())
            .collect::<String>();
        let raw_text = if word_text.trim().is_empty() {
            self.plain
        } else {
            word_text
        };
        let text = clean_lyric_text(&raw_text)?;
        let words = (!self.words.is_empty()).then(|| {
            self.words
                .into_iter()
                .filter(|word| !word.text.trim().is_empty())
                .collect::<Vec<_>>()
        });
        let begin = self.begin.or_else(|| {
            words
                .as_ref()
                .and_then(|words| words.first().map(|w| w.start))
        })?;
        let end = self
            .end
            .or_else(|| words.as_ref().and_then(|words| words.last().map(|w| w.end)));
        Some(LyricLine {
            time: begin,
            text,
            end,
            words: words.filter(|words| !words.is_empty()),
            translation: clean_lyric_text(&self.translation),
            roman: clean_lyric_text(&self.roman),
            hidden: false,
        })
    }
}

/// 解析 TTML 文本；不是 TTML 或没有可用行时返回空。
pub(crate) fn parse_ttml_lyrics(text: &str) -> Vec<LyricLine> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().trim_text(false);

    let mut lines: Vec<LyricLine> = Vec::new();
    let mut current: Option<LineBuilder> = None;
    // span 嵌套栈：role + 起止时间 + 累积文本（Word 层在 End 时收成一个音节）
    let mut span_stack: Vec<(SpanRole, Option<f64>, Option<f64>, String)> = Vec::new();
    let mut in_body = false;
    let mut saw_tt = false;

    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) => return Vec::new(),
        };
        match event {
            Event::Eof => break,
            Event::Start(start) | Event::Empty(start)
                if local_name(start.name().as_ref()) == b"tt" =>
            {
                saw_tt = true;
            }
            Event::Start(start) if local_name(start.name().as_ref()) == b"body" => {
                in_body = true;
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"body" => {
                in_body = false;
            }
            Event::Start(start) if in_body && local_name(start.name().as_ref()) == b"p" => {
                let (begin, end) = time_attrs(&start);
                current = Some(LineBuilder::new(begin, end));
                span_stack.clear();
            }
            Event::End(end) if local_name(end.name().as_ref()) == b"p" => {
                if let Some(line) = current.take().and_then(LineBuilder::finish) {
                    lines.push(line);
                }
                span_stack.clear();
            }
            Event::Start(start)
                if current.is_some() && local_name(start.name().as_ref()) == b"span" =>
            {
                let role = span_role(&start);
                let (begin, end) = time_attrs(&start);
                span_stack.push((role, begin, end, String::new()));
            }
            Event::End(end) if current.is_some() && local_name(end.name().as_ref()) == b"span" => {
                let Some((role, begin, span_end, text)) = span_stack.pop() else {
                    continue;
                };
                let line = current.as_mut().expect("checked above");
                match role {
                    SpanRole::Word => {
                        if let (Some(start), Some(finish)) = (begin, span_end) {
                            // 和声容器内的首个音节与前面主唱音节之间补空格，避免 "line(oh)" 粘连
                            let inside_bg =
                                matches!(span_stack.last(), Some((SpanRole::Background, ..)));
                            if inside_bg {
                                if let Some(last) = line.words.last_mut() {
                                    if !last.text.ends_with(char::is_whitespace)
                                        && !text.starts_with(char::is_whitespace)
                                    {
                                        last.text.push(' ');
                                    }
                                }
                            }
                            line.words.push(LyricWord {
                                start,
                                end: finish.max(start),
                                text,
                            });
                        } else if let Some(parent) = span_stack.last_mut() {
                            parent.3.push_str(&text);
                        } else {
                            line.plain.push_str(&text);
                        }
                    }
                    SpanRole::Translation => line.translation.push_str(&text),
                    SpanRole::Roman => line.roman.push_str(&text),
                    SpanRole::Background => {
                        // 和声容器自身的直接文本（无逐字子 span 时）。
                        // 与前一个主唱音节之间补一个空格，避免 "line(oh)" 粘连。
                        if !text.trim().is_empty() {
                            if let Some(last) = line.words.last_mut() {
                                if !last.text.ends_with(char::is_whitespace) {
                                    last.text.push(' ');
                                }
                            }
                            if let (Some(start), Some(finish)) = (begin, span_end) {
                                line.words.push(LyricWord {
                                    start,
                                    end: finish.max(start),
                                    text,
                                });
                            } else {
                                line.plain.push_str(&text);
                            }
                        }
                    }
                    SpanRole::Other => {
                        if let Some(parent) = span_stack.last_mut() {
                            parent.3.push_str(&text);
                        } else {
                            line.plain.push_str(&text);
                        }
                    }
                }
            }
            Event::Text(text) if current.is_some() => {
                let Ok(decoded) = text.decode() else {
                    continue;
                };
                let decoded = decoded.into_owned();
                if let Some(top) = span_stack.last_mut() {
                    top.3.push_str(&decoded);
                } else if let Some(line) = current.as_mut() {
                    // p 直接文本：有逐字时视为词间空白并入上一个音节，否则是整行文本
                    match line.words.last_mut() {
                        Some(last) if decoded.trim().is_empty() => last.text.push_str(&decoded),
                        _ => line.plain.push_str(&decoded),
                    }
                }
            }
            Event::CData(data) if current.is_some() => {
                let decoded = String::from_utf8_lossy(&data).into_owned();
                if let Some(top) = span_stack.last_mut() {
                    top.3.push_str(&decoded);
                } else if let Some(line) = current.as_mut() {
                    line.plain.push_str(&decoded);
                }
            }
            // quick-xml 0.41 起 `&amp;` / `&#x27;` 等实体引用是独立事件，不再内联在 Text 里
            Event::GeneralRef(entity) if current.is_some() => {
                let decoded = if entity.is_char_ref() {
                    entity.resolve_char_ref().ok().flatten().map(String::from)
                } else {
                    entity
                        .decode()
                        .ok()
                        .and_then(|name| quick_xml::escape::resolve_predefined_entity(&name))
                        .map(str::to_owned)
                };
                let Some(decoded) = decoded else {
                    continue;
                };
                if let Some(top) = span_stack.last_mut() {
                    top.3.push_str(&decoded);
                } else if let Some(line) = current.as_mut() {
                    match line.words.last_mut() {
                        Some(last) if line.plain.is_empty() => last.text.push_str(&decoded),
                        _ => line.plain.push_str(&decoded),
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_tt {
        return Vec::new();
    }

    lines.sort_by(|a, b| {
        a.time
            .partial_cmp(&b.time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    lines.dedup_by(|a, b| (a.time - b.time).abs() < 0.01 && a.text == b.text);
    clamp_lyrics(lines)
}

fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|byte| *byte == b':') {
        Some(index) => &name[index + 1..],
        None => name,
    }
}

fn attr_value(start: &BytesStart<'_>, wanted: &[u8]) -> Option<String> {
    start.attributes().flatten().find_map(|attr| {
        (local_name(attr.key.as_ref()) == wanted)
            .then(|| {
                attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(|value| value.into_owned())
            })
            .flatten()
    })
}

fn time_attrs(start: &BytesStart<'_>) -> (Option<f64>, Option<f64>) {
    let begin = attr_value(start, b"begin").and_then(|value| parse_ttml_time(&value));
    let end = attr_value(start, b"end").and_then(|value| parse_ttml_time(&value));
    (begin, end)
}

fn span_role(start: &BytesStart<'_>) -> SpanRole {
    match attr_value(start, b"role").as_deref() {
        Some("x-translation") => SpanRole::Translation,
        Some("x-roman") => SpanRole::Roman,
        Some("x-bg") => SpanRole::Background,
        Some(_) => SpanRole::Other,
        None => SpanRole::Word,
    }
}

/// TTML 时间表达式 → 秒。支持 `hh:mm:ss.fff`、`mm:ss.fff`、`ss.fff`、
/// 以及 `123ms` / `1.5s` / `2m` / `1h` 的 offset-time 形式；帧/tick 形式不支持。
pub(crate) fn parse_ttml_time(raw: &str) -> Option<f64> {
    let value = raw.trim();
    if value.is_empty() {
        return None;
    }

    if value.contains(':') {
        let parts = value.split(':').collect::<Vec<_>>();
        let (hours, minutes, seconds) = match parts.as_slice() {
            [minutes, seconds] => (
                0.0,
                minutes.parse::<f64>().ok()?,
                seconds.parse::<f64>().ok()?,
            ),
            [hours, minutes, seconds] => (
                hours.parse::<f64>().ok()?,
                minutes.parse::<f64>().ok()?,
                seconds.parse::<f64>().ok()?,
            ),
            _ => return None,
        };
        if hours < 0.0 || minutes < 0.0 || seconds < 0.0 {
            return None;
        }
        let total = hours * 3600.0 + minutes * 60.0 + seconds;
        return total.is_finite().then_some(total);
    }

    let (number, unit) = match value.find(|ch: char| ch.is_ascii_alphabetic()) {
        Some(index) => value.split_at(index),
        None => (value, "s"),
    };
    let number = number.trim().parse::<f64>().ok()?;
    let seconds = match unit.trim() {
        "ms" => number / 1000.0,
        "s" => number,
        "m" => number * 60.0,
        "h" => number * 3600.0,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:amll="http://www.example.com/ns/amll" xmlns:itunes="http://music.apple.com/lyric-ttml-internal">
  <head><metadata><ttm:agent type="person" xml:id="v1"/><amll:meta key="ncmMusicId" value="123"/></metadata></head>
  <body dur="00:10.000">
    <div begin="00:00.000" end="00:10.000">
      <p begin="00:01.000" end="00:03.000" ttm:agent="v1" itunes:key="L1"><span begin="00:01.000" end="00:01.500">Hel</span><span begin="00:01.500" end="00:02.000">lo </span><span begin="00:02.000" end="00:03.000">world</span><span ttm:role="x-translation" xml:lang="zh-CN">你好，世界</span><span ttm:role="x-roman">ha-ro wa-ru-do</span></p>
      <p begin="00:04.000" end="00:06.000" ttm:agent="v1" itunes:key="L2"><span begin="4s" end="4500ms">Second</span> <span begin="00:04.500" end="00:06.000">line</span><span ttm:role="x-bg"><span begin="00:05.000" end="00:06.000">(oh)</span></span></p>
      <p begin="00:07.000" end="00:08.000">Plain line &amp; text</p>
    </div>
  </body>
</tt>"#;

    #[test]
    fn parses_words_translation_and_roman() {
        let lines = parse_ttml_lyrics(SAMPLE);
        assert_eq!(lines.len(), 3);

        let first = &lines[0];
        assert!((first.time - 1.0).abs() < 1e-9);
        assert_eq!(first.end, Some(3.0));
        assert_eq!(first.text, "Hello world");
        assert_eq!(first.translation.as_deref(), Some("你好，世界"));
        assert_eq!(first.roman.as_deref(), Some("ha-ro wa-ru-do"));
        let words = first.words.as_ref().expect("words");
        assert_eq!(words.len(), 3);
        assert_eq!(words[1].text, "lo ");
        assert!((words[1].start - 1.5).abs() < 1e-9 && (words[1].end - 2.0).abs() < 1e-9);

        let second = &lines[1];
        assert_eq!(second.text, "Second line (oh)");
        let words = second.words.as_ref().expect("words");
        // p 内的词间空白并入上一个音节；和声音节并入本行
        assert_eq!(words[0].text, "Second ");
        assert_eq!(words.last().map(|w| w.text.as_str()), Some("(oh)"));
        assert!((words[0].start - 4.0).abs() < 1e-9 && (words[0].end - 4.5).abs() < 1e-9);

        let third = &lines[2];
        assert_eq!(third.text, "Plain line & text");
        assert!(third.words.is_none());
        assert!(third.translation.is_none());
    }

    #[test]
    fn rejects_non_ttml_and_bad_xml() {
        assert!(parse_ttml_lyrics("[00:01.00]not ttml").is_empty());
        assert!(parse_ttml_lyrics("<html><body><p begin=\"1s\">x</p></body></html>").is_empty());
        assert!(parse_ttml_lyrics("<tt><body><p begin=\"1s\"><span>broken").is_empty());
    }

    #[test]
    fn parses_time_expressions() {
        assert_eq!(parse_ttml_time("00:01.250"), Some(1.25));
        assert_eq!(parse_ttml_time("01:02:03.5"), Some(3723.5));
        assert_eq!(parse_ttml_time("1500ms"), Some(1.5));
        assert_eq!(parse_ttml_time("2.5s"), Some(2.5));
        assert_eq!(parse_ttml_time("7"), Some(7.0));
        assert_eq!(parse_ttml_time("00:00:01:12"), None);
        assert_eq!(parse_ttml_time("-1s"), None);
        assert_eq!(parse_ttml_time("abc"), None);
    }

    #[test]
    fn line_level_ttml_without_spans_still_sorted() {
        let text = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>
            <p begin="00:05.000" end="00:06.000">B</p>
            <p begin="00:02.000" end="00:03.000">A</p>
        </div></body></tt>"#;
        let lines = parse_ttml_lyrics(text);
        assert_eq!(
            lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["A", "B"]
        );
    }
}
