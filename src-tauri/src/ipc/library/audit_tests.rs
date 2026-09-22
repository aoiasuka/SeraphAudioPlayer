//! 2026-09-22 歌词解析模型审计的回归测试（缺陷先写测试再修）。

use super::lyrics::*;
use super::online_lyrics::{attach_lyric_tracks, parse_qq_translation_text};
use super::prelude::*;
use super::ttml::parse_ttml_lyrics;

#[test]
fn yrc_text_containing_angle_bracket_is_not_mistaken_for_krc() {
    // 歌词文本里有 `<3`：此前只要文本含 `<` 就先走 KRC 解析，KRC 找不到 `<a,b,c>` 标签时
    // 把整行（含 `(start,dur,0)` 标签）当行级文本返回，YRC 逐字全部丢失
    let text =
        "[1200,800](1200,200,0)I (1400,200,0)<3 (1600,400,0)you\n[2500,500](2500,500,0)world";
    let lyrics = parse_lyrics_bytes(text.as_bytes());
    assert_eq!(lyrics.len(), 2);
    assert_eq!(lyrics[0].text, "I <3 you");
    assert_eq!(lyrics[0].words.as_ref().map(Vec::len), Some(3));
    assert_eq!(lyrics[1].words.as_ref().map(Vec::len), Some(1));
}

#[test]
fn clamp_caps_words_translations_and_roman_too() {
    let mut line = LyricLine::new(0, "字".repeat(MAX_LYRIC_LINE_CHARS + 100));
    line.words = Some(
        (0..(MAX_LYRIC_LINE_CHARS + 100) as u64)
            .map(|i| LyricWord::new(i, Some(i + 1), "字"))
            .collect(),
    );
    line.translations = (0..10)
        .map(|_| LyricText::new("译".repeat(MAX_LYRIC_LINE_CHARS + 5)))
        .collect();
    line.roman = Some(LyricText::new("r".repeat(MAX_LYRIC_LINE_CHARS + 5)));
    let out = clamp_lyrics(vec![line]);
    let line = &out[0];
    let words = line.words.as_ref().unwrap();
    let joined: usize = words.iter().map(|w| w.text.chars().count()).sum();
    assert!(joined <= MAX_LYRIC_LINE_CHARS, "words joined {joined}");
    assert!(line.translations.len() <= MAX_LYRIC_TRANSLATIONS);
    assert!(line
        .translations
        .iter()
        .all(|t| t.text.chars().count() <= MAX_LYRIC_LINE_CHARS));
    assert!(line.roman.as_ref().unwrap().text.chars().count() <= MAX_LYRIC_LINE_CHARS);
}

#[test]
fn entities_are_decoded_once_and_numeric_entities_are_supported() {
    // 链式 replace 会把 `&amp;lt;` 解两次成 `<`；单趟解码只解一层
    assert_eq!(clean_lyric_text("a &amp;lt; b"), Some("a &lt; b".into()));
    assert_eq!(
        clean_lyric_text("Tom &amp; Jerry"),
        Some("Tom & Jerry".into())
    );
    // QQ 网页端返回的 `&#39;` / `&#8217;` 一类数字实体
    assert_eq!(
        clean_lyric_text("don&#39;t &#8217;"),
        Some("don't \u{2019}".into())
    );
    assert_eq!(clean_lyric_text("x&#x27;y"), Some("x'y".into()));
    // 不完整实体原样保留
    assert_eq!(
        clean_lyric_text("a & b &foo; &#zz;"),
        Some("a & b &foo; &#zz;".into())
    );
    // `<br>` 变体、\0、全角空白与制表符仍归一
    assert_eq!(
        clean_lyric_text("a<br/>b<br />c<br>d\0\u{3000}\te"),
        Some("a b c d e".into())
    );
    // 行内时间标签剥离，非时间标签保留
    assert_eq!(
        clean_lyric_text("a<00:01.00>b[live]"),
        Some("ab[live]".into())
    );
    assert_eq!(clean_lyric_segment(" a  b "), " a b ");
    assert_eq!(clean_lyric_segment("plain"), "plain");
}

