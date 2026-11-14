//! 播放状态回归：使用模拟解码器和无输出流的会话，不打开声卡。
use super::*;
use seraph_core::types::{BitDepth, Channels, SampleRate};
use seraph_decoder::{DecoderError, Packet};

#[test]
fn missing_output_counts_frames_and_excludes_pause_and_stop() {
    let tap = SpectrumTap::new();
    let shared = PlaybackShared::new(48_000, 2, 0.5, tap.clone());
    let (mut producer, mut consumer) = RingBuffer::new(16);
    for _ in 0..2 {
        assert!(producer
            .push(QueuedSample {
                generation: 0,
                value: 0.25
            })
            .is_ok());
    }
    let mut render = RenderState::new(0);
    let mut output = [1.0; 8];
    render_output_f32(&mut output, &shared, &mut consumer, &mut render);
    assert_eq!(shared.frame_position.load(Ordering::Relaxed), 1);
    assert_eq!(tap.diagnostics().missing_output_frames, 3);
    assert_eq!(&output[2..], &[0.0; 6]);
    shared.paused.store(true, Ordering::Release);
    render_output_f32(&mut output, &shared, &mut consumer, &mut render);
    shared.paused.store(false, Ordering::Release);
    shared.stopped.store(true, Ordering::Release);
    render_output_f32(&mut output, &shared, &mut consumer, &mut render);
    assert_eq!(tap.diagnostics().missing_output_frames, 3);
}

#[test]
fn bug_audit_06_engine_is_silent_until_settings_arrive() {
    let mut engine = PlaybackEngine::new(EventBus::new());
    assert_eq!(engine.volume, 0.0);
    engine.set_volume(0.15).unwrap();
    assert_eq!(engine.volume, 0.15);
    engine.set_volume(0.0).unwrap();
    assert_eq!(engine.volume, 0.0);
}

#[test]
fn bug_audit_07_resume_checks_selected_track_identity_and_preserves_position() {
    let missing =
        std::env::temp_dir().join(format!("seraph-session-audit-{}.flac", std::process::id()));
    assert!(!missing.exists(), "测试路径必须不存在，以免打开声卡");
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let mut engine = PlaybackEngine::new(bus);
    let shared = Arc::new(PlaybackShared::new(8_000, 1, 0.0, SpectrumTap::new()));
    shared.paused.store(true, Ordering::Release);
    shared.frame_position.store(80_000, Ordering::Relaxed);
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        while !worker_shared.stopped.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    });
    engine.session = Some(PlaybackSession {
        path: missing.clone(),
        track_id: "a".into(),
        duration_seconds: 180.0,
        shared: shared.clone(),
        decode_worker: Some(worker),
        render_worker: None,
        _stream: None,
    });

    let resumed = engine.play_file(missing.clone(), "a".into(), None);
    let position = shared.progress_seconds();
    let same_track_resumed = !shared.paused.load(Ordering::Acquire);
    let _ = rx.try_iter().collect::<Vec<_>>();
    shared.paused.store(true, Ordering::Release);
    // 即使路径相同，队列选中了 B 也不能恢复 A 的会话。
    let changed = engine.play_file(missing, "b".into(), None);
    engine.stop_session();
    let events = rx.try_iter().collect::<Vec<_>>();
    assert!(resumed.is_ok() && same_track_resumed);
    assert_eq!(position, 10.0);
    assert!(
        changed.is_err(),
        "应尝试加载 B 并报文件缺失，不能成功恢复 A"
    );
    assert!(shared.stopped.load(Ordering::Acquire));
    assert!(!events
        .iter()
        .any(|event| matches!(event, PlayerEvent::PlaybackResumed)));
}

struct UnseekableDecoder {
    info: StreamInfo,
}

impl Decoder for UnseekableDecoder {
    fn open(&mut self, _: &std::path::Path) -> std::result::Result<(), DecoderError> {
        Ok(())
    }
    fn info(&self) -> Option<&StreamInfo> {
        Some(&self.info)
    }
    fn next_packet(&mut self) -> std::result::Result<Option<Packet>, DecoderError> {
        Ok(Some(Packet {
            samples: vec![0.25; 256],
            timestamp_seconds: 0.0,
        }))
    }
    fn seek(&mut self, _: f64) -> std::result::Result<(), DecoderError> {
        Err(DecoderError::UnsupportedFormat("测试流不可跳转".into()))
    }
}

