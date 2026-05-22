# AudioPlayerX86

> Windows 平台 WASAPI 独占模式高保真音频播放器

详细设计见 [ARCHITECTURE.md](./ARCHITECTURE.md)

## 编译产物

| 产物 | 路径 | 说明 |
|------|------|------|
| **AudioPlayerX86.exe** | `build_app\bin\Release\` | 主程序,Qt 6 Widgets GUI,双击运行 |
| 各 example .exe(可选) | `build\bin\Release\` | 后端验证工具,默认不编译 |

## 快速开始(构建主程序)

### 1. 准备环境

- **Visual Studio 2022**(含 C++ 桌面开发)
- **CMake 3.20+**
- **Qt 6.5 LTS 或以上**,选 `msvc2019_32`(x86)套件
  - 官方下载:<https://www.qt.io/download-open-source>
  - 安装时勾选 `Qt 6.5.x → MSVC 2019 32-bit`

### 2. 一键构建

```powershell
# 设置 Qt 路径(只需一次)
$env:APX_QT_PATH = "C:\Qt\6.5.3\msvc2019_32"

# 编译 + 自动 windeployqt
.\scripts\build_app.ps1

# 编完直接跑
.\scripts\build_app.ps1 -Run
```

产物在 `build_app\bin\Release\AudioPlayerX86.exe`,所在目录已经包含 Qt DLLs,可整体拷贝到任意 Windows 机器运行。

### 3. UI 一览

```
┌─ Audio Player X86 ────────────── ─ □ × ┐
│ File   Playback   Help                 │
├────────────────────────────────────────┤
│   ♪ My Song.wav                        │
│     96000 Hz, 2ch, 24/24-bit int24...  │
│                                        │
│  [══════●═══════════]  01:23 / 04:05   │
│                                        │
│   [▶ 播放] [❚❚ 暂停] [■ 停止]  [📁]    │
│                                        │
│   输出设备: [ Topping DX3 Pro    ▼ ]   │
├────────────────────────────────────────┤
│ 播放中                                  │
└────────────────────────────────────────┘
```

## 当前状态(M1)

| 模块 | 状态 |
|------|------|
| 项目骨架 (CMake + vcpkg + Qt) | ✅ |
| WASAPI 独占模式(事件驱动 + AVRT) | ✅ |
| 设备枚举 + 热插拔通知 | ✅ |
| WAV 解码器(PCM 16/24/32 + IEEE_FLOAT) | ✅ |
| RingBuffer(SPSC 无锁) | ✅ |
| PlayerController 状态机 | ✅ |
| Qt 6 主窗口 + 深色主题 | ✅ |
| FLAC / MP3 解码器 | ⏳ M2 |
| DSD (DoP) 支持 | ⏳ M3 |
| 播放列表持久化 | ⏳ M2 |

## 目录结构

```
audio_player_x86/
├── core/                  # 引擎(解码 / 缓冲 / 输出抽象 / 格式)
│   ├── format/            #   AudioFormat
│   ├── buffer/            #   RingBuffer
│   ├── output/            #   IAudioOutput
│   └── decoder/           #   IDecoder + WavDecoder + DecoderFactory
├── platform/              # Windows 平台
│   ├── wasapi/            #   WasapiExclusiveOutput
│   └── mmdevice/          #   DeviceEnumerator + IMMNotificationClient
├── app/                   # 应用层(无 Qt 依赖)
│   └── controller/        #   PlayerController + PlayerState
├── ui/                    # Qt 6 Widgets 主程序
│   ├── main.cpp           #   入口
│   ├── mainwindow/        #   MainWindow + DeviceBridge
│   └── resources/         #   dark.qss + resources.qrc
├── examples/              # 后端 demo(默认不编)
│   ├── wasapi_exclusive_demo/
│   ├── wasapi_callback_demo/
│   ├── wasapi_play_wav_demo/
│   ├── wasapi_devices_demo/
│   └── cli_player_demo/
├── scripts/
│   ├── build_app.ps1      # 构建主程序
│   └── build_demo.ps1     # 构建 example
├── ARCHITECTURE.md
├── CMakeLists.txt
└── vcpkg.json
```

## 编译选项

| 选项 | 默认 | 说明 |
|------|------|------|
| `APX_BUILD_UI` | ON | Qt 主程序 `AudioPlayerX86.exe` |
| `APX_BUILD_APP` | ON | app 层(controller 等) |
| `APX_BUILD_EXAMPLES` | ON | 各 demo;主程序构建脚本中默认 OFF |
| `APX_BUILD_TESTS` | OFF | 单元测试(M3) |

只想构建主程序、跳过 demos:`build_app.ps1` 已经这么做。

只想构建某个 demo、不需要 Qt:
```powershell
.\scripts\build_demo.ps1 -Target cli_player_demo -Run
```

## License

TBD