#[test]
fn lrc_time_token_keeps_semantics() {
    let cases = [
        ("00:01.20", 1.2),
        ("00:01,20", 1.2),
        ("1:02.5", 62.5),
        ("00:29:26", 29.26),
        ("01:02:03.5", 3723.5),
        ("1234,567", 1.234),
        ("  00:01.000 ", 1.0),
    ];
    for (token, expected) in cases {
        let got = parse_lrc_time_token(token).unwrap_or_else(|| panic!("{token}"));
        assert!((got - expected).abs() < 1e-9, "{token}: {got}");
    }
    for bad in ["", "a:b", "00:-1", "1:2:3:4", "1234", "br", "00:01.0e5x"] {
        assert!(parse_lrc_time_token(bad).is_none(), "{bad}");
    }
    // 超长 token 不 panic
    let long = format!("00:{}", "1".repeat(200));
    let _ = parse_lrc_time_token(&long);
}

#[test]
fn numeric_tuple_helpers_keep_semantics() {
    assert_eq!(parse_numeric_tuple("1,2", 2), Some([1, 2, 0]));
    assert_eq!(parse_numeric_tuple("1,2,3", 3), Some([1, 2, 3]));
    assert_eq!(parse_numeric_tuple("1,2", 3), None);
    assert_eq!(parse_numeric_tuple("1,,2", 3), None);
    assert_eq!(parse_numeric_tuple("99999999999999999999,1", 2), None);
    assert!(is_numeric_tuple("1,2,3", 3));
    assert!(!is_numeric_tuple("1,2,3,", 3));
    assert!(!is_numeric_tuple("a,2,3", 3));
    assert!(contains_tuple_marker("x(1,2,3)y", '(', ')', 3));
    assert!(!contains_tuple_marker("x(1,2)y", '(', ')', 3));
    assert!(!contains_tuple_marker("x(1,2", '(', ')', 2));
}

#[test]
fn line_terminators_are_split_like_before() {
    let text = "[00:01.00]a\r\n[00:02.00]b\r[00:03.00]c\u{2028}[00:04.00]d\u{2029}[00:05.00]e\n";
    let lines = parse_lyrics_text(text);
    assert_eq!(
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c", "d", "e"]
    );
    let plain = parse_lyrics_text("x\u{2028}y");
    assert_eq!(plain.len(), 2);
}

#[test]
fn decode_lyric_bytes_borrows_utf8_and_sniffs_utf16_from_the_head() {
    let utf8 = "[00:01.00]中文".as_bytes();
    assert!(matches!(
        decode_lyric_bytes(utf8),
        std::borrow::Cow::Borrowed(_)
    ));
    let long = format!("[00:01.00]{}", "hello ".repeat(4000));
    let bytes = long
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    assert_eq!(decode_lyric_bytes(&bytes).as_ref(), long);
    let be = long
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect::<Vec<_>>();
    assert_eq!(decode_lyric_bytes(&be).as_ref(), long);
}

#[test]
fn extract_qrc_lyric_content_borrows() {
    let text = r#"<Lyric_1 LyricType="1" LyricContent="[1,2]a(1,2)"/>"#;
    assert_eq!(extract_qrc_lyric_content(text), Some("[1,2]a(1,2)"));
    assert_eq!(extract_qrc_lyric_content("[00:01.00]x"), None);
}

#[test]
fn qrc_container_with_unescaped_quote_in_metadata_still_yields_words() {
    // 真实 QQ 缓存：`[al:THE BEST "Blue"]` 里的直引号曾把 LyricContent 截断在第三行
    let text = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<QrcInfos><LyricInfo LyricCount=\"1\"><Lyric_1 LyricType=\"1\" LyricContent=\"[ti:red moon]\n[al:THE BEST \"Blue\"]\n[1000,2000]he(1000,500)llo(1500,500)\n\"/></LyricInfo></QrcInfos>";
    let lyrics = parse_lyrics_bytes(text.as_bytes());
    assert_eq!(lyrics.len(), 1);
    assert_eq!(lyrics[0].text, "hello");
    assert_eq!(lyrics[0].words.as_ref().map(Vec::len), Some(2));
}

