# wasapi_play_wav_demo

把 `IDecoder` / `RingBuffer` / `WasapiExclusiveOutput` 串起来,真正播放一个 WAV 文件。这是 M1 阶段的最终形态。

## 数据流

```
            decoder->read()                  ring.read()
[ WAV file ] ─────────────▶ [ RingBuffer ] ─────────────▶ [ WASAPI Exclusive ]
                ▲                                  ▲
        producer thread                   render thread (AVRT Pro Audio)
```

## 用法

```powershell
.\build\bin\Release\wasapi_play_wav_demo.exe "D:\Music\test.wav"
```

支持文件:
- WAVE_FORMAT_PCM        — 16 / 24 (packed) / 32 bit
- WAVE_FORMAT_IEEE_FLOAT — 32 bit
- WAVE_FORMAT_EXTENSIBLE — 上述两种通过 SubFormat GUID

不支持:RF64、Wave64、ADPCM、μ-law/A-law。

## 预期输出

```
[ OK ] File:     D:\Music\test.wav
[ OK ] Format:   96000 Hz, 2ch, 24/24-bit int24packed
[ OK ] Duration: 245.331 s (23551776 frames)
[ OK ] Device:   Topping DX3 Pro
[ OK ] Buffer:   480 frames (5.00 ms),周期 5.00 ms
[ OK ] Playing... (Ctrl+C 退出)
[ OK ] 播放完毕
```

## 常见问题

| 现象 | 原因 |
|------|------|
| `[FAIL] WASAPI open: IsFormatSupported failed: ...UNSUPPORTED_FORMAT` | DAC 不支持源文件的采样率/位深。换文件或换设备 |
| `[FAIL] WASAPI open: ...DEVICE_IN_USE` | 设备被其它独占程序占用(常见:foobar2000 WASAPI 输出、ASIO 软件) |
| 文件能解但听不到声 | 系统默认输出不是你的 DAC。"声音设置 → 输出设备"选对 |
| 播放到中段卡顿/喷麦 | 缓冲区欠载。可加大 `RingBuffer` 容量或在 producer 端预读更多 |

## 设计要点

- **共格式协商**:`decoder->format()` 直接喂给 `WasapiExclusiveOutput::open()`,无重采样。如果 DAC 不支持,程序直接退出而非偷偷降级——这是位完美策略
- **EOF 处理**:producer 读到 0 设 `eof` 标志,主线程等 `ring.readable()==0` 再 stop;再多睡一个 buffer_ms 让最后帧出声
- **Ctrl+C 中断**:`signal(SIGINT)` → 主循环退出 → `stop()` + `join()`,资源完整释放
