# Seraph Audio Player — 项目架构设计

> Windows 平台 WASAPI 独占模式高保真音频播放器

---

## 一、项目概述

### 1.1 项目定位
面向 HiFi 发烧友和音频爱好者的 Windows 桌面音频播放器,核心目标是实现 **位完美 (Bit-Perfect)** 的音频回放,绕过 Windows 系统混音器,获得最纯净的音质输出。

### 1.2 目标用户
- 拥有外置 DAC / 解码器的音频爱好者
- 对系统重采样、混音引擎敏感的发烧友
- 需要播放高码率 PCM (24bit/192kHz 及以上) 和 DSD 文件的用户
- 偏好简洁、高性能本地播放器的用户

### 1.3 核心价值
| 特性 | 说明 |
|------|------|
| 独占输出 | 直接独占音频设备,绕过 Windows 音频引擎 |
| 位完美 | 不做任何重采样、音量调节、效果处理 |
| 采样率自适应 | 根据音源自动切换 DAC 输出采样率 |
| DSD 原生 | 支持 DoP / Native DSD 输出 |
| 低延迟 | 直通 WASAPI 缓冲区,延迟可控 |

---

## 二、设计目标

### 2.1 功能性目标
- [F1] 支持主流无损/有损格式: FLAC, WAV, AIFF, APE, ALAC, MP3, AAC, OGG, Opus
- [F2] 支持 DSD 格式: DSF, DFF (DSD64 / DSD128 / DSD256)
- [F3] WASAPI 独占模式输出,事件驱动 (Event-Driven)
- [F4] 设备枚举、热插拔检测、设备切换
- [F5] 采样率与位深度自动跟随音源
- [F6] 播放列表管理(创建、保存、加载 M3U/M3U8)
- [F7] CUE Sheet 解析与整轨播放
- [F8] 音频标签 (Tag) 读取与显示
- [F9] Gapless 无缝播放
- [F10] ReplayGain 信息显示(独占模式下不应用,仅显示)

### 2.2 非功能性目标
| 指标 | 目标值 |
|------|--------|
| 启动时间 | < 1.5s (冷启动) |
| 内存占用 | < 150MB (空载) |
| CPU 占用 | < 3% (16/44.1 播放) |
| 输出延迟 | 可配置 10ms ~ 200ms |
| 平台 | Windows 7 SP1 / 8.1 / 10 / 11 |
| 架构 | x86 (32-bit) 主目标,x64 备选 |

### 2.3 非目标 (Non-Goals)
- 不做音效处理 (EQ / 混响 / 空间音频)
- 不做流媒体服务集成 (Tidal / Qobuz 等)
- 不做跨平台 (仅 Windows)
- 不做音乐库管理 (以播放列表为中心,非媒体库为中心)

---

## 三、技术栈选型

### 3.1 核心技术栈

| 层 | 选型 | 理由 |
|----|------|------|
| 语言 | C++ 17 | 直接调用 Win32/COM API,性能最优 |
| GUI 框架 | Qt 6.5 LTS | 成熟、控件丰富、信号槽机制契合播放器事件模型 |
| 构建系统 | CMake 3.20+ | 跨编译器、跨 IDE 友好 |
| 编译器 | MSVC 2019/2022 | Windows 平台一等公民,COM 支持最好 |
| 音频 API | WASAPI (Core Audio) | Windows 原生,独占模式支持完善 |
| 包管理 | vcpkg | 微软维护,与 MSVC/CMake 集成顺畅 |

### 3.2 关键第三方库

| 库 | 用途 | 许可证 |
|----|------|--------|
| FFmpeg (libavcodec/libavformat) | 通用音频解码 (MP3/AAC/ALAC/OGG/Opus) | LGPL 2.1+ |
| libFLAC | FLAC 原生解码(独立于 FFmpeg,精度优先) | BSD |
| libsndfile | WAV / AIFF / W64 解码 | LGPL |
| TagLib | 音频元数据读取 | LGPL / MPL |
| libcue | CUE Sheet 解析 | GPL (考虑自研替代) |
| spdlog | 日志 | MIT |
| nlohmann/json | 配置文件序列化 | MIT |

