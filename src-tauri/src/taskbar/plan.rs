//! 任务栏渲染计划:PlayerEvent → 播放阶段状态机 → 渲染计划的纯逻辑层。
//!
//! 该层不触碰任何 Win32/COM,可单测;thumbbar.rs 只负责把 [`RenderPlan`]
//! 翻译成 ITaskbarList3 调用。进度以整秒为粒度(与 SMTC 同口径),
//! 上游事件高于 1Hz 时计划天然去抖(整秒不变则计划相等,不触发刷新)。

use seraph_core::PlayerEvent;

/// 缩略图工具栏按钮 command id(WM_COMMAND 的 LOWORD)。
pub const BTN_PREV: u16 = 1;
pub const BTN_TOGGLE: u16 = 2;
pub const BTN_NEXT: u16 = 3;

/// 播放/暂停复合按钮当前应显示的图标。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleGlyph {
    Play,
    Pause,
}

/// 任务栏图标进度条渲染计划。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPlan {
    /// 不显示进度(停止 / 时长未知 / 用户关闭)。
    None,
    /// 缓冲中:不确定进度动画。
    Indeterminate,
    Normal {
        done: u64,
        total: u64,
    },
    Paused {
        done: u64,
        total: u64,
    },
}

/// 一帧完整的任务栏渲染计划;PartialEq 用于上游去抖。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderPlan {
    pub buttons_visible: bool,
    pub toggle: ToggleGlyph,
    pub progress: ProgressPlan,
}

/// 设置页开关(前端持久化,水合后经 IPC 同步)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Features {
    pub buttons: bool,
    pub progress: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            buttons: true,
            progress: true,
        }
    }
}

/// 播放阶段:由事件流直接驱动(不读 AppState,避免跨线程时序竞争)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlayPhase {
    Stopped,
    Playing,
    Paused,
    Buffering,
}

/// 事件累积出的任务栏状态。
#[derive(Debug, Clone, Copy)]
pub struct PlanState {
    phase: PlayPhase,
    seconds: u64,
    total: u64,
}

impl Default for PlanState {
    fn default() -> Self {
        Self {
            phase: PlayPhase::Stopped,
            seconds: 0,
            total: 0,
        }
    }
}

impl PlanState {
    /// 依据播放事件推进状态。与 smtc.rs 一致:PlaybackEnded 不直接改相
    /// (队列自动推进随后会发 Started/Stopped,跟随即可)。
    pub fn apply_event(&mut self, event: &PlayerEvent) {
        match event {
            PlayerEvent::PlaybackStarted { .. } => {
                self.phase = PlayPhase::Playing;
                self.seconds = 0;
                self.total = 0;
            }
            // 暂停态切歌(PreserveState)只发 TrackChanged:保持相位,进度归零
            PlayerEvent::TrackChanged { .. } => {
                self.seconds = 0;
                self.total = 0;
            }
            PlayerEvent::PlaybackResumed => self.phase = PlayPhase::Playing,
            PlayerEvent::PlaybackPaused => self.phase = PlayPhase::Paused,
            PlayerEvent::PlaybackStopped => {
                self.phase = PlayPhase::Stopped;
                self.seconds = 0;
                self.total = 0;
            }
            PlayerEvent::BufferingStart => self.phase = PlayPhase::Buffering,
            PlayerEvent::BufferingEnd => self.phase = PlayPhase::Playing,
            PlayerEvent::Progress { seconds, total, .. } => {
                self.seconds = seconds.max(0.0) as u64;
                self.total = total.max(0.0) as u64;
            }
            _ => {}
        }
    }

    /// 当前是否处于播放中(缓冲视为播放,与切换按钮口径一致)。
    pub fn is_playing(&self) -> bool {
        matches!(self.phase, PlayPhase::Playing | PlayPhase::Buffering)
    }

    /// 最近一次 Progress 的整秒进度(暂停后保留,供歌词条快照)。
    pub fn seconds(&self) -> u64 {
        self.seconds
    }

    /// 最近一次 Progress 的整秒总时长(未知为 0)。
    pub fn total(&self) -> u64 {
        self.total
    }
}

