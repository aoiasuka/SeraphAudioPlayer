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

fn push_frames(producer: &mut rtrb::Producer<QueuedSample>, frames: usize, value: f32) {
    for _ in 0..frames {
        assert!(producer
            .push(QueuedSample {
                generation: 0,
                value
            })
            .is_ok());
    }
}

/// AUD-09：缺样（解码跟不上）之后恢复出声必须从 0 淡入，不能从满幅硬切回来。
#[test]
fn aud_09_output_fades_in_after_underrun() {
    let shared = PlaybackShared::new(48_000, 1, 1.0, SpectrumTap::new());
    let (mut producer, mut consumer) = RingBuffer::new(4_096);
    let mut render = RenderState::new(0);

    // 先正常播放一段，让增益爬到满值
    push_frames(&mut producer, 1_024, 1.0);
    let mut output = vec![0.0_f32; 1_024];
    render_output_f32(&mut output, &shared, &mut consumer, &mut render);
    assert!((output[1_023] - 1.0).abs() < 1.0e-6, "稳态应为满幅");

    // 缓冲空了：整段静音
    let mut silence = vec![1.0_f32; 512];
    render_output_f32(&mut silence, &shared, &mut consumer, &mut render);
    assert!(silence.iter().all(|sample| *sample == 0.0));

    // 数据回来：第一个样本接近 0，随后单调爬升
    push_frames(&mut producer, 1_024, 1.0);
    let mut resumed = vec![0.0_f32; 1_024];
    render_output_f32(&mut resumed, &shared, &mut consumer, &mut render);
    assert!(
        resumed[0] < 0.05,
        "恢复后的首个样本应从静音淡入，实际 {}",
        resumed[0]
    );
    assert!(resumed.windows(2).all(|pair| pair[1] >= pair[0]));
    assert!((resumed[1_023] - 1.0).abs() < 1.0e-6);
}

/// AUD-10：可视化 tap 拿到的是乘音量之前的样本，响度读数不随音量滑块变化。
#[test]
fn aud_10_visualizer_tap_is_independent_of_volume() {
    let tap = SpectrumTap::new();
    let shared = PlaybackShared::new(48_000, 1, 0.25, tap.clone());
    let (mut producer, mut consumer) = RingBuffer::new(4_096);
    let mut render = RenderState::new(0);
    push_frames(&mut producer, 2_048, 0.5);
    let mut output = vec![0.0_f32; 2_048];
    render_output_f32(&mut output, &shared, &mut consumer, &mut render);

    // 出声的样本乘了音量
    assert!((output[2_047] - 0.125).abs() < 1.0e-6);
    // tap 里是原始幅度
    let mut tapped = Vec::new();
    tap.drain(&mut tapped);
    assert!(!tapped.is_empty());
    assert!(tapped.iter().all(|sample| (*sample - 0.5).abs() < 1.0e-6));
}

/// AUD-06：已经落在 16 位格点上的样本原样输出（位精确），其余才加 dither。
#[test]
fn aud_06_sixteen_bit_samples_on_the_grid_pass_through_bit_exact() {
    let mut dither = TpdfDither::default();
    for raw in [i16::MIN, -12_345, -1, 0, 1, 2, 777, 12_345, i16::MAX] {
        let value = f32::from(raw) / 32_768.0;
        for _ in 0..64 {
            assert_eq!(quantize_i16_tpdf(value, &mut dither), raw);
        }
    }
    // 不在格点上的样本仍然被 dither：同一个输入会落到相邻的两个码字上
    let off_grid = 100.5 / 32_768.0;
    let codes: std::collections::BTreeSet<i16> = (0..256)
        .map(|_| quantize_i16_tpdf(off_grid, &mut dither))
        .collect();
    assert!(codes.len() > 1, "非格点样本应被 dither 打散：{codes:?}");
}

/// AUD-07：24 位输出同理——格点上的样本位精确，其余 dither 后舍入而不是硬截断。
#[test]
fn aud_07_twenty_four_bit_quantization_dithers_only_off_grid_samples() {
    let mut dither = TpdfDither::default();
    for raw in [-8_388_608_i32, -65_536, -256, 0, 256, 65_536, 8_388_352] {
        let value = raw as f32 / 8_388_608.0;
        assert_eq!(quantize_i24_tpdf(value, &mut dither), raw);
    }
    // 16 位源装进 24 位容器：每个 16 位码字都在 24 位格点上
    for raw in [i16::MIN, -3, 0, 5, i16::MAX] {
        let value = f32::from(raw) / 32_768.0;
        assert_eq!(quantize_i24_tpdf(value, &mut dither), i32::from(raw) << 8);
    }
    let off_grid = 0.5_f32 / 8_388_608.0 + 0.25;
    let codes: std::collections::BTreeSet<i32> = (0..256)
        .map(|_| quantize_i24_tpdf(off_grid, &mut dither))
        .collect();
    assert!(codes.len() > 1, "非格点样本应被 dither 打散");
}

/// AUD-04：只有「独占不可用」值得退回共享模式重试。
#[test]
fn aud_04_only_exclusive_unavailable_triggers_shared_fallback() {
    let unavailable = BackendError::ExclusiveModeUnavailable("format not supported".into());
    assert_eq!(
        exclusive_fallback_reason(OutputDriver::WasapiExclusive, &unavailable),
        Some("format not supported")
    );
    // 已经是共享模式就不再重试
    assert!(exclusive_fallback_reason(OutputDriver::Shared, &unavailable).is_none());
    // 设备丢失、文件问题换模式也没用
    for err in [
        BackendError::DeviceLost("unplugged".into()),
        BackendError::UnsupportedFormat("bad file".into()),
        BackendError::DeviceNotFound,
    ] {
        assert!(exclusive_fallback_reason(OutputDriver::WasapiExclusive, &err).is_none());
    }
}
