# Seraph Audio Player

> Windows 平台 WASAPI 独占模式高保真音频播放器 (原名 AudioPlayerX86)

详细设计见 [ARCHITECTURE.md](./ARCHITECTURE.md)

当前版本:**v0.3.2-bugfix** (本地 tag)

## 编译产物

| 产物 | 路径 | 说明 |
|------|------|------|
| **SeraphAudioPlayer.exe** | `build_app\bin\Release\` | 主程序,Qt 6 Quick GUI,双击运行 |
| 各 example .exe(可选) | `build\bin\Release\` | 后端验证工具,默认不编译 |

## 快速开始(构建主程序)

### 1. 准备环境

- **Visual Studio 2022**(含 C++ 桌面开发)
- **CMake 3.20+**
- **Qt 6.5 LTS 或以上**,可用 `msvc2019_64` 或 `msvc2019_32`
  - 官方下载:<https://www.qt.io/download-open-source>
- **vcpkg**(可选):用于真实 Opus 解码,装 `opusfile`

### 2. 一键构建

```powershell
# 设置 Qt 路径(只需一次)
$env:APX_QT_PATH = "C:\Qt\6.5.3\msvc2019_64"

# 编译 + 自动 windeployqt
.\scripts\build_app.ps1

# 编完直接跑
.\scripts\build_app.ps1 -Run
```

产物在 `build_app\bin\Release\SeraphAudioPlayer.exe`,所在目录已经包含 Qt DLLs,可整体拷贝到任意 Windows 机器运行。

## 支持的音频格式

### 容器与解码

| 格式 | 扩展名 | 实现 | 备注 |
|------|--------|------|------|
| **RIFF/WAVE** | `.wav` `.wave` | 内置 | PCM 16/24-packed/32、IEEE float32、WAVE_FORMAT_EXTENSIBLE |
| **RF64 (BWF)** | `.wav` | 内置 | 大于 4GB 的 WAV,含 `ds64` chunk |
| **Sony Wave64** | `.w64` | 内置 | 16-byte GUID chunk + 64-bit size,8-byte 对齐 |
| **FLAC** | `.flac` | dr_flac | 16 / 20 / 24 / 32 bit + VORBIS_COMMENT + ReplayGain |
| **MP3** | `.mp3` | dr_mp3 | Int16 默认,可切 Float32 输出;ID3v2.2/3/4 + TXXX ReplayGain |
| **OGG Vorbis** | `.ogg` `.oga` | stb_vorbis | 16-bit |
| **Opus** | `.opus` | opusfile (vcpkg `opusfile`) | 装了即真实解码,未装则桩;通过 `APX_HAVE_OPUS` 条件编译 |
| **AAC / M4A / MP4** | `.aac` `.m4a` `.mp4` | Media Foundation Source Reader | Windows 原生,零外部依赖 |
| **DSD (DSF)** | `.dsf` | 内置 | Sony 格式 → DoP 24-bit PCM |
| **DSD (DFF)** | `.dff` | 内置 | DSDIFF raw DSD(不含 DST 压缩) |

文件识别同时使用扩展名和 magic-number 嗅探,扩展名错也能识别。

### 输出后端

| 后端 | 状态 |
|------|------|
| **WASAPI 独占** | ✅ 默认,事件驱动 + AVRT "Pro Audio" |
| **WASAPI 共享回退** | ✅ 独占失败时自动降级(多相 windowed-sinc 重采样 + TPDF dither + noise shaping) |
| **WASAPI Native DSD** | ✅ 协商 `KSDATAFORMAT_SUBTYPE_DSD`;decoder 输出 LSB8 raw,DAC 需在 WASAPI 端点暴露 DSD format |
| **ASIO** | ⚙️ 框架完整 + 注册表枚举可用;真实 open/start 需要用户提供 Steinberg ASIO SDK |
| 原生 DSD(DSD-Native, 独立后端) | ✅ 已并入 WASAPI 独占 (KSDATAFORMAT_SUBTYPE_DSD) |

### DoP 输出

DSD 解码默认走 **DoP v1.1**,输出格式 = `DSD_rate / 16` Hz,24-bit packed:
- DSD64  → 176400 Hz 24-bit
- DSD128 → 352800 Hz 24-bit
- DSD256 → 705600 Hz 24-bit

支持 `DopMarkerMode::PerFrame` (默认) 与 `PerSample` 两种 marker 策略,通过设置中心实时切换,无需重启。

### DSD 输出模式

`PlayerController::DsdMode` 三档,设置中心可改:

| 模式 | 行为 |
|------|------|
| **ForceDoP** (默认) | DSD decoder 输出 DoP 24-bit, WASAPI 协商普通 PCM —— 最广兼容 |
| **ForceNative** | DSD decoder 输出 raw LSB8, WASAPI 协商 `KSDATAFORMAT_SUBTYPE_DSD`;协商失败即报错 |
| **Auto** | 先尝试 Native,任意一步失败静默回落到 DoP |

## 功能特性

### 解码 / 元数据

| 模块 | 状态 |
|------|------|
| WAV (含 RF64 / Wave64) | ✅ |
| FLAC + VORBIS_COMMENT + ReplayGain | ✅ |
| MP3 + ID3v2.2/3/4 + TXXX ReplayGain | ✅ |
| OGG Vorbis | ✅ |
| **Opus** (opusfile,条件编译) | ✅ |
| **AAC / M4A / MP4** (Media Foundation) | ✅ |
| DSF / DFF → DoP | ✅ |
| 元数据读取 (WAV INFO / FLAC VC / MP3 ID3v2 + cover) | ✅ |
| **嵌入歌词** (MP3 SYLT/USLT、FLAC VC SYNCEDLYRICS/LYRICS、MP4 ©lyr) | ✅ |
| Cue Sheet 解析 (单文件多 track) | ✅ |
| **歌词读取** (LRC + offset + UTF-16/GBK 编码嗅探 + 翻译副行 + 词级 `<mm:ss.xx>` + ViewModel 缓存) | ✅ |

### 输出 / DSP

| 模块 | 状态 |
|------|------|
| WASAPI 独占 (事件驱动 + AVRT) | ✅ |
| WASAPI Native DSD 协商 (SUBTYPE_DSD) | ✅ |
| WASAPI 共享回退 + 高质量 SRC + dither | ✅ |
| **Polyphase 重采样器** (windowed-sinc 32 tap × 64 phase) | ✅ |
| **SIMD 派发** (AVX2 + SSE2 + scalar,运行时 CPUID) | ✅ |
| **TPDF Dither + 一阶 noise shaping** (Int16 路径,实时开关) | ✅ |
| **DoP Marker / DSD 输出模式 实时切换** (无需重启) | ✅ |
| 10 段 RBJ EQ (默认禁用,接入 producer 链) | ✅ |
| Visualizer (VU + 16 段频谱) | ✅ |
| ReplayGain (Track/Album + Pre-amp ±12dB + 防 clipping) | ✅ |

### 播放器 / 应用层

| 模块 | 状态 |
|------|------|
| PlayerController 状态机 | ✅ |
| RingBuffer (SPSC 无锁) | ✅ |
| 预填充等待 ring 就绪(消除首段静音) | ✅ |
| Gapless (预载下一首,格式一致时无缝衔接) | ✅ |
| Playlist 模型 (顺序 / 列表循环 / 单曲循环 / 随机) | ✅ |
| **Playlist M3U / JSON 序列化** (PlaylistIO) | ✅ |
| 设备枚举 + 热插拔事件 + 自动会话恢复 | ✅ |
| 渲染统计 (periods / frames / underruns / glitch / recovery) | ✅ |
| MP3 / Vorbis 大文件 totalFrames 后台异步计算 | ✅ |
| 错误回调 (Error → 立即恢复) | ✅ |

### UI

| 模块 | 状态 |
|------|------|
| Qt 6 Quick QML + 极简护眼主题 | ✅ |
| **PlaylistViewModel** (QAbstractListModel,12 个 role) | ✅ |
| **PlaylistView** (搜索 + 模式切换 + 上下移 + 右键菜单 + Cue 拖入 + M3U/JSON 导入导出) | ✅ |
| **QueueDrawer** (与 PlaylistView 共用同一 ListModel) | ✅ |
| **设置中心 · Hi-Fi 高级** (ReplayGain / Dither / DoP / DSD Mode / SIMD / 共享回退) | ✅ |
| **快捷键自定义** (23 action,录制式改键,QSettings 落盘) | ✅ |
| 系统媒体控件 (SMTC) | ✅ |
| 任务栏缩略图按钮 (canPrev/canNext 严格匹配模式) | ✅ |
| Jump List | ✅ |
| 设备热插拔事件订阅 | ✅ |
| 实时统计面板 (1Hz 刷新) | ✅ |

### 真机验证须知

| 模块 | 状态 |
|------|------|
| WASAPI Native DSD | 代码完整。能用与否取决于 DAC 在 WASAPI 端点是否暴露 `KSDATAFORMAT_SUBTYPE_DSD`;多数 USB DAC 在 Windows 走 ASIO native,这条路径主要在 RME 等专业声卡上有意义。Auto 模式可自动回落到 DoP |
| ASIO open/start | 注册表枚举已可用 (无 SDK 也能列驱动);真实 open/start/buffer-switch 需要用户把 Steinberg ASIO SDK 放进 `third_party/asiosdk/`,CMake 检测后自动编入 |

## 目录结构

```
audio_player_x86/
├── core/                       # 引擎(无 UI / 无 Win 头文件污染)
│   ├── format/                 #   AudioFormat (含 DsdLsb8 类型)
│   ├── buffer/                 #   RingBuffer (SPSC)
│   ├── output/                 #   IAudioOutput (+ ErrorCallback)
│   ├── decoder/                #   IDecoder + WAV/FLAC/MP3/Vorbis/DSF/DFF
│   │                           #     + Opus + AAC/M4A/MfMedia + Factory
│   ├── dsd/                    #   DopMode (PerFrame / PerSample)
│   ├── dsp/                    #   Equalizer / Visualizer / FormatConverter
│   │                           #     + PolyphaseResampler (+ AVX2/SSE2)
│   ├── metadata/               #   MetadataReader (WAV/FLAC/MP3 + ReplayGain
│   │                           #     + ID3v2 SYLT/USLT + FLAC VC + MP4 ©lyr 歌词)
│   └── lyrics/                 #   LyricsLoader (LRC + offset + 翻译 + 词级)
├── platform/                   # Windows 平台
│   ├── wasapi/                 #   WasapiExclusiveOutput + WasapiSharedOutput
│   ├── mmdevice/               #   DeviceEnumerator (热插拔)
│   ├── asio/                   #   AsioOutput (桩)
│   ├── smtc/                   #   系统媒体传输控件
│   └── taskbar/                #   Windows 任务栏 / Jump List
├── app/                        # 应用层
│   ├── controller/             #   PlayerController + PlayerState
│   ├── playlist/               #   Playlist + CueSheet + PlaylistIO (M3U/JSON)
│   ├── metadata/               #   元数据缓存
│   └── settings/               #   持久化设置
├── ui/                         # Qt 6 Quick / QML
│   ├── main.cpp
│   ├── bridge/                 #   PlayerViewModel + PlaylistViewModel
│   │                           #     + ShortcutsViewModel + DeviceBridge
│   │                           #     + CoverImageProvider
│   ├── qml/
│   │   ├── views/              #     HomeView / PlaylistView / SettingsView ...
│   │   └── components/         #     MiniPlayer / QueueDrawer / EqDialog
│   │                           #       / ShortcutsDialog ...
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
| `APX_BUILD_UI` | ON | Qt 主程序 `SeraphAudioPlayer.exe` |
| `APX_BUILD_APP` | ON | app 层(controller / playlist / metadata 等) |
| `APX_BUILD_EXAMPLES` | ON | 各 demo;主程序构建脚本中默认 OFF |
| `APX_BUILD_TESTS` | OFF | 单元测试 |
| `APX_HAVE_OPUS` | 探测 | 检测到 vcpkg `opusfile` 时自动定义,启用真实 Opus 解码 |

