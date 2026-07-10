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
}

#[derive(Debug, Clone, Default)]
struct PlaybackQueue {
    tracks: Vec<PlaybackQueueTrack>,
    current_index: usize,
    recent_track_ids: Vec<String>,
    shuffle_mode: bool,
    loop_mode: bool,
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
    ) {
        let current_index = clamp_index(current_track_index, tracks.len());
        *self.playback_queue.write() = PlaybackQueue {
            tracks,
            current_index,
            recent_track_ids,
            shuffle_mode,
            loop_mode,
        };
    }

    pub fn set_playback_modes(&self, shuffle_mode: bool, loop_mode: bool) {
        let mut queue = self.playback_queue.write();
        queue.shuffle_mode = shuffle_mode;
        queue.loop_mode = loop_mode;
    }

    pub fn set_current_track(&self, track_id: &str) {
        let mut queue = self.playback_queue.write();
        let Some(index) = queue.tracks.iter().position(|track| track.id == track_id) else {
            return;
        };
        queue.current_index = index;
        queue.recent_track_ids = with_recent_track(&queue.recent_track_ids, track_id);
    }

    pub fn advance_track(&self, direction: TrackAdvance) -> Result<(), String> {
        self.advance_track_with_start(direction, PlaybackStart::PreserveState)
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
                let index = queue.resolve_next_index();
                queue.current_index = index;
                let next = queue.current_track().cloned();
                if let Some(track) = next.as_ref() {
                    queue.recent_track_ids = with_recent_track(&queue.recent_track_ids, &track.id);
                }
                next
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
            let Some(index) = queue.resolve_advance_index(direction) else {
                return Ok(());
            };
            queue.current_index = index;
            let next = queue.current_track().cloned();
            if let Some(track) = next.as_ref() {
                queue.recent_track_ids = with_recent_track(&queue.recent_track_ids, &track.id);
            }
            next
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
    fn current_track(&self) -> Option<&PlaybackQueueTrack> {
        self.tracks.get(self.current_index)
    }

    fn is_current_track(&self, track_id: &str) -> bool {
        self.current_track()
            .is_some_and(|track| track.id.as_str() == track_id)
    }

    fn resolve_advance_index(&self, direction: TrackAdvance) -> Option<usize> {
        if self.tracks.is_empty() {
            return None;
        }

        Some(match direction {
            TrackAdvance::Next => self.resolve_next_index(),
            TrackAdvance::Previous => self.resolve_previous_index(),
        })
    }

    fn resolve_next_index(&self) -> usize {
        if self.shuffle_mode {
            return self.resolve_shuffle_next_index();
        }
        (self.current_index + 1) % self.tracks.len()
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
