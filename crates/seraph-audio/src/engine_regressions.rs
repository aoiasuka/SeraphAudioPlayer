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
