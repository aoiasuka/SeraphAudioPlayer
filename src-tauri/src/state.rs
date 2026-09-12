use parking_lot::RwLock;
use seraph_audio::PlaybackController;
use seraph_core::{EventBus, PlayerEvent, PlayerState};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

/// Tauri 全局应用状态。
///
/// 真正实现音频功能后，这里会持有 `AudioEngine` / `Library` 等的句柄；
/// 当前只暴露事件总线和状态机的占位。
///
/// 所有字段都是 `Arc`/可克隆句柄，共享同一份底层状态；`Clone` 只复制句柄，
/// 便于把 AppState 移入 `spawn_blocking`（H-1：让阻塞的播放命令离开主线程）。
#[derive(Clone)]
pub struct AppState {
    pub event_bus: EventBus,
    pub player_state: Arc<RwLock<PlayerState>>,
    playback_queue: Arc<RwLock<PlaybackQueue>>,
    pub audio: PlaybackController,
}

impl AppState {
    pub fn new() -> Self {
        let event_bus = EventBus::new();
        Self {
            audio: PlaybackController::new(event_bus.clone()),
            event_bus,
            player_state: Arc::new(RwLock::new(PlayerState::Stopped)),
            playback_queue: Arc::new(RwLock::new(PlaybackQueue::default())),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackQueueTrack {
    pub id: String,
    pub path: String,
    // SMTC 系统媒体浮窗展示用元数据（旧队列快照缺省为空，不影响播放）
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub cover: String,
    #[serde(default)]
    pub duration: u64,
}

#[derive(Debug, Clone, Default)]
struct PlaybackQueue {
    tracks: Vec<PlaybackQueueTrack>,
    current_index: usize,
    recent_track_ids: Vec<String>,
    shuffle_mode: bool,
    loop_mode: bool,
    next_index: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackQueuePreview {
    pub current_track_id: Option<String>,
    pub next_track_id: Option<String>,
    pub shuffle_mode: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum TrackAdvance {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy)]
enum PlaybackStart {
    PreserveState,
    ForcePlaying,
}

impl AppState {
    pub fn sync_playback_queue(
        &self,
        tracks: Vec<PlaybackQueueTrack>,
        current_track_index: usize,
        recent_track_ids: Vec<String>,
        shuffle_mode: bool,
        loop_mode: bool,
    ) -> PlaybackQueuePreview {
        let mut queue = self.playback_queue.write();
        queue.sync(
            tracks,
            current_track_index,
            recent_track_ids,
            shuffle_mode,
            loop_mode,
        );
        queue.preview()
    }

    pub fn set_playback_modes(&self, shuffle_mode: bool, loop_mode: bool) -> PlaybackQueuePreview {
        let mut queue = self.playback_queue.write();
        if queue.shuffle_mode != shuffle_mode {
            queue.next_index = None;
        }
        queue.shuffle_mode = shuffle_mode;
        queue.loop_mode = loop_mode;
        queue.preview()
    }

    pub fn set_current_track(&self, track_id: &str) {
        let mut queue = self.playback_queue.write();
        let Some(index) = queue.tracks.iter().position(|track| track.id == track_id) else {
            return;
        };
        queue.select_track(index);
    }

    pub fn advance_track(&self, direction: TrackAdvance) -> Result<(), String> {
        self.advance_track_with_start(direction, PlaybackStart::PreserveState)
    }

    /// SMTC 媒体键 Play：无已加载会话（resume 失败）时，从队列当前曲目从头播放。
    pub fn play_current_track(&self) -> Result<(), String> {
        let track = self.playback_queue.read().current_track().cloned();
        let Some(track) = track else {
            return Ok(());
        };
        self.play_track(&track, PlaybackStart::ForcePlaying)
    }

    /// 按 id 查询队列内曲目（SMTC 元数据展示用）。
    pub fn queue_track_by_id(&self, track_id: &str) -> Option<PlaybackQueueTrack> {
        self.playback_queue
            .read()
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .cloned()
    }

    /// 队列当前曲目（任务栏歌词条播放快照用）。
    pub fn current_queue_track(&self) -> Option<PlaybackQueueTrack> {
        self.playback_queue.read().current_track().cloned()
    }

    pub fn handle_playback_ended(&self, track_id: &str) -> Result<(), String> {
        let next = {
            let mut queue = self.playback_queue.write();
            if !queue.is_current_track(track_id) {
                return Ok(());
            }

            if queue.loop_mode {
                queue.current_track().cloned()
            } else if queue.tracks.len() > 1 {
                queue.advance(TrackAdvance::Next)
            } else {
                None
            }
        };

        if let Some(track) = next {
            self.play_track(&track, PlaybackStart::ForcePlaying)?;
        } else {
            *self.player_state.write() = PlayerState::Stopped;
            self.event_bus.publish(PlayerEvent::PlaybackStopped);
        }

        Ok(())
    }

    fn advance_track_with_start(
        &self,
        direction: TrackAdvance,
        start: PlaybackStart,
    ) -> Result<(), String> {
        let next = {
            let mut queue = self.playback_queue.write();
            queue.advance(direction)
        };

        let Some(track) = next else {
            return Ok(());
        };
        self.play_track(&track, start)
    }

    fn play_track(&self, track: &PlaybackQueueTrack, start: PlaybackStart) -> Result<(), String> {
        let should_play = match start {
            PlaybackStart::ForcePlaying => true,
            PlaybackStart::PreserveState => *self.player_state.read() == PlayerState::Playing,
        };

        if should_play {
            self.audio
                .play_file(PathBuf::from(&track.path), track.id.clone(), 0.0)
                .map_err(|err| err.to_string())?;
            *self.player_state.write() = PlayerState::Playing;
        } else {
            self.event_bus.publish(PlayerEvent::TrackChanged {
                track_id: track.id.clone(),
            });
            self.event_bus.publish(PlayerEvent::Progress {
                track_id: track.id.clone(),
                seconds: 0.0,
                total: 0.0,
            });
        }

        Ok(())
    }
}

impl PlaybackQueue {
    fn sync(
        &mut self,
        tracks: Vec<PlaybackQueueTrack>,
        current_track_index: usize,
        recent_track_ids: Vec<String>,
        shuffle_mode: bool,
        loop_mode: bool,
    ) {
        let current_index = clamp_index(current_track_index, tracks.len());
        let recent_track_ids = tracks
            .get(current_index)
            .map(|track| with_recent_track(&recent_track_ids, &track.id))
            .unwrap_or_default();
        // 重复同步和封面/歌词等元数据更新不能重新随机，否则预览会在切歌前改变。
        let keep_next = self.current_index == current_index
            && self.shuffle_mode == shuffle_mode
            && self.recent_track_ids == recent_track_ids
            && self
                .tracks
                .iter()
                .map(|track| &track.id)
                .eq(tracks.iter().map(|track| &track.id));
        let next_index = if keep_next { self.next_index } else { None };
        *self = Self {
            tracks,
            current_index,
            recent_track_ids,
            shuffle_mode,
            loop_mode,
            next_index,
        };
    }

    fn select_track(&mut self, index: usize) {
        let recent_track_ids = with_recent_track(&self.recent_track_ids, &self.tracks[index].id);
        if self.current_index != index || self.recent_track_ids != recent_track_ids {
            self.next_index = None;
        }
        self.current_index = index;
        self.recent_track_ids = recent_track_ids;
    }

    fn preview(&mut self) -> PlaybackQueuePreview {
        let next_index = self.resolve_advance_index(TrackAdvance::Next);
        PlaybackQueuePreview {
            current_track_id: self.current_track().map(|track| track.id.clone()),
            next_track_id: next_index.map(|index| self.tracks[index].id.clone()),
            shuffle_mode: self.shuffle_mode,
        }
    }

    // 自动续播、按钮和系统媒体键都从这里消费同一份预选结果。
    fn advance(&mut self, direction: TrackAdvance) -> Option<PlaybackQueueTrack> {
        let index = self.resolve_advance_index(direction)?;
        self.select_track(index);
        self.current_track().cloned()
    }

    fn current_track(&self) -> Option<&PlaybackQueueTrack> {
        self.tracks.get(self.current_index)
    }

    fn is_current_track(&self, track_id: &str) -> bool {
        self.current_track()
            .is_some_and(|track| track.id.as_str() == track_id)
    }

    fn resolve_advance_index(&mut self, direction: TrackAdvance) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }

        Some(match direction {
            TrackAdvance::Next => self.resolve_next_index(),
            TrackAdvance::Previous => self.resolve_previous_index(),
        })
    }

    fn resolve_next_index(&mut self) -> usize {
        if let Some(index) = self.next_index {
            return index;
        }
        let index = if self.shuffle_mode {
            self.resolve_shuffle_next_index()
        } else {
            (self.current_index + 1) % self.tracks.len()
        };
        self.next_index = Some(index);
        index
    }

    fn resolve_previous_index(&self) -> usize {
        if !self.shuffle_mode {
            return (self.current_index + self.tracks.len() - 1) % self.tracks.len();
        }

        let current_id = self.current_track().map(|track| track.id.as_str());
        let previous_recent_id = self
            .recent_track_ids
            .iter()
            .find(|track_id| Some(track_id.as_str()) != current_id);
        previous_recent_id
            .and_then(|track_id| self.tracks.iter().position(|track| track.id == *track_id))
            .unwrap_or_else(|| (self.current_index + self.tracks.len() - 1) % self.tracks.len())
    }

    fn resolve_shuffle_next_index(&self) -> usize {
        if self.tracks.len() <= 1 {
            return 0;
        }

        let candidates = (0..self.tracks.len()).filter(|index| *index != self.current_index);
        let fresh: Vec<usize> = candidates
            .clone()
            .filter(|index| !self.recent_track_ids.contains(&self.tracks[*index].id))
            .collect();
        let pool: Vec<usize> = if fresh.is_empty() {
            candidates.collect()
        } else {
            fresh
        };
        pool[pseudo_random_index(pool.len())]
    }
}

fn clamp_index(index: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        index.min(len - 1)
    }
}

fn with_recent_track(ids: &[String], track_id: &str) -> Vec<String> {
    let mut next = Vec::with_capacity(ids.len().min(11) + 1);
    next.push(track_id.to_string());
    for id in ids {
        if id != track_id && next.len() < 12 {
            next.push(id.clone());
        }
    }
    next
}

fn pseudo_random_index(len: usize) -> usize {
    if len <= 1 {
        return 0;
    }
    // P3-8：简单 xorshift 状态机替代"纳秒取模"——快速连点"下一首"时
    // 纳秒接近会反复选中相同索引。首次用时间纳秒做种子，之后每次推进状态。
    use std::sync::atomic::{AtomicU64, Ordering};
    static SHUFFLE_STATE: AtomicU64 = AtomicU64::new(0);

    let mut state = SHUFFLE_STATE.load(Ordering::Relaxed);
    if state == 0 {
        state = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
    }
    // xorshift64
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    SHUFFLE_STATE.store(state, Ordering::Relaxed);
    (state % len as u64) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks(ids: &[&str]) -> Vec<PlaybackQueueTrack> {
        ids.iter()
            .map(|id| PlaybackQueueTrack {
                id: (*id).into(),
                path: format!("C:/Music/{id}.flac"),
                title: (*id).into(),
                artist: String::new(),
                album: String::new(),
                cover: String::new(),
                duration: 180,
            })
            .collect()
    }

    fn shuffled_queue() -> PlaybackQueue {
        let mut queue = PlaybackQueue::default();
        queue.sync(tracks(&["a", "b", "c", "d"]), 0, vec![], true, false);
        queue
    }

    #[test]
    fn shuffle_preview_is_stable_and_matches_every_advance() {
        let mut queue = shuffled_queue();
        for _ in 0..64 {
            let preview = queue.preview();
            assert_ne!(preview.current_track_id, preview.next_track_id);
            for _ in 0..4 {
                assert_eq!(queue.preview().next_track_id, preview.next_track_id);
            }
            let next = queue.advance(TrackAdvance::Next).unwrap();
            assert_eq!(Some(next.id.clone()), preview.next_track_id);
            assert_eq!(queue.current_track().unwrap().id, next.id);
        }
    }

    #[test]
    fn resync_and_metadata_updates_preserve_the_reserved_track() {
        let mut queue = shuffled_queue();
        let expected = queue.preview().next_track_id;
        for i in 0..16 {
            let mut updated = queue.tracks.clone();
            updated[1].title = format!("Updated title {i}");
            updated[2].cover = format!("C:/covers/{i}.png");
            queue.sync(updated, 0, vec!["a".into()], true, false);
            assert_eq!(queue.preview().next_track_id, expected);
        }
        assert_eq!(
            Some(queue.advance(TrackAdvance::Next).unwrap().id),
            expected
        );
    }

    #[test]
    fn duplicate_track_events_do_not_change_the_preview() {
        let mut queue = shuffled_queue();
        let expected = queue.preview().next_track_id;
        queue.select_track(0);
        queue.select_track(0);
        assert_eq!(queue.preview().next_track_id, expected);
    }

    #[test]
    fn shuffle_excludes_current_and_recent_tracks_when_fresh_tracks_exist() {
        let mut queue = shuffled_queue();
        queue.sync(
            queue.tracks.clone(),
            0,
            vec!["a".into(), "b".into(), "c".into()],
            true,
            false,
        );
        assert_eq!(queue.preview().next_track_id.as_deref(), Some("d"));
        assert_eq!(queue.advance(TrackAdvance::Next).unwrap().id, "d");
    }

    #[test]
    fn replacing_or_reordering_a_queue_refreshes_the_preview() {
        let mut queue = shuffled_queue();
        queue.preview();
        queue.sync(tracks(&["a", "e"]), 0, vec![], true, false);
        assert_eq!(queue.preview().next_track_id.as_deref(), Some("e"));
        queue.sync(tracks(&["e", "a"]), 1, vec![], true, false);
        assert_eq!(queue.advance(TrackAdvance::Next).unwrap().id, "e");
    }

    #[test]
    fn disabling_shuffle_uses_sequential_order_and_wraps() {
        let mut queue = shuffled_queue();
        queue.preview();
        queue.sync(queue.tracks.clone(), 0, vec![], false, false);
        for expected in ["b", "c", "d", "a"] {
            assert_eq!(queue.preview().next_track_id.as_deref(), Some(expected));
            assert_eq!(queue.advance(TrackAdvance::Next).unwrap().id, expected);
        }
    }

    #[test]
    fn previous_in_shuffle_follows_history_and_refreshes_next() {
        let mut queue = shuffled_queue();
        queue.advance(TrackAdvance::Next);
        queue.preview();
        assert_eq!(queue.advance(TrackAdvance::Previous).unwrap().id, "a");
        let preview = queue.preview();
        assert_ne!(preview.next_track_id.as_deref(), Some("a"));
        assert_eq!(
            Some(queue.advance(TrackAdvance::Next).unwrap().id),
            preview.next_track_id
        );
    }

    #[test]
    fn empty_and_single_track_queues_have_valid_previews() {
        let mut queue = PlaybackQueue::default();
        assert!(queue.preview().next_track_id.is_none());
        assert!(queue.advance(TrackAdvance::Next).is_none());
        queue.sync(tracks(&["only"]), 99, vec![], true, false);
        assert_eq!(queue.preview().next_track_id.as_deref(), Some("only"));
        assert_eq!(queue.advance(TrackAdvance::Next).unwrap().id, "only");
        queue.sync(vec![], 0, vec![], true, false);
        assert!(queue.preview().next_track_id.is_none());
    }
}
