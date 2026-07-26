//! WAV（RIFF）内嵌 ID3v2 封面兜底提取。
//!
//! 病灶：部分打标签工具在 ID3v2.4 里把帧大小写成普通 u32（规范要求
//! syncsafe 整数）。帧内容超过 127 字节时两种编码解读不同，lofty 按规范
//! 解析会把 APIC 图片数据拦腰截断——用户库中 WAV 封面「只显示上半张 /
//! 空白」的根因。文字帧（标题/艺术家等）几乎都小于 128 字节、两种解读
//! 一致不受影响，因此只兜底封面。
//!
//! 处理：手动遍历 RIFF 顶层 chunk 找 `id3 `/`ID3 `（真实病例中该 chunk
//! 位于 RIFF 头声明大小之外，故按实际文件大小遍历），再以 mutagen 同款
//! 启发式消解 v2.4 帧大小的 syncsafe/u32 歧义后提取 APIC。

use super::prelude::*;
use std::io::{Seek as _, SeekFrom};

/// 防呆：ID3 chunk 大小上限（与 dsd_tags 同值）。
const MAX_WAV_ID3_BYTES: u64 = 32 * 1024 * 1024;

/// lofty 提取的 WAV 封面缺失或疑似截断时，从 id3 chunk 重新解析 APIC；
/// 重解析不出结果则保留原值。
pub(crate) fn wav_cover_fallback(path: &Path, current: Option<CoverArt>) -> Option<CoverArt> {
    if cover_looks_complete(current.as_ref()) {
        return current;
    }
    extract_wav_id3_bytes(path)
        .and_then(|bytes| apic_from_id3(&bytes))
        .or(current)
}

/// JPEG/PNG 有固定结束标记，可检测截断；其余格式无从判断，视为完整。
fn cover_looks_complete(cover: Option<&CoverArt>) -> bool {
    let Some(cover) = cover else {
        return false;
    };
    match cover.ext {
        "jpg" => cover.data.ends_with(&[0xff, 0xd9]),
        // IEND chunk 无数据，其 CRC 是固定值
        "png" => cover.data.ends_with(&[0xae, 0x42, 0x60, 0x82]),
        _ => true,
    }
}

/// 已落盘的封面文件是否为截断的 JPEG/PNG（修复前的 WAV 病灶会把半张图
/// 写进 covers 目录）。读不到文件、在线封面或其他格式一律 false，
/// 不触发重提取。
pub(crate) fn cover_file_looks_truncated(cover: &str) -> bool {
    if cover.is_empty() || cover.starts_with("http") {
        return false;
    }
    let path = Path::new(cover);
    let ext = match path.extension().and_then(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case("jpg") => "jpg",
        Some(value) if value.eq_ignore_ascii_case("png") => "png",
        _ => return false,
    };
    let Ok(data) = fs::read(path) else {
        return false;
    };
    !cover_looks_complete(Some(&CoverArt { data, ext }))
}

/// 遍历 RIFF 顶层 chunk 定位 id3 chunk 并读出字节。
/// 不信任 RIFF 头声明的总大小——病例文件的 id3 chunk 追加在其之外。
fn extract_wav_id3_bytes(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut header = [0_u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }
    let real_len = file.metadata().ok()?.len();
    let mut pos = 12_u64;
    // 顶层 chunk 数量有限（畸形文件防死循环）
    for _ in 0..4096 {
        if pos.checked_add(8)? > real_len {
            return None;
        }
        file.seek(SeekFrom::Start(pos)).ok()?;
        let mut chunk_header = [0_u8; 8];
        file.read_exact(&mut chunk_header).ok()?;
        let size = u64::from(u32::from_le_bytes(chunk_header[4..8].try_into().ok()?));
        if chunk_header[0..4].eq_ignore_ascii_case(b"id3 ") {
            if size == 0 || size > MAX_WAV_ID3_BYTES || pos + 8 + size > real_len {
                return None;
            }
            let mut bytes = vec![0_u8; size as usize];
            file.read_exact(&mut bytes).ok()?;
            return bytes.starts_with(b"ID3").then_some(bytes);
        }
        pos = pos.checked_add(8 + size + (size & 1))?;
    }
    None
}