> **DSD 处理**:DSF/DFF 自研解析器(格式简单,无需引入额外库)

---

## 四、整体架构

### 4.1 分层架构

```
┌─────────────────────────────────────────────────────────┐
│                    Presentation Layer                   │
│  (Qt Widgets: MainWindow, PlaylistView, DeviceDialog)   │
└──────────────────────────┬──────────────────────────────┘
                           │ Signals / Slots
┌──────────────────────────┴──────────────────────────────┐
│                    Application Layer                    │
│   PlayerController · PlaylistManager · SettingsManager  │
└──────────────────────────┬──────────────────────────────┘
                           │ Public API
┌──────────────────────────┴──────────────────────────────┐
│                       Core Layer                        │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌─────────┐  │
│  │  Decoder │→ │  Buffer  │→ │  Engine  │→ │ Output  │  │
│  │ Pipeline │  │  Queue   │  │ (Mixer-  │  │ Adapter │  │
│  │          │  │          │  │  Free)   │  │         │  │
│  └──────────┘  └──────────┘  └──────────┘  └────┬────┘  │
└──────────────────────────────────────────────────┼──────┘
                                                  │
┌─────────────────────────────────────────────────┴───────┐
│                     Platform Layer                      │
│        WASAPI (Exclusive Mode) · MMDevice API           │
│        COM Initialization · Device Notification         │
└─────────────────────────────────────────────────────────┘
```

### 4.2 模块清单

```
audio_player_x86
│
├── core/                       # 核心引擎(无 UI 依赖)
│   ├── decoder/                # 解码器抽象与各格式实现
│   ├── engine/                 # 播放引擎(状态机、调度)
│   ├── output/                 # 输出抽象与 WASAPI 实现
│   ├── buffer/                 # 环形缓冲区、生产者-消费者
│   ├── format/                 # 音频格式描述(采样率/位深/通道)
│   └── dsd/                    # DSD 处理(DoP 封装、原生输出)
│
├── platform/                   # Windows 平台代码
│   ├── wasapi/                 # WASAPI 独占模式封装
│   ├── mmdevice/               # 设备枚举与通知
│   └── com/                    # COM 初始化与智能指针
│
├── app/                        # 应用层
│   ├── controller/             # 播放控制器
│   ├── playlist/               # 播放列表管理
│   ├── settings/               # 配置管理
│   └── metadata/               # 元数据读取
│
├── ui/                         # Qt UI 层
│   ├── mainwindow/
│   ├── playlistview/
│   ├── devicedialog/
│   └── resources/              # 图标、样式表 (.qss)
│
├── third_party/                # vcpkg 不可用时的本地依赖
├── tests/                      # 单元测试与集成测试
├── docs/                       # 文档
├── scripts/                    # 构建脚本
├── CMakeLists.txt
├── vcpkg.json
└── README.md
```

---

## 五、核心技术方案

### 5.1 WASAPI 独占模式

**初始化流程**:
```
CoInitializeEx(COINIT_MULTITHREADED)
   ↓
MMDeviceEnumerator → 枚举 / 选择 IMMDevice
   ↓
IMMDevice::Activate(IAudioClient)
   ↓
IAudioClient::IsFormatSupported(SHARE_MODE_EXCLUSIVE, ...)
   ↓ 协商支持的 WAVEFORMATEXTENSIBLE
IAudioClient::Initialize(EXCLUSIVE, EVENTCALLBACK, period, ...)
   ↓
IAudioClient::SetEventHandle(hEvent)
   ↓
IAudioClient::GetService(IAudioRenderClient)
   ↓
启动渲染线程 → WaitForSingleObject(hEvent) → GetBuffer/ReleaseBuffer
```

