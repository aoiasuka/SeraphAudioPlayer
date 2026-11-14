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
    /// 本次播放实际打开的输出格式，建流成功后发布一次（界面的信号链显示用）。
    ///
    /// 共享模式下 `output_*` 是交给系统混音器的格式，设备最终工作在什么格式由
    /// Windows 决定、这里看不到；独占模式下是向设备申请的格式。
    OutputFormat {
        track_id: TrackId,
        /// `"shared"` 或 `"exclusive"`
        mode: String,
        source_rate: u32,
        /// 源位深；解码器不提供时为 0
        source_bits: u16,
        source_channels: u16,
        output_rate: u32,
        /// 输出的有效位深（独占 24-in-32 容器记 24）
        output_bits: u16,
        output_float: bool,
        output_channels: u16,
    },
    BufferingStart,
    BufferingEnd,
    DeviceLost {
        reason: String,
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
    fn output_format_serializes_with_snake_case_fields() {
        // 前端按这些字段名读取（usePlayback 的 output_format 分支）
        let event = PlayerEvent::OutputFormat {
            track_id: "t".into(),
            mode: "exclusive".into(),
            source_rate: 44_100,
            source_bits: 16,
            source_channels: 2,
            output_rate: 48_000,
            output_bits: 24,
            output_float: false,
            output_channels: 2,
        };
        let json = serde_json::to_value(&event).expect("serialize");
        assert_eq!(json["type"], "output_format");
        assert_eq!(json["track_id"], "t");
        assert_eq!(json["mode"], "exclusive");
        assert_eq!(json["source_rate"], 44_100);
        assert_eq!(json["source_bits"], 16);
        assert_eq!(json["source_channels"], 2);
        assert_eq!(json["output_rate"], 48_000);
        assert_eq!(json["output_bits"], 24);
        assert_eq!(json["output_float"], false);
        assert_eq!(json["output_channels"], 2);
    }

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