/// 从 ID3v2.3/2.4 字节块解析 APIC 封面：优先 CoverFront，否则第一张
/// 非空图（与 lofty 路径同语义）。
fn apic_from_id3(bytes: &[u8]) -> Option<CoverArt> {
    if bytes.len() < 10 || &bytes[0..3] != b"ID3" {
        return None;
    }
    let major = bytes[3];
    if !(3..=4).contains(&major) {
        return None;
    }
    // unsynchronisation / extended header / experimental / footer：
    // 罕见且规范文件由 lofty 正确处理，兜底路径不冒险。
    if bytes[5] & 0b1111_0000 != 0 {
        return None;
    }
    let tag_size = syncsafe_u32(bytes[6..10].try_into().ok()?)? as usize;
    let body = &bytes[10..bytes.len().min(10 + tag_size)];

    let mut pos = 0_usize;
    let mut fallback: Option<CoverArt> = None;
    while pos + 10 <= body.len() {
        let id = &body[pos..pos + 4];
        if !is_frame_id(id) {
            break; // padding 或垃圾数据
        }
        let raw: [u8; 4] = body[pos + 4..pos + 8].try_into().ok()?;
        let size = if major == 4 {
            resolve_v24_frame_size(raw, body, pos)?
        } else {
            u32::from_be_bytes(raw) as usize
        };
        let content_end = pos.checked_add(10)?.checked_add(size)?;
        if content_end > body.len() {
            break;
        }
        // 帧 flags 非零（压缩/加密/unsync 等）的帧不处理
        if id == b"APIC" && body[pos + 8..pos + 10] == [0, 0] {
            if let Some((pic_type, data)) = parse_apic_content(&body[pos + 10..content_end]) {
                if let Some(ext) = (!data.is_empty())
                    .then(|| cover_image_extension(None, data))
                    .flatten()
                {
                    let art = CoverArt {
                        data: data.to_vec(),
                        ext,
                    };
                    // 3 = CoverFront
                    if pic_type == 3 {
                        return Some(art);
                    }
                    if fallback.is_none() {
                        fallback = Some(art);
                    }
                }
            }
        }
        pos = content_end;
    }
    fallback
}