**关键设计点**:
- 使用 **事件驱动模式 (AUDCLNT_STREAMFLAGS_EVENTCALLBACK)**,由音频驱动通知缓冲区可写,避免轮询
- 渲染线程提升为 **AVRT (Pro Audio)** 优先级,降低 glitch
- 缓冲区大小协商: 优先使用设备 `DefaultDevicePeriod` / `MinimumDevicePeriod`
- 格式协商失败时按 `WAVEFORMATEXTENSIBLE` 重试,KSDATAFORMAT_SUBTYPE_PCM / IEEE_FLOAT
- 设备占用失败 (`AUDCLNT_E_DEVICE_IN_USE`) 时给出明确提示

### 5.2 采样率自适应

播放器**不做任何重采样**,流程:
1. 解码器输出原始 PCM 流的格式描述 (sample_rate, bits, channels)
2. PlayerController 在切歌时调用 `WasapiOutput::reopen(format)`
3. 输出端关闭并重新协商设备格式
4. 若 DAC 不支持该格式 → UI 报错,跳过该曲目(用户可选"软件重采样降级",但默认关闭)

### 5.3 DSD 支持方案

**两种输出路径**:

| 模式 | 说明 | 实现 |
|------|------|------|
| DoP (DSD over PCM) | 将 DSD 流封装为 24bit PCM 帧 (0xFA/0x05 标记),WASAPI 以 PCM 形式传输,DAC 识别后解封装 | 标准方案,兼容性最佳 |
| Native DSD | 通过 ASIO 或特定驱动直接传输 DSD | v2.0 考虑,当前优先 DoP |

**DSD 缓冲区设计**:DSD 比特率与 PCM 不同,DSD64 ≈ 2.8MHz,需要独立的缓冲与节流策略。

### 5.4 缓冲区与线程模型

```
┌──────────────┐    ┌─────────────────┐    ┌──────────────┐
│ Decoder      │───▶│  Ring Buffer    │───▶│ Render       │
│ Thread       │    │  (lock-free)    │    │ Thread (AVRT)│
└──────────────┘    └─────────────────┘    └──────────────┘
     生产者              SPSC 队列                消费者
```

- **解码线程**: 普通优先级,从文件读取并解码到缓冲区
- **渲染线程**: AVRT "Pro Audio",从缓冲区拷贝到 WASAPI 缓冲区
- **环形缓冲区**: 单生产者单消费者 (SPSC),无锁,容量约 2~5 秒音频
- **背压机制**: 缓冲区满时解码线程阻塞,避免内存膨胀

### 5.5 Gapless 无缝播放

- 播放列表预读: 当前曲目剩余 < 3s 时,提前开始下一曲目的解码
- 同格式直接拼接: 若下一曲格式与当前一致,直接接到环形缓冲后段
- 格式变化时关闭并重开输出: 不可避免会有几十毫秒间隙,UI 中标记"格式切换"

### 5.6 音量控制策略

**独占模式下的音量哲学**: 默认不提供软件音量调节(任何 PCM 缩放都破坏位完美)。

提供三档可选策略 (Settings):
1. **Bit-Perfect** (默认): 软件音量禁用,固定 0dB,通过 DAC/功放调节音量
2. **Hardware Volume**: 若设备支持 `IAudioEndpointVolume`,调用硬件音量
3. **Software Volume**: 退化为 32-bit float 缩放(明确提示破坏位完美)

---

## 六、数据流(典型播放时序)

```
User 点击播放
     │
     ▼
PlayerController::play(track)
     │
     ├─▶ DecoderFactory::create(file_ext)         # 选择解码器
     │       │
     │       └─▶ FlacDecoder / FFmpegDecoder / DsdDecoder
     │
     ├─▶ 读取首帧获取 AudioFormat
     │
     ├─▶ WasapiOutput::open(format)               # 协商独占模式
     │       │
     │       ├─▶ IsFormatSupported 检查
     │       ├─▶ Initialize + SetEventHandle
     │       └─▶ 启动 RenderThread
     │
     ├─▶ DecoderThread 启动 → 持续写入 RingBuffer
     │
     └─▶ RenderThread 等待事件 → 从 RingBuffer 读 → WASAPI GetBuffer/ReleaseBuffer
              │
              └─▶ 缓冲区即将空 → 触发"曲目结束"事件 → 加载下一曲
```