/// (状态, 开关) → 渲染计划。
pub fn render_plan(state: &PlanState, features: &Features) -> RenderPlan {
    let toggle = match state.phase {
        PlayPhase::Playing | PlayPhase::Buffering => ToggleGlyph::Pause,
        PlayPhase::Stopped | PlayPhase::Paused => ToggleGlyph::Play,
    };

    let progress = if !features.progress {
        ProgressPlan::None
    } else {
        match state.phase {
            PlayPhase::Stopped => ProgressPlan::None,
            PlayPhase::Buffering => ProgressPlan::Indeterminate,
            // 时长未知(流 / 元数据缺失)时不显示假进度
            PlayPhase::Playing | PlayPhase::Paused if state.total == 0 => ProgressPlan::None,
            PlayPhase::Playing => ProgressPlan::Normal {
                done: state.seconds.min(state.total),
                total: state.total,
            },
            PlayPhase::Paused => ProgressPlan::Paused {
                done: state.seconds.min(state.total),
                total: state.total,
            },
        }
    };

    RenderPlan {
        buttons_visible: features.buttons,
        toggle,
        progress,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started() -> PlayerEvent {
        PlayerEvent::PlaybackStarted {
            track_id: "t1".into(),
        }
    }

    fn progress(seconds: f64, total: f64) -> PlayerEvent {
        PlayerEvent::Progress {
            track_id: "t1".into(),
            seconds,
            total,
        }
    }

    fn state_after(events: &[PlayerEvent]) -> PlanState {
        let mut state = PlanState::default();
        for event in events {
            state.apply_event(event);
        }
        state
    }

    #[test]
    fn initial_state_shows_play_glyph_and_no_progress() {
        let plan = render_plan(&PlanState::default(), &Features::default());
        assert_eq!(plan.toggle, ToggleGlyph::Play);
        assert_eq!(plan.progress, ProgressPlan::None);
        assert!(plan.buttons_visible);
    }

    #[test]
    fn playing_shows_pause_glyph_and_normal_progress() {
        let state = state_after(&[started(), progress(30.4, 200.9)]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.toggle, ToggleGlyph::Pause);
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 30,
                total: 200
            }
        );
    }

    #[test]
    fn paused_shows_play_glyph_and_paused_progress() {
        let state = state_after(&[
            started(),
            progress(10.0, 100.0),
            PlayerEvent::PlaybackPaused,
        ]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.toggle, ToggleGlyph::Play);
        assert_eq!(
            plan.progress,
            ProgressPlan::Paused {
                done: 10,
                total: 100
            }
        );
    }

    #[test]
    fn stopped_clears_progress() {
        let state = state_after(&[
            started(),
            progress(50.0, 100.0),
            PlayerEvent::PlaybackStopped,
        ]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.toggle, ToggleGlyph::Play);
        assert_eq!(plan.progress, ProgressPlan::None);
    }

    #[test]
    fn buffering_shows_indeterminate_then_recovers() {
        let mut state =
            state_after(&[started(), progress(5.0, 100.0), PlayerEvent::BufferingStart]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.progress, ProgressPlan::Indeterminate);
        assert_eq!(plan.toggle, ToggleGlyph::Pause);

        state.apply_event(&PlayerEvent::BufferingEnd);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 5,
                total: 100
            }
        );
    }

    #[test]
    fn unknown_duration_hides_progress() {
        let state = state_after(&[started(), progress(30.0, 0.0)]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.progress, ProgressPlan::None);
    }

    #[test]
    fn done_is_clamped_to_total() {
        let state = state_after(&[started(), progress(120.0, 100.0)]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 100,
                total: 100
            }
        );
    }

    #[test]
    fn progress_feature_off_hides_progress_but_keeps_buttons() {
        let state = state_after(&[started(), progress(30.0, 100.0)]);
        let features = Features {
            buttons: true,
            progress: false,
        };
        let plan = render_plan(&state, &features);
        assert_eq!(plan.progress, ProgressPlan::None);
        assert!(plan.buttons_visible);
        assert_eq!(plan.toggle, ToggleGlyph::Pause);
    }

    #[test]
    fn buttons_feature_off_hides_buttons_but_keeps_progress() {
        let state = state_after(&[started(), progress(30.0, 100.0)]);
        let features = Features {
            buttons: false,
            progress: true,
        };
        let plan = render_plan(&state, &features);
        assert!(!plan.buttons_visible);
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 30,
                total: 100
            }
        );
    }

    #[test]
    fn track_changed_while_paused_keeps_phase_and_resets_progress() {
        let state = state_after(&[
            started(),
            progress(80.0, 100.0),
            PlayerEvent::PlaybackPaused,
            PlayerEvent::TrackChanged {
                track_id: "t2".into(),
            },
        ]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(plan.toggle, ToggleGlyph::Play);
        // 新曲目时长未知前不显示旧进度
        assert_eq!(plan.progress, ProgressPlan::None);
    }

    #[test]
    fn playback_ended_is_ignored_until_follow_up_event() {
        let state = state_after(&[
            started(),
            progress(99.0, 100.0),
            PlayerEvent::PlaybackEnded {
                track_id: "t1".into(),
            },
        ]);
        let plan = render_plan(&state, &Features::default());
        // Ended 后由队列推进决定去向(Started 或 Stopped),此刻保持原样
        assert_eq!(plan.toggle, ToggleGlyph::Pause);
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 99,
                total: 100
            }
        );
    }

    #[test]
    fn negative_progress_clamps_to_zero() {
        let state = state_after(&[started(), progress(-3.0, 100.0)]);
        let plan = render_plan(&state, &Features::default());
        assert_eq!(
            plan.progress,
            ProgressPlan::Normal {
                done: 0,
                total: 100
            }
        );
    }
}