fn is_frame_id(id: &[u8]) -> bool {
    id.len() == 4
        && id
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// v2.4 帧大小规范为 syncsafe，但大量工具错写普通 u32（帧内容超过 127
/// 字节时两种解读不同）。启发式（同 mutagen）：
/// 1. 任一字节最高位为 1 → 必非 syncsafe，按 u32；
/// 2. 两种解读一致 → 无歧义；
/// 3. 否则看帧边界落点是否合法（正好到 body 末尾、padding 或下一个合法
///    帧 ID）：syncsafe 合法优先（守规范），落点非法而 u32 落点合法才改判。
fn resolve_v24_frame_size(raw: [u8; 4], body: &[u8], pos: usize) -> Option<usize> {
    let as_u32 = u32::from_be_bytes(raw) as usize;
    let Some(as_syncsafe) = syncsafe_u32(raw) else {
        return Some(as_u32);
    };
    let as_syncsafe = as_syncsafe as usize;
    if as_syncsafe == as_u32 {
        return Some(as_syncsafe);
    }
    let base = pos.checked_add(10)?;
    if frame_boundary_plausible(body, base.checked_add(as_syncsafe)?) {
        return Some(as_syncsafe);
    }
    if frame_boundary_plausible(body, base.checked_add(as_u32)?) {
        return Some(as_u32);
    }
    Some(as_syncsafe)
}

/// 帧边界落点合法：正好到 body 末尾、落在 padding（\0）上、或紧跟一个
/// 合法帧 ID。
fn frame_boundary_plausible(body: &[u8], next: usize) -> bool {
    if next == body.len() {
        return true;
    }
    let Some(rest) = body.get(next..) else {
        return false;
    };
    rest[0] == 0 || rest.len() >= 4 && is_frame_id(&rest[..4])
}

/// syncsafe 整数：4 字节各取低 7 位；任一最高位为 1 即非法。
fn syncsafe_u32(bytes: [u8; 4]) -> Option<u32> {
    if bytes.iter().any(|b| b & 0x80 != 0) {
        return None;
    }
    Some(
        (u32::from(bytes[0]) << 21)
            | (u32::from(bytes[1]) << 14)
            | (u32::from(bytes[2]) << 7)
            | u32::from(bytes[3]),
    )
}

/// APIC 内容：encoding(1B) + MIME(latin1，\0 结尾) + picture type(1B)
/// + description(按 encoding 的终结符) + 图片数据。
///
/// 返回 (picture_type, 图片数据)；MIME 不采信，扩展名由魔数推断。
fn parse_apic_content(content: &[u8]) -> Option<(u8, &[u8])> {
    let (&encoding, rest) = content.split_first()?;
    let mime_end = rest.iter().position(|&b| b == 0)?;
    let rest = &rest[mime_end + 1..];
    let (&pic_type, rest) = rest.split_first()?;
    let data = match encoding {
        // ISO-8859-1 / UTF-8：单字节 \0 终结
        0 | 3 => {
            let end = rest.iter().position(|&b| b == 0)?;
            &rest[end + 1..]
        }
        // UTF-16（带 BOM）/ UTF-16BE：\0\0 终结，按 2 字节步进扫描
        1 | 2 => {
            let end = rest
                .chunks_exact(2)
                .position(|pair| pair == [0, 0])
                .map(|idx| idx * 2)?;
            &rest[end + 2..]
        }
        _ => return None,
    };
    Some((pic_type, data))
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn u32_frame_size_detected_by_high_bit() {
        // 0x0000c34e：第三字节高位为 1，非法 syncsafe → 直接按 u32
        assert_eq!(
            resolve_v24_frame_size([0x00, 0x00, 0xc3, 0x4e], &[], 0),
            Some(0xc34e)
        );
    }

    #[test]
    fn small_frame_size_has_no_ambiguity() {
        assert_eq!(resolve_v24_frame_size([0, 0, 0, 0x10], &[], 0), Some(16));
    }

    #[test]
    fn ambiguous_size_resolved_by_boundary_alignment() {
        // raw = [0,0,1,0x2c]：syncsafe=172，u32=300。
        // body：帧头 10 + 内容 300 正好到末尾 → u32 落点合法、syncsafe 落点
        // 是非零垃圾字节 → 改判 u32（复刻 weiyi 病例：字节全部低位但仍是 u32）。
        let mut body = vec![0_u8; 310];
        body[10 + 172] = 0xac; // syncsafe 落点：垃圾（非 padding、非帧 ID）
        assert_eq!(
            resolve_v24_frame_size([0, 0, 1, 0x2c], &body, 0),
            Some(300)
        );
    }

    #[test]
    fn spec_compliant_size_wins_when_boundary_valid() {
        // syncsafe 落点正好是下一个合法帧 ID → 守规范
        let mut body = vec![0xff_u8; 200];
        body[10 + 172..10 + 176].copy_from_slice(b"TIT2");
        assert_eq!(
            resolve_v24_frame_size([0, 0, 1, 0x2c], &body, 0),
            Some(172)
        );
    }

    #[test]
    fn detects_truncated_and_complete_covers() {
        let complete = CoverArt {
            data: vec![0xff, 0xd8, 0xff, 0xe0, 0xff, 0xd9],
            ext: "jpg",
        };
        let truncated = CoverArt {
            data: vec![0xff, 0xd8, 0xff, 0xe0, 0xaa],
            ext: "jpg",
        };
        assert!(cover_looks_complete(Some(&complete)));
        assert!(!cover_looks_complete(Some(&truncated)));
        assert!(!cover_looks_complete(None));
    }

    #[test]
    fn apic_with_utf16_description_parses() {
        let mut content = vec![1_u8]; // UTF-16 编码
        content.extend_from_slice(b"image/jpeg\0");
        content.push(3);
        content.extend_from_slice(&[0xff, 0xfe, 0x41, 0x00, 0x00, 0x00]); // BOM + "A" + 终结
        content.extend_from_slice(&[0xff, 0xd8, 0xff, 0xd9]);
        let (pic_type, data) = parse_apic_content(&content).expect("apic parse");
        assert_eq!(pic_type, 3);
        assert_eq!(data, &[0xff, 0xd8, 0xff, 0xd9]);
    }
}
