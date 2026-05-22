# wasapi_exclusive_demo

WASAPI 独占模式技术验证程序。**纯 Win32 + Core Audio**,不依赖第三方库。

## 程序行为

向系统默认音频渲染设备以独占模式输出 5 秒 1 kHz 正弦波 (-14 dBFS)。

控制台会按顺序打印:
```
[INFO] === WASAPI Exclusive Mode Demo ===
[ OK ] Default render device: <你的设备名>
[ OK ] Negotiated format: 44100 Hz, 16-bit int, 2 ch
[INFO] Device period: default=10.000 ms, min=3.000 ms
[ OK ] Buffer size: 441 frames (10.000 ms)
[ OK ] MMCSS task 'Pro Audio' attached
[ OK ] Playback started — 5 seconds of 1000 Hz tone
[ OK ] Rendered 220500 frames (target 220500), glitches=0
[ OK ] Done.
```

## 验证了什么

1. **COM 初始化** (MTA)
2. **设备枚举** (`IMMDeviceEnumerator` / `GetDefaultAudioEndpoint`)
3. **独占模式格式协商** (`WAVEFORMATEXTENSIBLE`, 16-bit PCM → 32-bit float 回退)
4. **设备周期获取** (`GetDevicePeriod`)
5. **事件驱动初始化** (`AUDCLNT_STREAMFLAGS_EVENTCALLBACK`)
6. **缓冲区对齐重试** (`AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED` 标准套路)
7. **AVRT 提权** (`Pro Audio` MMCSS 任务)
8. **渲染循环** (WaitForSingleObject → GetBuffer → 填充 → ReleaseBuffer)
9. **资源清理** (RAII-friendly 的跳转-清理结构)

## 单独构建运行

### 方式 A:用脚本
```powershell
cd D:\_audio_player_x86
.\scripts\build_demo.ps1
.\build\bin\Release\wasapi_exclusive_demo.exe
```

### 方式 B:手动 CMake
```powershell
cd D:\_audio_player_x86
cmake -S . -B build -A Win32 -DAPX_BUILD_APP=OFF -DAPX_BUILD_EXAMPLES=ON
cmake --build build --config Release
.\build\bin\Release\wasapi_exclusive_demo.exe
```

### 方式 C:不用 CMake,直接 cl.exe
```powershell
cd D:\_audio_player_x86\examples\wasapi_exclusive_demo
cl /std:c++17 /EHsc /utf-8 /W4 /DUNICODE /D_UNICODE main.cpp /link ole32.lib avrt.lib
.\main.exe
```
(需要先在 "x86 Native Tools Command Prompt for VS 2022" 中运行,以保证 32-bit 工具链)

## 常见问题

| 现象 | 原因 / 处理 |
|------|-------------|
| `[FAIL] IsFormatSupported: AUDCLNT_E_UNSUPPORTED_FORMAT` | 设备不支持 44.1 kHz 16-bit 也不支持 32-bit float。修改源码尝试 48 kHz 或 24-bit packed |
| `[FAIL] IAudioClient::Initialize: AUDCLNT_E_DEVICE_IN_USE` | 已有其他程序独占该设备(如 ASIO/foobar2000 WASAPI 输出)。关闭它们后再试 |
| `[FAIL] AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED` | 设备属性中关闭了"允许应用程序独占控制此设备"。在"声音 → 设备属性 → 高级"中勾选 |
| `[WARN] AvSetMmThreadCharacteristics failed` | MMCSS 服务未运行(罕见)。无碍,仅优先级未提升 |
| 听到的不是 1 kHz 而是其他频率/噪音 | 协商出了非预期格式,检查日志中 "Negotiated format" 一行 |

## 下一步

此 demo 将被重构为 `platform/wasapi/WasapiExclusiveOutput.{h,cpp}`:
- 抽到 `IAudioOutput` 接口后面
- 把渲染循环放进独立线程,通过 `DataCallback` 从环形缓冲区 pull 数据
- 增加错误恢复(设备拔出 / 重连)
- 支持设备指定(而非永远默认设备)