只想构建主程序、跳过 demos:`build_app.ps1` 已经这么做。

只想构建某个 demo、不需要 Qt:
```powershell
.\scripts\build_demo.ps1 -Target cli_player_demo -Run
```

## 键盘快捷键(默认)

| 类别 | 键 | 动作 |
|------|----|------|
| 播放 | `Space` / `Media Play` | 播放 / 暂停 |
| 播放 | `Left` / `Right` | 上一首 / 下一首 |
| 播放 | `Up` / `Down` | 音量 ±5 |
| 播放 | `M` | 静音切换 |
| 播放 | `Ctrl+L` | 喜欢 / 取消喜欢当前曲目 |
| 播放 | `Ctrl+R` | 循环模式切换 |
| 播放 | `Ctrl+S` | 随机切换 |
| 界面 | `Ctrl+Q` | 打开 / 收起队列抽屉 |
| 界面 | `Ctrl+Shift+Q` | 打开队列视图 (PlaylistView) |
| 界面 | `Ctrl+E` | 均衡器 |
| 界面 | `Ctrl+F` / `Ctrl+K` | 全局搜索 |
| 界面 | `F1` / `Ctrl+/` | 显示快捷键帮助 |
| 界面 | `F11` | 切换全屏 |
| 界面 | `Esc` | 返回 / 关闭抽屉 / 退出全屏 |
| 导航 | `1`..`7` | 首页 / 库 / 歌单 / 歌手 / 专辑 / 历史 / 喜欢 |
| 导航 | `Ctrl+,` | 设置 |

