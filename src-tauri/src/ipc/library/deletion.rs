use super::prelude::*;
use crate::ipc::{
    bilibili::{acquire_audio_operation, extract_bvid},
    cache::delete_streaming_cache_file,
};

fn is_streaming_track(track: &ImportedTrack) -> bool {
    track.id.starts_with("bilibili-")
        || track
            .source_id
            .as_deref()
            .is_some_and(|id| id.trim().to_ascii_lowercase().starts_with("bv"))
        || track
            .source_url
            .as_deref()
            .is_some_and(|url| url.to_ascii_lowercase().contains("bilibili.com"))
        || track.album == "Bilibili"
}

fn audio_operation_key(track: &ImportedTrack) -> String {
    [
        track.source_id.as_deref(),
        track.source_url.as_deref(),
        Some(track.id.as_str()),
        Path::new(&track.path)
            .file_name()
            .and_then(|name| name.to_str()),
    ]
    .into_iter()
    .flatten()
    .find_map(extract_bvid)
    .unwrap_or_else(|| cache_path_key(&track.path))
    .to_ascii_lowercase()
}

fn cache_path_key(path: &str) -> String {
    Path::new(path)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

/// 调用方全程持有 LIBRARY_LOCK。只接收 ID；用于删文件的路径来自可信曲库快照。
/// 成功项一次提交，失败项保留；占用 guard 直到存储提交后才释放。
pub(super) fn delete_selected_tracks(
    tracks: Vec<ImportedTrack>,
    track_ids: &[String],
    save: impl FnOnce(&[ImportedTrack]) -> Result<(), String>,
) -> Result<DeleteTracksResult, String> {
    let mut requested = HashSet::new();
    let ids = track_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .filter(|id| requested.insert((*id).to_string()))
        .collect::<Vec<_>>();
    let mut result = DeleteTracksResult::default();
    if ids.is_empty() {
        return Ok(result);
    }
    let by_id = tracks
        .iter()
        .map(|track| (track.id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let has_streaming_deletion = tracks
        .iter()
        .any(|track| requested.contains(&track.id) && is_streaming_track(track));
    // 纯本地记录删除不做磁盘扫描；含流媒体时预先固定路径身份，删除后也不改变 key。
    let path_keys = tracks
        .iter()
        .filter(|_| has_streaming_deletion)
        .map(|track| (track.id.as_str(), cache_path_key(&track.path)))
        .collect::<HashMap<_, _>>();
    let unselected_paths = tracks
        .iter()
        .filter(|track| !requested.contains(&track.id))
        .filter_map(|track| path_keys.get(track.id.as_str()))
        .collect::<HashSet<_>>();
    let mut operations = HashMap::new();
    let mut blocked_paths = HashMap::new();
    for track in tracks
        .iter()
        .filter(|track| requested.contains(&track.id) && is_streaming_track(track))
    {
        let key = audio_operation_key(track);
        if let Err(message) = operations
            .entry(key.clone())
            .or_insert_with(|| acquire_audio_operation(&key))
        {
            blocked_paths.insert(path_keys[track.id.as_str()].clone(), message.clone());
        }
    }

    // 同一批中重复引用文件只删一次；失败结果也复用，避免后续条目误报成功。
    let mut file_results = HashMap::<String, Result<bool, String>>::new();
    for id in ids {
        let Some(track) = by_id.get(id) else {
            // 幂等：后端已经不存在的记录，也允许前端清理残留引用。
            result.deleted_ids.push(id.to_string());
            continue;
        };
        if is_streaming_track(track) {
            let path_key = &path_keys[track.id.as_str()];
            let outcome = file_results.entry(path_key.clone()).or_insert_with(|| {
                if unselected_paths.contains(path_key) {
                    return Err("此文件仍被未选中的曲目引用，请一并选择后再删除".into());
                }
                if let Some(message) = blocked_paths.get(path_key) {
                    return Err(message.clone());
                }
                let removed = delete_streaming_cache_file(Path::new(&track.path))?;
                result.deleted_files += usize::from(removed);
                Ok(removed)
            });
            if let Err(message) = outcome {
                result.failures.push(DeleteTrackFailure {
                    id: id.to_string(),
                    title: track.title.clone(),
                    message: message.clone(),
                });
                continue;
            }
        }
        result.deleted_ids.push(id.to_string());
    }
    let deleted = result.deleted_ids.iter().collect::<HashSet<_>>();
    let previous_count = tracks.len();
    let updated = tracks
        .into_iter()
        .filter(|track| !deleted.contains(&track.id))
        .collect::<Vec<_>>();
    if updated.len() != previous_count {
        save(&updated)
            .map_err(|err| format!("文件处理后保存曲库失败，记录仍保留，请重试：{err}"))?;
    }
    Ok(result)
}
