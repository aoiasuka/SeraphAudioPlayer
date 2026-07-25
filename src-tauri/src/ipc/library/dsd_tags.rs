//! DSF / DFF（DSDIFF）内嵌 ID3v2 标签提取。
//!
//! lofty 0.24 不支持 DSD 容器——`lofty::read_from_path` 对 .dsf/.dff 直接
//! 失败，DSD 曲目的标题/艺术家/专辑/封面此前完全读不到（v0.5.1 用户报告的
//! 封面解析问题主因）。DSF 规范把标准 ID3v2 块放在文件尾、头部 offset 20
//! 的 u64 LE 指针指向它；DFF 是 big-endian IFF 容器，实践中以顶层 `ID3 `
//! chunk 携带标签。这里手动定位 ID3v2 字节块，交给 `id3` crate 解析。

use super::prelude::*;
use id3::TagLike as _;
use std::io::{Cursor, Read as _, Seek as _, SeekFrom};

/// 防呆：ID3 块大小上限（正常带大封面的标签远小于此）。
const MAX_DSD_ID3_BYTES: u64 = 32 * 1024 * 1024;

pub(crate) struct DsdTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<String>,
    pub cover: Option<CoverArt>,
}

/// 从 DSF/DFF 文件解析内嵌 ID3v2 标签；非 DSD 容器或无标签返回 None。
pub(crate) fn dsd_tags_from_path(path: &Path) -> Option<DsdTags> {
    let bytes = extract_dsd_id3_bytes(path)?;
    let tag = id3::Tag::read_from2(Cursor::new(bytes)).ok()?;

    // 封面：优先 CoverFront，否则第一张非空图（与 lofty 路径同语义）
    let mut chosen: Option<&id3::frame::Picture> = None;
    for picture in tag.pictures() {
        if picture.data.is_empty() {
            continue;
        }
        if picture.picture_type == id3::frame::PictureType::CoverFront {
            chosen = Some(picture);
            break;
        }
        if chosen.is_none() {
            chosen = Some(picture);
        }
    }
    let cover = chosen.and_then(|picture| {
        // MIME 走魔数推断（cover_image_extension 的 None 分支），
        // 不信任标签里的自由文本 MIME 字符串
        let ext = cover_image_extension(None, &picture.data)?;
        Some(CoverArt {
            data: picture.data.clone(),
            ext,
        })
    });

    Some(DsdTags {
        title: tag.title().and_then(clean_metadata_text),
        artist: tag.artist().and_then(clean_metadata_text),
        album: tag.album().and_then(clean_metadata_text),
        year: tag
            .year()
            .filter(|year| *year > 0)
            .map(|year| year.to_string())
            .or_else(|| {
                tag.date_recorded()
                    .filter(|ts| ts.year > 0)
                    .map(|ts| ts.year.to_string())
            }),
        cover,
    })
}

/// 定位并读出 DSF/DFF 内嵌的 ID3v2 字节块。
fn extract_dsd_id3_bytes(path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(path).ok()?;
    let mut magic = [0_u8; 4];
    file.read_exact(&mut magic).ok()?;
    match &magic {
        b"DSD " => extract_dsf_id3(&mut file),
        b"FRM8" => extract_dff_id3(&mut file),
        _ => None,
    }
}

/// DSF：头 28 字节中 offset 20 的 u64 LE 是 metadata（ID3v2）绝对偏移，0 = 无标签。
/// 块长度按实际文件大小推算（截断文件宁可解析失败也不越界读）。
fn extract_dsf_id3(file: &mut fs::File) -> Option<Vec<u8>> {
    let mut header_rest = [0_u8; 24];
    file.read_exact(&mut header_rest).ok()?;
    let metadata_ptr = u64::from_le_bytes(header_rest[16..24].try_into().ok()?);
    if metadata_ptr == 0 {
        return None;
    }
    let real_len = file.metadata().ok()?.len();
    if metadata_ptr >= real_len {
        return None;
    }
    let id3_len = real_len - metadata_ptr;
    if id3_len > MAX_DSD_ID3_BYTES {
        return None;
    }
    file.seek(SeekFrom::Start(metadata_ptr)).ok()?;
    let mut bytes = vec![0_u8; id3_len as usize];
    file.read_exact(&mut bytes).ok()?;
    bytes.starts_with(b"ID3").then_some(bytes)
}

/// DFF（DSDIFF，big-endian IFF）：跳过 FRM8 头后遍历顶层 chunk 找 `ID3 `。
/// chunk 布局：[4B id][8B u64 BE size][data]，奇数大小补 1 字节对齐。
fn extract_dff_id3(file: &mut fs::File) -> Option<Vec<u8>> {
    // 已读 "FRM8"；跳过 8 字节容器大小，校验 4 字节 form type "DSD "
    let mut form_header = [0_u8; 12];
    file.read_exact(&mut form_header).ok()?;
    if &form_header[8..12] != b"DSD " {
        return None;
    }
    let real_len = file.metadata().ok()?.len();
    // 顶层 chunk 数量有限（畸形文件防死循环）
    for _ in 0..4096 {
        let mut chunk_header = [0_u8; 12];
        if file.read_exact(&mut chunk_header).is_err() {
            return None;
        }
        let size = u64::from_be_bytes(chunk_header[4..12].try_into().ok()?);
        if &chunk_header[0..4] == b"ID3 " {
            if size == 0 || size > MAX_DSD_ID3_BYTES {
                return None;
            }
            let mut bytes = vec![0_u8; size as usize];
            file.read_exact(&mut bytes).ok()?;
            return bytes.starts_with(b"ID3").then_some(bytes);
        }
        let skip = size.checked_add(size & 1)?;
        let cur = file.stream_position().ok()?;
        let next = cur.checked_add(skip)?;
        if next >= real_len {
            return None;
        }
        file.seek(SeekFrom::Start(next)).ok()?;
    }
    None
}
