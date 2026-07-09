# Seraph Audio Player

Premium local HiFi audio player built with Rust, Tauri, and React.

Seraph Audio Player 是一款面向本地高保真音乐播放的桌面播放器，目标是在 Windows 上提供低延迟、可控、稳定的本地音频播放体验。底层播放、设备与切歌状态由 Rust 后端负责，React 前端只作为 UI 投影层。

## 核心特性

- **Rust 音频后端**：播放状态、切歌、结束续播、上一首/下一首由 Rust 统一管理，减少前后端状态分叉。
- **WASAPI Exclusive**：支持 Windows WASAPI 独占输出，绕过系统混音路径。
- **多格式解码**：基于 Symphonia / FFmpeg 的多级解码路径，支持常见本地音频与部分流媒体缓存文件。
- **DSD/高采样率处理**：包含 DSD PCM 转换与重采样处理模块。
- **Bilibili 音频导入与缓存**：支持导入 Bilibili 音频并管理本地缓存。
- **缓存保护机制**：缓存目录写入 `.seraph-cache` 标记，清理时只处理受管理的缓存文件，降低误删风险。
- **状态一致性保护**：播放命令会等待 Rust 音频线程返回真实执行结果，再同步给前端 UI。
- **持久化迁移**：前端播放偏好使用版本化持久化状态，旧字段会在启动时自动迁移到当前结构。
- **收窄 Tauri 权限**：桌面壳只开启窗口控制、事件监听、拖放和打开文件对话框所需权限。
- **中英文 Windows 安装包**：Tauri 打包配置会生成英文/中文 MSI，并为 NSIS EXE 安装器启用语言选择。

## 架构概览

```text
Seraph Audio Player
├─ crates/
│  ├─ seraph-core/        # 共享事件、状态与领域类型
│  ├─ seraph-audio/       # 播放控制器、WASAPI/CPAL 输出、播放会话
│  ├─ seraph-decoder/     # Symphonia / FFmpeg / DSD 解码
│  ├─ seraph-dsp/         # 重采样与 DSD DSP
│  ├─ seraph-playlist/    # 播放列表与曲库模型
│  └─ seraph-visualizer/  # 频谱/可视化基础模块
├─ src-tauri/
│  └─ src/ipc/            # Tauri IPC、缓存、曲库、播放命令
└─ src/
   ├─ components/         # React UI
   ├─ hooks/              # 播放事件、拖放导入、波形等 hook
   └─ store/              # 前端 UI 状态与后端命令封装
```

## 播放状态机

当前播放队列、随机/循环模式、播放结束后的续播、`next_track` / `prev_track` 都由 Rust 后端处理。

前端负责：

- 同步当前队列快照到后端；
- 发送播放、暂停、上一首、下一首等命令；
- 监听后端 `TrackChanged`、`PlaybackStarted`、`PlaybackStopped`、`Progress` 等事件并更新 UI。

这样可以避免播放结束时前端自己推算下一首，导致 UI 状态和真实音频后端分叉。

## 缓存默认路径

缓存设置保存在应用数据目录的 `cache-settings.json`。已有设置文件的用户不会被自动迁移。

新用户首次启动时，默认缓存目录按以下优先级选择：

1. `<应用 exe 所在目录>/bilibili-cache`
2. `C:\ProgramData\Seraph Audio Player\bilibili-cache`
3. Tauri AppData 目录下的 `bilibili-cache`

每个候选路径都会尝试创建目录并写入 `.seraph-cache` 标记；如果不可写或不安全，会自动尝试下一个路径。

## 本地开发

### 环境要求

- Node.js 22+
- Rust stable
- Windows SDK / MSVC 工具链
- Tauri CLI 依赖由项目脚本调用

### 安装依赖

```bash
npm install
```

### 前端开发模式

```bash
npm run dev
```

### Tauri 桌面开发模式

```bash
npm run tauri:dev
```

### 类型检查与测试

```bash
npm run typecheck
npm test
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm audit --audit-level=low
```

这些检查也会在 GitHub Release 工作流中作为发布门禁执行。

## 构建安装包

只生成 Windows EXE 安装器：

```bash
npm run tauri -- build --bundles nsis
```

同时生成 Windows EXE 安装器和 MSI：

```bash
npm run tauri -- build --bundles nsis,msi
```

产物默认位于：

```text
target/release/bundle/nsis/
target/release/bundle/msi/
```

当前 Tauri 配置会生成：

- NSIS EXE 安装器；
- `en-US` MSI；
- `zh-CN` MSI。

## GitHub Release

仓库包含 `.github/workflows/release.yml`。推送 `v*` tag 时会触发 Windows release 构建，并发布安装包到 GitHub Release。

示例：

```bash
git tag v0.3.4
git push origin v0.3.4
```

## 许可证

MIT License
