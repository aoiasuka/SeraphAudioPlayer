//! 歌词解析微基准（合成输入，`#[ignore]`，由 `npm run benchmark:lyrics` 驱动）。
//!
//! 每项输出一行 JSON：`{"case":…,"iters":…,"per_iter_us":…,"bytes":…,"lines":…}`。
//! 合成数据只代表解析器本身的相对耗时，不代表桌面端真实延迟（不含 I/O、IPC）。
//! 运行：`cargo test --release -p seraph-tauri --lib lyrics_bench -- --ignored --nocapture`

use super::lyrics::*;
use super::prelude::*;
use std::collections::BTreeMap;
use std::time::Instant;

const LINES: usize = 300;
const WORDS_PER_LINE: usize = 24;

fn lrc_tag(ms: u64, open: char, close: char) -> String {
    format!(
        "{open}{:02}:{:02}.{:03}{close}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    )
}

fn syllable(index: usize) -> &'static str {
    const POOL: [&str; 12] = [
        "我", "的", "天", "空", "Hello ", "world ", "光", "り", "の", "中", "で", "夢",
    ];
    POOL[index % POOL.len()]
}

pub(crate) fn enhanced_lrc() -> String {
    let mut out = String::from("[ti:bench]\n[ar:seraph]\n[offset:-196]\n");
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        out.push_str(&lrc_tag(start, '[', ']'));
        for word in 0..WORDS_PER_LINE {
            out.push_str(&lrc_tag(start + word as u64 * 100, '<', '>'));
            out.push_str(syllable(word));
        }
        out.push_str(&lrc_tag(start + WORDS_PER_LINE as u64 * 100, '<', '>'));
        out.push('\n');
        // 双语：相邻同起点译文行
        out.push_str(&lrc_tag(start, '[', ']'));
        out.push_str("translation line 译文\n");
    }
    out
}

pub(crate) fn plain_lrc() -> String {
    let mut out = String::from("[ti:bench]\n");
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        out.push_str(&lrc_tag(start, '[', ']'));
        for word in 0..WORDS_PER_LINE {
            out.push_str(syllable(word));
        }
        out.push('\n');
    }
    out
}

pub(crate) fn qrc_container() -> String {
    let mut body = String::from("[ti:bench]&#10;[offset:0]&#10;");
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        body.push_str(&format!("[{start},{}]", WORDS_PER_LINE * 100));
        for word in 0..WORDS_PER_LINE {
            body.push_str(syllable(word));
            body.push_str(&format!("({},100)", start + word as u64 * 100));
        }
        body.push_str("&#10;");
    }
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?><QrcInfos><QrcHeadInfo SaveTime="1" Version="100"/><LyricInfo LyricCount="1"><Lyric_1 LyricType="1" LyricContent="{body}"/></LyricInfo></QrcInfos>"#
    )
}

pub(crate) fn yrc_text() -> String {
    let mut out = String::new();
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        out.push_str(&format!("[{start},{}]", WORDS_PER_LINE * 100));
        for word in 0..WORDS_PER_LINE {
            out.push_str(&format!("({},100,0)", start + word as u64 * 100));
            out.push_str(syllable(word));
        }
        out.push('\n');
    }
    out
}

pub(crate) fn krc_text() -> String {
    let translations = (0..LINES)
        .map(|_| r#"["translation line"]"#)
        .collect::<Vec<_>>()
        .join(",");
    let language = BASE64_STANDARD.encode(format!(
        r#"{{"content":[{{"type":1,"language":0,"lyricContent":[{translations}]}}]}}"#
    ));
    let mut out = format!("[id:$00000000]\n[ar:seraph]\n[language:{language}]\n");
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        out.push_str(&format!("[{start},{}]", WORDS_PER_LINE * 100));
        for word in 0..WORDS_PER_LINE {
            out.push_str(&format!("<{},100,0>", word as u64 * 100));
            out.push_str(syllable(word));
        }
        out.push('\n');
    }
    out
}

