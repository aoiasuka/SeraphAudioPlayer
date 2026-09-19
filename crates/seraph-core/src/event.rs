use crate::types::TrackId;
use serde::{Deserialize, Serialize};

/// 全局播放器事件。
///
/// 由音频引擎线程产生，通过 [`crate::EventBus`] 广播给所有订阅者，
/// 最终由 Tauri 层桥接成 `app.emit_all(...)` 推给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PlayerEvent {
    PlaybackStarted {
        track_id: TrackId,
    },
    PlaybackPaused,
    PlaybackResumed,
    PlaybackStopped,
    PlaybackEnded {
        track_id: TrackId,
    },
    TrackChanged {
        track_id: TrackId,
    },
    Progress {
        track_id: TrackId,
        seconds: f64,
        total: f64,
        /// 输出延迟（秒）：`seconds` 是已送入设备缓冲的位置，比可听音频领先这么多
        /// （共享模式取 cpal 回调的 playback − callback 时间戳，独占模式取缓冲填充量）。
        /// 歌词定位用 `seconds - output_latency`；旧载荷缺省为 0。
        #[serde(default)]
        output_latency: f64,
    },
    BufferingStart,
    BufferingEnd,
    DeviceLost {
        reason: String,
    },
    DeviceRecovered {
        device_name: String,
    },
    VolumeChanged {
        volume: f32,
    },
    /// 跳转失败后会话仍可继续，携带回滚位置而不发布致命 Error。
    SeekFailed {
        track_id: TrackId,
        seconds: f64,
        message: String,
    },
    Error {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_carries_output_latency_and_accepts_legacy_payloads() {
        let event = PlayerEvent::Progress {
            track_id: "t".into(),
            seconds: 12.5,
            total: 180.0,
            output_latency: 0.125,
        };
        let json = serde_json::to_value(&event).expect("serialize");
        assert_eq!(json["type"], "progress");
        assert_eq!(json["output_latency"], 0.125);

        // 没有该字段的旧载荷仍可反序列化，延迟按 0 处理
        let legacy: PlayerEvent =
            serde_json::from_str(r#"{"type":"progress","track_id":"t","seconds":1.0,"total":2.0}"#)
                .expect("legacy payload");
        match legacy {
            PlayerEvent::Progress { output_latency, .. } => assert_eq!(output_latency, 0.0),
            other => panic!("unexpected {other:?}"),
        }
    }
}