#[test]
fn ttml_whitespace_only_spans_merge_into_previous_word() {
    // 空白音节此前被 filter 掉，音节拼接变成 "Helloworld"，KaraokeLine 渲染丢空格
    let text = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="00:01.000" end="00:03.000"><span begin="00:01.000" end="00:01.500">Hello</span><span begin="00:01.500" end="00:01.600"> </span><span begin="00:01.600" end="00:03.000">world</span></p></div></body></tt>"#;
    let lines = parse_ttml_lyrics(text);
    let words = lines[0].words.as_ref().unwrap();
    assert_eq!(lines[0].text, "Hello world");
    assert_eq!(
        words.iter().map(|w| w.text.as_str()).collect::<String>(),
        "Hello world"
    );
    assert_eq!(words[0].end_ms, Some(1600), "空白音节的时间并入前一音节");
    let text = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="1s" end="2s"><span begin="1s" end="1.1s"> </span><span begin="1.1s" end="2s">x</span></p></div></body></tt>"#;
    let lines = parse_ttml_lyrics(text);
    assert_eq!(lines[0].words.as_ref().unwrap().len(), 1);
}

#[test]
fn ttml_br_inside_p_becomes_a_space() {
    let text = r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="1s" end="2s">line one<br/>line two</p><p begin="3s" end="4s"><span begin="3s" end="3.5s">a</span><br/><span begin="3.5s" end="4s">b</span></p></div></body></tt>"#;
    let lines = parse_ttml_lyrics(text);
    assert_eq!(lines[0].text, "line one line two");
    assert_eq!(lines[1].text, "a b");
}

#[test]
fn roman_track_keeps_its_word_timing_and_qq_notice_is_dropped() {
    let mut original = LyricLine::new(1000, "こんにちは");
    original.words = Some(vec![LyricWord::new(1000, Some(2000), "こんにちは")]);
    let mut roman = LyricLine::new(1000, "kon ni chi wa");
    roman.words = Some(vec![
        LyricWord::new(1000, Some(1500), "kon "),
        LyricWord::new(1500, Some(2000), "ni chi wa"),
    ]);
    let merged = attach_lyric_tracks(vec![original], Vec::new(), vec![roman]);
    let roman = merged[0].roman.as_ref().expect("roman");
    assert_eq!(roman.text, "kon ni chi wa");
    assert_eq!(roman.words.as_ref().map(Vec::len), Some(2));

    let trans = parse_qq_translation_text(
        "[kana:1た]\n[00:00.77]QQ音乐享有本翻译作品的著作权\n[00:07.20]//\n[00:16.93]藏匿在左手心的夙愿\n",
    );
    assert_eq!(
        trans.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["藏匿在左手心的夙愿"]
    );
}

#[test]
fn document_deserializer_rejects_scalars_and_streams_both_shapes() {
    // 流式反序列化（不再经 untagged 缓冲）：两种形态都直接读；标量报错而不是 panic
    let modern: LyricDocument =
        serde_json::from_str(r#"{"schema":2,"lines":[{"startMs":1,"text":"a"}]}"#).unwrap();
    assert_eq!(modern.lines[0].text, "a");
    let legacy: LyricDocument = serde_json::from_str(r#"[{"time":1.5,"text":"b"}]"#).unwrap();
    assert_eq!(legacy.lines[0].start_ms, 1500);
    assert_eq!(legacy.source.kind, LyricSourceKind::Legacy);
    assert!(serde_json::from_str::<LyricDocument>("42").is_err());
    assert!(serde_json::from_str::<LyricDocument>("\"x\"").is_err());
    let tolerant: LyricDocument =
        serde_json::from_str(r#"{"schema":2,"future":1,"lines":[]}"#).unwrap();
    assert!(tolerant.is_empty());
}

#[test]
fn display_shift_saturates_instead_of_overflowing() {
    let mut doc =
        LyricDocument::from_lines(vec![LyricLine::new(u64::MAX, "x")], LyricSource::default())
            .with_offset(i32::MIN);
    super::display::project_document_with(
        &mut doc,
        LyricsDisplayOptions {
            ignore_file_offset: true,
            ..LyricsDisplayOptions::DEFAULT
        },
    );
    assert!(doc.lines[0].start_ms > 0);
}