---

## 七、关键接口设计(草案)

### 7.1 IDecoder
```cpp
class IDecoder {
public:
    virtual ~IDecoder() = default;
    virtual bool open(const std::wstring& path) = 0;
    virtual AudioFormat format() const = 0;
    virtual int64_t totalSamples() const = 0;
    virtual int64_t currentPosition() const = 0;
    virtual bool seek(int64_t sample_pos) = 0;
    // 读取一块 PCM/DSD 数据,返回写入字节数;0 表示 EOF
    virtual size_t read(uint8_t* dst, size_t bytes) = 0;
    virtual void close() = 0;
};
```

### 7.2 IAudioOutput
```cpp
class IAudioOutput {
public:
    virtual ~IAudioOutput() = default;
    virtual bool open(const AudioFormat& fmt, const DeviceId& dev) = 0;
    virtual void close() = 0;
    virtual void start() = 0;
    virtual void stop() = 0;
    virtual void pause(bool on) = 0;
    // 由内部 RenderThread 回调到上层 pull 数据
    using DataCallback = std::function<size_t(uint8_t* dst, size_t bytes)>;
    virtual void setDataCallback(DataCallback cb) = 0;
};
```

### 7.3 PlayerController (信号)
```cpp
signals:
    void stateChanged(PlayState state);        // Stopped/Playing/Paused
    void positionChanged(int64_t sample_pos);
    void trackChanged(const Track& track);
    void formatChanged(const AudioFormat& fmt);
    void errorOccurred(const QString& msg);
```

---

## 八、配置与持久化

**配置文件**: `%APPDATA%\AudioPlayerX86\config.json`

```json
{
  "output": {
    "device_id": "{0.0.0.00000000}.{...}",
    "exclusive_mode": true,
    "event_driven": true,
    "buffer_ms": 50,
    "dsd_mode": "DoP"
  },
  "volume": {
    "mode": "bit_perfect"
  },
  "playback": {
    "gapless": true,
    "auto_resample_fallback": false
  },
  "ui": {
    "theme": "dark",
    "remember_playlist": true
  }
}
```

**播放列表**: M3U8 (UTF-8) 标准格式,存于 `%APPDATA%\AudioPlayerX86\playlists\`

**日志**: `%APPDATA%\AudioPlayerX86\logs\player_YYYYMMDD.log`,按天滚动

---

## 九、错误处理与诊断

### 9.1 常见错误及处理
| 错误 | 处理 |
|------|------|
| AUDCLNT_E_DEVICE_IN_USE | 提示"设备被其他独占程序占用",列出当前占用进程 |
| AUDCLNT_E_UNSUPPORTED_FORMAT | 尝试常见替代格式,失败则跳过 |
| 设备拔出 (Device Removed) | 暂停播放,提示用户选择新设备 |
| 解码错误 | 跳到下一曲,记录日志 |
| 缓冲区欠载 (Glitch) | 日志记录,UI 显示"音频卡顿"指示灯 |

### 9.2 诊断面板
独立"诊断"窗口显示:
- 当前设备名 / 端点 ID
- 协商格式 (sample rate / bits / channels / mask)
- 当前缓冲区水位
- 累计 glitch 次数
- 渲染线程优先级
- WASAPI 报告的延迟 (`GetStreamLatency`)

---

## 十、开发路线图

### M1 — 骨架与最小可用 (4 周)
- 项目骨架 (CMake + Qt + vcpkg)
- WASAPI 独占模式封装 (单设备、单格式)
- WAV / FLAC 解码
- 最简 UI: 文件选择 + 播放/暂停/停止
- 日志系统

### M2 — 核心功能完整 (4 周)
- 解码器: MP3 / AAC / ALAC / APE / OGG (FFmpeg)
- 设备枚举与切换
- 采样率自适应
- 播放列表 (M3U/M3U8)
- 元数据 (TagLib)
- 进度条与拖动 seek

### M3 — 高级特性 (4 周)
- DSD (DSF/DFF) + DoP 输出
- CUE Sheet 整轨播放
- Gapless 无缝播放
- 诊断面板
- 配置持久化

### M4 — 打磨与发布 (3 周)
- UI 美化 (深色主题、图标)
- 安装包 (Inno Setup / WiX)
- 错误提示完善
- 性能调优
- v1.0 发布

### M5 — 后续规划
- ASIO 输出后端
- Native DSD
- WaveOut / DirectSound 后备输出
- 远程控制 (移动端)

---

## 十一、构建与部署

### 11.1 构建环境
```
Windows 10/11
Visual Studio 2022 (含 C++ 桌面开发 + ATL/MFC)
CMake 3.25+
Qt 6.5 LTS (msvc2019_32bit kit, x86)
vcpkg (latest)
```

### 11.2 构建步骤(预期)
```powershell
# 1. 依赖
vcpkg install ffmpeg:x86-windows-static libflac:x86-windows-static `
              libsndfile:x86-windows-static taglib:x86-windows-static `
              spdlog:x86-windows-static nlohmann-json:x86-windows-static