#[test]
fn bug_audit_08_failed_seek_keeps_playing_and_reports_rollback() {
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let shared = Arc::new(PlaybackShared::new(8_000, 1, 0.5, SpectrumTap::new()));
    *shared.seek_request.lock() = Some(SeekRequest {
        seconds: 10.0,
        prev_frames: 16_000,
    });
    let (producer, consumer) = RingBuffer::new(shared.max_buffer_samples);
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        let info = StreamInfo {
            sample_rate: SampleRate(8_000),
            bit_depth: BitDepth(16),
            channels: Channels(1),
            duration_seconds: 180.0,
        };
        run_decode_worker(
            Box::new(UnseekableDecoder { info: info.clone() }),
            "test-track",
            &info,
            &worker_shared,
            producer,
            &bus,
            0.0,
            &DspControl::new(),
            false,
        )
    });
    let event = rx.recv_timeout(Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(1);
    while consumer.slots() == 0 && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(1));
    }
    let queued = consumer.slots();
    let playing = !shared.paused.load(Ordering::Acquire) && !shared.stopped.load(Ordering::Acquire);
    shared.stopped.store(true, Ordering::Release);
    worker.join().unwrap().unwrap();
    assert!(playing && queued > 0);
    assert!(
        matches!(event.unwrap(), PlayerEvent::SeekFailed { track_id, seconds, .. }
        if track_id == "test-track" && seconds == 2.0)
    );
    assert!(!rx.try_iter().any(|event| matches!(
        event,
        PlayerEvent::Error { .. } | PlayerEvent::PlaybackStopped
    )));
}

/// 造一个「正在播放」的假会话：不打开声卡，解码线程空转到 stopped。
fn fake_live_session(engine: &mut PlaybackEngine, path: std::path::PathBuf) -> Arc<PlaybackShared> {
    let shared = Arc::new(PlaybackShared::new(8_000, 1, 0.0, SpectrumTap::new()));
    let worker_shared = shared.clone();
    let worker = thread::spawn(move || {
        while !worker_shared.stopped.load(Ordering::Acquire) {
            thread::sleep(Duration::from_millis(1));
        }
    });
    engine.session = Some(PlaybackSession {
        path,
        track_id: "a".into(),
        duration_seconds: 180.0,
        shared: shared.clone(),
        decode_worker: Some(worker),
        render_worker: None,
        _stream: None,
    });
    shared
}

fn missing_audio_path(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("seraph-{tag}-{}.flac", std::process::id()));
    assert!(!path.exists(), "测试路径必须不存在，以免打开声卡");
    path
}

/// AUD-01：命令失败但播放没停（例如设备 ID 校验失败、切换设备失败后已回滚）时，
/// 只发 Error，不能补发 PlaybackStopped——否则界面显示停止、曲终不再自动切歌。
#[test]
fn aud_01_failed_command_does_not_report_stopped_while_session_is_live() {
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let mut engine = PlaybackEngine::new(bus.clone());
    let shared = fake_live_session(&mut engine, missing_audio_path("aud01-live"));

    // 无效设备 ID：校验发生在动会话之前
    let err = engine
        .set_output_device("seraph-test:no-such-device".into())
        .expect_err("无效设备必须报错");
    assert!(!shared.stopped.load(Ordering::Acquire), "会话不应被动到");
    publish_command_failure(&bus, &engine, &err);
    let events = rx.try_iter().collect::<Vec<_>>();
    assert!(events
        .iter()
        .any(|event| matches!(event, PlayerEvent::Error { .. })));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, PlayerEvent::PlaybackStopped)),
        "播放仍在继续，不能上报已停止：{events:?}"
    );
    engine.stop_session();
}

/// AUD-01 的另一半：会话确实没了（起播失败）时照旧上报已停止。
#[test]
fn aud_01_failed_command_reports_stopped_when_session_is_gone() {
    let bus = EventBus::new();
    let rx = bus.subscribe();
    let mut engine = PlaybackEngine::new(bus.clone());
    let err = engine
        .play_file(missing_audio_path("aud01-gone"), "a".into(), Some(0.0))
        .expect_err("文件不存在必须报错");
    publish_command_failure(&bus, &engine, &err);
    let events = rx.try_iter().collect::<Vec<_>>();
    assert!(events
        .iter()
        .any(|event| matches!(event, PlayerEvent::PlaybackStopped)));
}

/// AUD-05：切换驱动后起播失败，引擎里的驱动要还原成旧值。
#[test]
fn aud_05_failed_driver_switch_rolls_back_driver() {
    let mut engine = PlaybackEngine::new(EventBus::new());
    assert_eq!(engine.driver, OutputDriver::WasapiExclusive);
    // 会话指向不存在的文件：重建会话必然在打开解码器时失败，不会碰声卡
    fake_live_session(&mut engine, missing_audio_path("aud05"));

    let result = engine.set_driver("direct".into());
    assert!(result.is_err(), "重建会话应失败");
    assert_eq!(
        engine.driver,
        OutputDriver::WasapiExclusive,
        "失败后驱动必须回滚，否则之后每次播放都用起不来的新驱动"
    );

    // 没有会话时切换驱动照常生效
    engine.stop_session();
    engine.set_driver("direct".into()).unwrap();
    assert_eq!(engine.driver, OutputDriver::Shared);
}

/// AUD-08：卡住的工作线程不能把调用方一起钉死。
#[test]
fn aud_08_stuck_worker_join_is_bounded() {
    let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
    let worker = thread::spawn(move || {
        let _ = release_rx.recv();
        7_u32
    });
    let started = Instant::now();
    let joined = join_worker_within(worker, "test", Duration::from_millis(60));
    assert!(joined.is_none(), "超时应 detach");
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(release_tx);

    let quick = thread::spawn(|| 7_u32);
    let joined = join_worker_within(quick, "test", Duration::from_secs(5));
    assert_eq!(joined.and_then(|result| result.ok()), Some(7));
}
