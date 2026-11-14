use super::prelude::*;
use crate::ipc::{
    bilibili::{acquire_audio_operation, extract_bvid},
    cache::delete_streaming_cache_file,
};

/// 流媒体身份只看来源字段与 ID 前缀（与前端 `trackFilters.ts::isStreamingTrack` 同口径）。
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
    // DEC4-03：不按专辑名判定。标签里专辑写着「Bilibili」的本地文件会被当成流媒体，
    // 删除时去删「缓存文件」而它不在受管缓存目录里，结果这条记录永远删不掉。
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

/// 不触盘的路径键：只做分隔符与大小写归一化。
fn lexical_path_key(path: &str) -> String {
    path.replace('\\', "/").to_ascii_lowercase()
}

/// 文件名（小写）：判断两条记录有没有可能指向同一个文件的廉价前置条件。
fn file_name_key(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_ascii_lowercase())
}

/// 调用方全程持有 LIBRARY_LOCK。只接收 ID；用于删文件的路径来自可信曲库快照。
/// 成功项一次提交，失败项保留；占用 guard 直到存储提交后才释放。
pub(super) fn delete_selected_tracks(
    tracks: Vec<ImportedTrack>,
    track_ids: &[String],
    save: impl FnOnce(Vec<ImportedTrack>) -> Result<(), String>,
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
    //
    // BE-02：规范化（`canonicalize`）是一次文件系统调用，此前对**全库每一首**都做——
    // 曲库里有离线网络盘上的本地曲目时，每个路径可能等几秒才超时，全程还持有曲库锁。
    // 现在只对「文件名与某个待删缓存文件相同」的曲目做：两条记录要指向同一个文件，
    // 文件名必然相同（不区分大小写）；文件名不同的直接用不触盘的字符串键。
    let deleting_names = tracks
        .iter()
        .filter(|track| requested.contains(&track.id) && is_streaming_track(track))
        .filter_map(|track| file_name_key(&track.path))
        .collect::<HashSet<_>>();
    let path_keys = tracks
        .iter()
        .filter(|_| has_streaming_deletion)
        .map(|track| {
            let may_collide =
                file_name_key(&track.path).is_some_and(|name| deleting_names.contains(&name));
            let key = if may_collide {
                cache_path_key(&track.path)
            } else {
                lexical_path_key(&track.path)
            };
            (track.id.as_str(), key)
        })
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
        save(updated)
            .map_err(|err| format!("文件处理后保存曲库失败，记录仍保留，请重试：{err}"))?;
    }
    Ok(result)
}