# 2. 配置
cmake -S . -B build -A Win32 `
      -DCMAKE_TOOLCHAIN_FILE=<vcpkg>/scripts/buildsystems/vcpkg.cmake `
      -DCMAKE_PREFIX_PATH=<Qt>/6.5.x/msvc2019_32

# 3. 编译
cmake --build build --config Release -j

# 4. 部署
windeployqt --release --no-translations build/bin/AudioPlayerX86.exe
```

### 11.3 发布产物
- `AudioPlayerX86_v1.0_x86_setup.exe` — 安装包 (~30MB)
- `AudioPlayerX86_v1.0_x86_portable.zip` — 绿色版 (~25MB)

---

## 十二、质量保障

### 12.1 测试策略
- **单元测试**: GoogleTest,覆盖 Decoder / Buffer / Format 模块
- **集成测试**: 离线脚本驱动,使用回环虚拟音频设备验证位完美
- **位完美验证**: 用已知校验和的 WAV/FLAC,通过 SPDIF/HDMI loopback 抓取并比对
- **手动测试矩阵**: Win 7/10/11 × 常见 USB DAC × 常见格式

### 12.2 性能基准
- 16/44.1 FLAC 播放 CPU < 3%
- 24/192 FLAC 播放 CPU < 5%
- DSD64 (DoP) CPU < 8%
- 启动到首音输出 < 2s

---

## 十三、风险与开放问题

| 风险 | 影响 | 缓解 |
|------|------|------|
| 部分 USB DAC 驱动对 WASAPI 独占支持不规范 | 设备无法独占 | 提供回退到共享模式选项 + 兼容性列表 |
| FFmpeg LGPL 动态链接合规 | 法律风险 | 动态链接 + LICENSE 声明,或替换为各格式独立库 |
| 32-bit 与某些新版 vcpkg 端口不兼容 | 构建失败 | 锁定 vcpkg baseline,必要时退到 x64 |
| DSD 文件格式繁多(DSF/DFF/不同位序) | 解码错误 | M3 阶段集中测试,准备样本库 |

---

## 十四、附录

### 14.1 参考资料
- Microsoft Learn — Core Audio APIs (WASAPI)
- foobar2000 — WASAPI Output Component (设计参考)
- HQPlayer / JRiver Media Center (产品参考)
- AES17 / EBU R128 (音频测量标准)

### 14.2 术语表
| 术语 | 含义 |
|------|------|
| WASAPI | Windows Audio Session API |
| Exclusive Mode | 独占模式,绕过系统混音器 |
| Bit-Perfect | 位完美,输出与源文件比特一致 |
| DoP | DSD over PCM,DSD 数据封装在 PCM 帧中传输 |
| AVRT | Multimedia Class Scheduler,多媒体线程调度 |
| Gapless | 无缝衔接,曲目间无静音间断 |
| MMCSS | Multimedia Class Scheduler Service |

---

*文档版本: v0.1 (架构初稿) · 最后更新: 2026-05-18*