**全部可在 `F1` 弹窗里点击 chip 录制改键,改键即时生效,QSettings 落盘。**

## 关键 API 速查

### 加载 + 播放

```cpp
PlayerController pc;
pc.loadFile(L"D:/Music/album.flac");
pc.play();
```

### 切换设备(独占失败自动回退共享)

```cpp
pc.setAllowSharedFallback(true);          // 默认 true
pc.setDevice(L"{0.0.0.00000000}.{...}");  // 来自 DeviceEnumerator
```

### EQ / Visualizer

```cpp
pc.equalizer().setEnabled(true);
pc.equalizer().setGain(0, +6.0);          // 31 Hz +6 dB

auto snap = pc.visualizer().snapshot();   // {vu_left, vu_right, peak_*, bands[16]}
```

### ReplayGain + Pre-amp

```cpp
auto md = MetadataReader::read(path);
if (md && !std::isnan(md->rg_track_gain_db)) {
    pc.setTrackReplayGain(md->rg_track_gain_db, md->rg_track_peak);
    pc.setReplayGainMode(PlayerController::ReplayGainMode::Track);
    pc.setReplayGainPreampDb(+3.0);       // 在 RG 之上额外 +3 dB
}
```

### Dither / 共享回退 (运行时切换)

```cpp
pc.setSharedDither(true);                 // Int16 dst 时 TPDF + noise shape, 立即生效
pc.setSharedHighQuality(true);            // 多相 FIR vs 线性插值, 下次 open 时生效
pc.setAllowSharedFallback(false);         // "要么 bit-perfect, 要么报错"
```

