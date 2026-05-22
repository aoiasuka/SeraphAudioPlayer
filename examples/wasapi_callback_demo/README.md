# wasapi_callback_demo

接续 `wasapi_exclusive_demo` 的下一步技术验证。这是**最终架构核心数据通路的最小可运行版本**:

```
┌─────────────────┐    ┌─────────────┐    ┌──────────────────────┐
│ Producer Thread │───▶│ RingBuffer  │───▶│ WasapiExclusiveOutput│
│ (模拟解码器)    │    │ (SPSC,无锁)│    │  渲染线程 / 独占模式 │
│ Sine16 generator│    │ ~1s capacity│    │  AVRT Pro Audio      │
└─────────────────┘    └─────────────┘    └──────────────────────┘
```

## 验证了什么

相比 `wasapi_exclusive_demo`:

| 维度 | wasapi_exclusive_demo | wasapi_callback_demo |
|------|----------------------|----------------------|
| 文件 | 1 个 main.cpp | 用 apx_core + apx_platform 静态库 |
| 数据生成 | 渲染线程内现算 | 独立 producer 线程,通过 RingBuffer 传递 |
| 抽象 | 全裸 WASAPI | 走 `IAudioOutput` 接口 |
| 用途 | 验证 WASAPI 可用 | 验证最终架构的核心 |

## 构建运行

```powershell
cd D:\_audio_player_x86
cmake -S . -B build -A Win32 -DAPX_BUILD_APP=OFF -DAPX_BUILD_EXAMPLES=ON
cmake --build build --config Release
.\build\bin\Release\wasapi_callback_demo.exe
```

预期输出:
```
[INFO] Requested format: 44100 Hz, 2ch, 16/16-bit int16
[ OK ] RingBuffer capacity: 262144 bytes (~1486 ms)
[ OK ] Device: <你的 DAC>
[ OK ] Buffer: 441 frames (10.00 ms), device period 10.00 ms
[ OK ] Playback started — 5 s of 1000 Hz tone
[ OK ] Done.
```

## 关键代码片段

回调签名极简:
```cpp
out.setDataCallback([&ring](std::uint8_t* dst, std::size_t bytes) {
    return ring.read(dst, bytes);     // 不够输出端会自动静音补齐
});
```

之后接入真正解码器时,把 `Producer Thread` 里的正弦生成换成 `IDecoder::read()`,
`RingBuffer` 不变,`WasapiExclusiveOutput` 不变。这就是 M1 阶段的目标形态。