pub(crate) fn ttml_text() -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?><tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata" xmlns:amll="http://www.example.com/ns/amll"><head><metadata><ttm:agent type="person" xml:id="v1"/><amll:meta key="musicName" value="bench"/></metadata></head><body><div>"#,
    );
    for line in 0..LINES {
        let start = 1000 + line as u64 * 3000;
        let end = start + WORDS_PER_LINE as u64 * 100;
        out.push_str(&format!(
            r#"<p begin="{}" end="{}" ttm:agent="v1">"#,
            lrc_tag(start, ' ', ' ').trim(),
            lrc_tag(end, ' ', ' ').trim()
        ));
        for word in 0..WORDS_PER_LINE {
            let ws = start + word as u64 * 100;
            out.push_str(&format!(
                r#"<span begin="{}" end="{}">{}</span>"#,
                lrc_tag(ws, ' ', ' ').trim(),
                lrc_tag(ws + 100, ' ', ' ').trim(),
                syllable(word)
            ));
        }
        out.push_str(r#"<span ttm:role="x-translation">译文 &amp; translation</span><span ttm:role="x-roman">ro-ma-ji</span></p>"#);
    }
    out.push_str("</div></body></tt>");
    out
}

/// 明文 → zlib → QQ 3DES → hex（云端形态）；本地形态另加魔数 + QMC1 XOR。
pub(crate) fn encrypted_qrc(container: &str) -> (String, Vec<u8>) {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(container.as_bytes()).unwrap();
    let mut compressed = encoder.finish().unwrap();
    while !compressed.len().is_multiple_of(8) {
        compressed.push(0);
    }
    let cipher = super::qq_des::QqTripleDes::new(QRC_KEY);
    let (blocks, _) = compressed.as_chunks_mut::<8>();
    for block in blocks {
        cipher.encrypt_block(block);
    }
    let hex = compressed
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let mut local = QRC_MAGIC_HEADER.to_vec();
    local.extend_from_slice(&compressed);
    qmc1_decrypt(&mut local);
    local[..QRC_MAGIC_HEADER.len()].copy_from_slice(QRC_MAGIC_HEADER);
    (hex, local)
}

pub(crate) fn lyrics_json(docs: usize) -> Vec<u8> {
    let lines = parse_lyrics_text(&enhanced_lrc());
    let doc = LyricDocument::from_lines(lines, LyricSource::of(LyricSourceKind::Sidecar))
        .with_offset(-196);
    let map = (0..docs)
        .map(|index| (format!("track-{index}"), &doc))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_vec(&map).unwrap()
}

fn bench(case: &str, bytes: usize, mut run: impl FnMut() -> usize) {
    // 预热 + 自适应迭代次数（目标 ≥ 0.4 s）
    let lines = run();
    let probe = Instant::now();
    run();
    let single = probe.elapsed().as_secs_f64().max(1e-6);
    let iters = ((0.4 / single).ceil() as usize).clamp(3, 2000);
    let start = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(run());
    }
    let per_iter_us = start.elapsed().as_secs_f64() * 1e6 / iters as f64;
    println!(
        r#"{{"case":"{case}","iters":{iters},"per_iter_us":{per_iter_us:.1},"bytes":{bytes},"lines":{lines}}}"#
    );
}

#[test]
#[ignore]
fn lyrics_parse_benchmark() {
    let enhanced = enhanced_lrc();
    bench("enhanced_lrc", enhanced.len(), || {
        parse_lyrics_bytes(enhanced.as_bytes()).len()
    });
    let plain = plain_lrc();
    bench("plain_lrc", plain.len(), || {
        parse_lyrics_bytes(plain.as_bytes()).len()
    });
    let qrc = qrc_container();
    bench("qrc_container_text", qrc.len(), || {
        parse_lyrics_bytes(qrc.as_bytes()).len()
    });
    let yrc = yrc_text();
    bench("yrc_text", yrc.len(), || {
        parse_lyrics_bytes(yrc.as_bytes()).len()
    });
    let krc = krc_text();
    bench("krc_text", krc.len(), || {
        parse_lyrics_bytes(krc.as_bytes()).len()
    });
    let ttml = ttml_text();
    bench("ttml", ttml.len(), || {
        parse_lyrics_file_bytes_with_offset(Some("ttml"), ttml.as_bytes())
            .0
            .len()
    });
    let (hex, local) = encrypted_qrc(&qrc);
    bench("qrc_cloud_hex_decrypt+parse", hex.len(), || {
        parse_lyrics_bytes(decrypt_qrc_cloud(&hex).unwrap().as_bytes()).len()
    });
    bench("qrc_local_decrypt+parse", local.len(), || {
        parse_lyrics_bytes(&local).len()
    });
    let json = lyrics_json(50);
    bench("lyrics_json_deserialize_50_docs", json.len(), || {
        serde_json::from_slice::<BTreeMap<String, LyricDocument>>(&json)
            .unwrap()
            .len()
    });
    let utf16 = enhanced
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    bench("utf16le_enhanced_lrc", utf16.len(), || {
        parse_lyrics_bytes(&utf16).len()
    });
}