### DSD 输出模式

```cpp
pc.setDopMarkerMode(DopMarkerMode::PerFrame);  // 立即同步给当前 decoder
pc.setDsdMode(PlayerController::DsdMode::Auto); // Native 失败回落 DoP
// 下次 loadFile 才协商;运行中切 mode 不打断当前播放
```

### Gapless

```cpp
pc.loadFile(L"track1.flac");
pc.play();
pc.enqueueNext(L"track2.flac");           // 必须与当前格式一致,否则失败
pc.setOnTrackChanged([](const std::wstring& p){ /* UI 更新 */ });
```

### Playlist + CUE + 导入导出

```cpp
Playlist pl;
auto tracks = CueSheet::parse(L"album.cue");
for (auto& t : tracks) pl.append(std::move(t));
pl.setMode(PlaybackMode::Sequential);

// M3U / JSON 序列化
PlaylistIO::saveM3U(pl,  L"D:/my.m3u8");
PlaylistIO::saveJson(pl, L"D:/my.json");

apx::Playlist loaded;
PlaylistIO::loadM3U(L"D:/my.m3u8", loaded);
```

### 渲染统计

```cpp
auto s = pc.stats();   // periods_total / frames_total / underruns
                       // glitch_frames / recovery_count
```

### 重采样 SIMD 路径

```cpp
// 启动后 CPUID 决定;只读,通常是 "avx2" / "sse2" / "scalar"
const char* path = PolyphaseResampler::simdPath();
```

## License

TBD
