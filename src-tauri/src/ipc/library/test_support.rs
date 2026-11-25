//! library 模块测试共用的夹具与辅助函数。
//!
//! LIB-04：这些函数此前带着 `#[cfg(test)]` 散落在 `media_library.rs` 的生产代码中间
//! （含 40 行的 DSF 夹具生成器），现在集中在这里，经 `prelude` 汇入各测试模块。
#![cfg(test)]

use super::prelude::*;
pub(crate) use super::storage::{backup_corrupt_file, write_json_atomic};

/// 唯一的临时路径（不创建文件）。
pub(crate) fn temp_audio_path(prefix: &str, extension: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}-{nanos}.{extension}"))
}

/// 写一个 1 秒、双声道、DSD64 的最小 DSF 文件。
pub(crate) fn write_test_dsf(path: &Path) {
    use std::io::Write;

    let channels = 2_u32;
    let dsd_rate = 2_822_400_u32;
    let sample_count = dsd_rate as u64;
    let block_size_per_channel = 8_u32;
    // 审2：数据体必须与 sample_count 自洽（每声道 sample_count/8 字节）——
    // 解码器现在会把声明时长与文件真实大小交叉校验，虚大的 sample_count
    // 不再被信任（此前夹具只写 16 字节数据却声称 1 秒时长）。
    let bytes_per_channel = sample_count / 8;
    let data_len = channels as u64 * bytes_per_channel;
    let file_size = 28_u64 + 52 + 12 + data_len;

    let mut file = fs::File::create(path).expect("create dsf");
    file.write_all(b"DSD ").unwrap();
    file.write_all(&28_u64.to_le_bytes()).unwrap();
    file.write_all(&file_size.to_le_bytes()).unwrap();
    file.write_all(&0_u64.to_le_bytes()).unwrap();

    file.write_all(b"fmt ").unwrap();
    file.write_all(&52_u64.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();
    file.write_all(&2_u32.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&dsd_rate.to_le_bytes()).unwrap();
    file.write_all(&1_u32.to_le_bytes()).unwrap();
    file.write_all(&sample_count.to_le_bytes()).unwrap();
    file.write_all(&block_size_per_channel.to_le_bytes())
        .unwrap();
    file.write_all(&0_u32.to_le_bytes()).unwrap();

    file.write_all(b"data").unwrap();
    file.write_all(&(12_u64 + data_len).to_le_bytes()).unwrap();
    // 0x69 = 01101001，DSD 静音位型
    let body = vec![0x69_u8; data_len as usize];
    file.write_all(&body).unwrap();
}

pub(crate) fn is_dsd_file(path: &Path) -> bool {
    if let Some(format) = audio_format_from_magic(path) {
        return is_dsd_format(format);
    }

    path.extension()
        .and_then(|value| value.to_str())
        .and_then(audio_format_from_extension)
        .is_some_and(is_dsd_format)
}

/// 与导入路径同一套判定（文件头优先于扩展名）得到的格式标签。
pub(crate) fn audio_format_label(path: &Path) -> String {
    audio_format_from_magic(path)
        .or_else(|| {
            path.extension()
                .and_then(|value| value.to_str())
                .and_then(audio_format_from_extension)
        })
        .unwrap_or("AUDIO")
        .to_string()
}

pub(crate) fn parse_audio_metadata(path: &Path) -> ParsedAudioMetadata {
    // v0.5.1：曾是与正式路径重复的旧副本（漏掉 cover 与 DSD ID3 提取，测试
    // 测不到真实链路）——改为直接委托 parse_audio_metadata_with_dsd_hint。
    parse_audio_metadata_with_dsd_hint(path, is_dsd_file(path))
}

/// 旧版单文件曲库（`library-cache.json`）的读取：不存在视为空库，损坏报错。
pub(crate) fn read_tracks_from_file(path: &Path) -> Result<Vec<ImportedTrack>, String> {
    if !path.is_file() {
        return Ok(Vec::new());
    }

    let bytes = fs::read(path)
        .map_err(|err| format!("failed to read library cache {}: {err}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| format!("failed to parse library cache {}: {err}", path.display()))
}

/// 把曲目拆成「歌词清空的主记录」+「track_id → 歌词」映射，供磁盘分离存储。
/// 纯函数，便于测试往返一致性。
pub(crate) fn split_lyrics_for_storage(
    tracks: &[ImportedTrack],
) -> (Vec<ImportedTrack>, HashMap<String, LyricDocument>) {
    let mut lyrics_by_id = HashMap::new();
    let stripped = tracks
        .iter()
        .map(|track| {
            if !track.lyrics.is_empty() {
                lyrics_by_id.insert(track.id.clone(), track.lyrics.clone());
            }
            ImportedTrack {
                lyrics: LyricDocument::EMPTY,
                ..track.clone()
            }
        })
        .collect();
    (stripped, lyrics_by_id)
}

/// 把歌词边车按 track_id 合并回主记录（读盘时用）。
pub(crate) fn merge_lyrics_from_storage(
    mut tracks: Vec<ImportedTrack>,
    lyrics_by_id: &HashMap<String, LyricDocument>,
) -> Vec<ImportedTrack> {
    for track in &mut tracks {
        if let Some(lyrics) = lyrics_by_id.get(&track.id) {
            track.lyrics = lyrics.clone();
        }
    }
    tracks
}
