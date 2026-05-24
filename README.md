# AudioPlayerX86

> Windows 平台 WASAPI 独占模式高保真音频播放器

详细设计见 [ARCHITECTURE.md](./ARCHITECTURE.md)

## 编译产物

| 产物 | 路径 | 说明 |
|------|------|------|
| **AudioPlayerX86.exe** | `build_app\bin\Release\` | 主程序,Qt 6 Quick GUI,双击运行 |
| 各 example .exe(可选) | `build\bin\Release\` | 后端验证工具,默认不编译 |

## 快速开始(构建主程序)

### 1. 准备环境

- **Visual Studio 2022**(含 C++ 桌面开发)
- **CMake 3.20+**
- **Qt 6.5 LTS 或以上**,可用 `msvc2019_64` 或 `msvc2019_32`
  - 官方下载:<https://www.qt.io/download-open-source>

### 2. 一键构建

```powershell
# 设置 Qt 路径(只需一次)
$env:APX_QT_PATH = "C:\Qt\6.5.3\msvc2019_64"

# 编译 + 自动 windeployqt
.\scripts\build_app.ps1

# 编完直接跑
.\scripts\build_app.ps1 -Run
```

产物在 `build_app\bin\Release\AudioPlayerX86.exe`,所在目录已经包含 Qt DLLs,可整体拷贝到任意 Windows 机器运行。

## 支持的音频格式

### 容器与解码

| 格式 | 扩展名 | 实现 | 备注 |
|------|--------|------|------|
| **RIFF/WAVE** | `.wav` `.wave` | 内置 | PCM 16/24-packed/32、IEEE float32、WAVE_FORMAT_EXTENSIBLE |
| **RF64 (BWF)** | `.wav` | 内置 | 大于 4GB 的 WAV,含 `ds64` chunk |
| **Sony Wave64** | `.w64` | 内置 | 16-byte GUID chunk + 64-bit size,8-byte 对齐 |
| **FLAC** | `.flac` | dr_flac | 16 / 20 / 24 / 32 bit |
| **MP3** | `.mp3` | dr_mp3 | Int16 默认,可切 Float32 输出 |
| **OGG Vorbis** | `.ogg` `.oga` | stb_vorbis | 16-bit |
| **DSD (DSF)** | `.dsf` | 内置 | Sony 格式 → DoP 24-bit PCM |
| **DSD (DFF)** | `.dff` | 内置 | DSDIFF raw DSD(不含 DST 压缩) |
| AAC / M4A | `.aac` `.m4a` `.mp4` | 骨架 | 真实解码待接入 (fdk-aac / Media Foundation) |
| Opus | `.opus` | 骨架 | 真实解码待接入 (opusfile + libopus) |

文件识别同时使用扩展名和 magic-number 嗅探,扩展名错也能识别。

### 输出后端

| 后端 | 状态 |
|------|------|
| **WASAPI 独占** | ✅ 默认,事件驱动 + AVRT "Pro Audio" |
| **WASAPI 共享** | ✅ 独占失败时自动回退(线性插值重采样 + 位深转换) |
| **ASIO** | ⏳ 桩,用户放入 ASIO SDK 后可启用 |
| 原生 DSD(DSD-Native) | ⏳ 类型系统已就绪,需 DAC 驱动支持 |

### DoP 输出

DSD 解码默认走 **DoP v1.1**,输出格式 = `DSD_rate / 16` Hz,24-bit packed:
- DSD64  → 176400 Hz 24-bit
- DSD128 → 352800 Hz 24-bit
- DSD256 → 705600 Hz 24-bit

支持 `DopMarkerMode::PerFrame` (默认) 与 `PerSample` 两种 marker 策略,通过 `DsdDecoder::setMarkerMode` / `DffDecoder::setMarkerMode` 切换以适配不同 DAC。

## 功能特性

| 模块 | 状态 |
|------|------|
| 项目骨架 (CMake + vcpkg + Qt) | ✅ |
| WASAPI 独占模式(事件驱动 + AVRT) | ✅ |
| WASAPI 共享模式回退 + 线性重采样 | ✅ |
| 设备枚举 + 热插拔事件回调 + 自动会话恢复 | ✅ |
| RingBuffer(SPSC 无锁) | ✅ |
| PlayerController 状态机 | ✅ |
| 渲染线程错误回调(Error → 立即恢复) | ✅ |
| 预填充等待 ring 就绪(消除首段静音) | ✅ |
| MP3 / Vorbis 大文件 totalFrames 后台异步计算 | ✅ |
| MP3 可选 Float32 输出 | ✅ |
| FLAC / DSF / DFF 解码器 | ✅ |
| RF64 / Wave64 容器 | ✅ |
| **10 段 RBJ EQ** (默认禁用,接入 producer 链) | ✅ |
| **Visualizer** (VU + 16 段频谱) | ✅ |
| **ReplayGain** (Track/Album 模式 + 防 clipping) | ✅ FLAC 标签 |
| **Gapless** (预载下一首,格式一致时无缝衔接) | ✅ |
| **Playlist** 模型 (顺序/列表循环/单曲循环/随机) | ✅ |
| **CUE Sheet** 解析 (单文件多 track) | ✅ |
| Qt 6 Quick QML UI + 深色主题 | ✅ |
| 元数据读取 (WAV INFO / FLAC VORBIS_COMMENT + cover) | ✅ |
| 歌词读取 (LRC) | ✅ |
| 系统媒体控件 (SMTC) | ✅ |
| 任务栏缩略图按钮 | ✅ |
| AAC / Opus 真实解码 | ⏳ 骨架就绪 |
| 高质量重采样器 (SoXR / Speex) | ⏳ |
| 原生 DSD 输出 | ⏳ |

## 目录结构

```
audio_player_x86/
├── core/                       # 引擎(无 UI / 无 Win 头文件污染)
│   ├── format/                 #   AudioFormat (含 DsdLsb8 类型)
│   ├── buffer/                 #   RingBuffer (SPSC)
│   ├── output/                 #   IAudioOutput (+ ErrorCallback)
│   ├── decoder/                #   IDecoder + 各格式 decoder + DecoderFactory
│   ├── dsd/                    #   DopMode (PerFrame / PerSample)
│   ├── dsp/                    #   Equalizer / Visualizer / FormatConverter
│   ├── metadata/               #   MetadataReader (含 ReplayGain 字段)
│   └── lyrics/                 #   LyricsLoader (LRC)
├── platform/                   # Windows 平台
│   ├── wasapi/                 #   WasapiExclusiveOutput + WasapiSharedOutput
│   ├── mmdevice/               #   DeviceEnumerator (热插拔)
│   ├── asio/                   #   AsioOutput (桩)
│   ├── smtc/                   #   系统媒体传输控件
│   └── taskbar/                #   Windows 任务栏 / Jump List
├── app/                        # 应用层
│   ├── controller/             #   PlayerController + PlayerState
│   ├── playlist/               #   Playlist + CueSheet
│   ├── metadata/               #   元数据缓存
│   └── settings/               #   持久化设置
├── ui/                         # Qt 6 Quick / QML
│   ├── main.cpp
│   ├── bridge/                 #   PlayerViewModel (C++ <-> QML)
│   ├── qml/                    #   QML 界面
│   └── resources/
├── examples/                   # 后端 demo (默认不编)
├── scripts/
├── third_party/                # 单头/源码三方库
│   ├── dr_libs/                #   dr_flac.h / dr_mp3.h (后者自动下载)
│   └── stb/                    #   stb_vorbis.c (自动下载)
├── ARCHITECTURE.md
├── CMakeLists.txt
└── vcpkg.json
```

## 编译选项

| 选项 | 默认 | 说明 |
|------|------|------|
| `APX_BUILD_UI` | ON | Qt 主程序 `AudioPlayerX86.exe` |
| `APX_BUILD_APP` | ON | app 层(controller / playlist / metadata 等) |
| `APX_BUILD_EXAMPLES` | ON | 各 demo;主程序构建脚本中默认 OFF |
| `APX_BUILD_TESTS` | OFF | 单元测试 |

只想构建主程序、跳过 demos:`build_app.ps1` 已经这么做。

只想构建某个 demo、不需要 Qt:
```powershell
.\scripts\build_demo.ps1 -Target cli_player_demo -Run
```

## 关键 API 速查

### 加载 + 播放

```cpp
PlayerController pc;
pc.loadFile(L"D:/Music/album.flac");
pc.play();
```

### 切换设备(独占失败自动回退共享)

```cpp
pc.setAllowSharedFallback(true);        // 默认 true
pc.setDevice(L"{0.0.0.00000000}.{...}");  // 来自 DeviceEnumerator
```

### EQ / Visualizer

```cpp
pc.equalizer().setEnabled(true);
pc.equalizer().setGain(0, +6.0);         // 31 Hz +6 dB

auto snap = pc.visualizer().snapshot();   // {vu_left, vu_right, peak_*, bands[16]}
```

### ReplayGain

```cpp
auto md = MetadataReader::read(path);
if (md && !std::isnan(md->rg_track_gain_db)) {
    pc.setTrackReplayGain(md->rg_track_gain_db, md->rg_track_peak);
    pc.setReplayGainMode(PlayerController::ReplayGainMode::Track);
}
```

### Gapless

```cpp
pc.loadFile(L"track1.flac");
pc.play();
pc.enqueueNext(L"track2.flac");   // 必须与当前格式一致,否则失败
pc.setOnTrackChanged([](const std::wstring& p){ /* UI 更新 */ });
```

### Playlist + CUE

```cpp
Playlist pl;
auto tracks = CueSheet::parse(L"album.cue");
for (auto& t : tracks) pl.append(std::move(t));
pl.setMode(PlaybackMode::Sequential);
pc.loadFile(pl.itemAt(pl.currentIndex()).path);
```

## License

TBD
