<div align="center">

# 🗃️ SERAPH AUDIO ARCHIVE

**Premium Local HiFi Audio Player / 档案级本地高保真音频播放系统**

[![Rust](https://img.shields.io/badge/Rust-Core%20Engine-black?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-System%20Shell-black?style=for-the-badge&logo=tauri)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-Archive%20UI-black?style=for-the-badge&logo=react)](https://react.dev/)
[![WASAPI](https://img.shields.io/badge/Audio-WASAPI%20Exclusive-brown?style=for-the-badge)]()

*“摒弃历史包袱，以现代工程重塑纯粹的听觉档案。”*

</div>

---

## 📜 项目愿景

Seraph Audio Archive 是一款面向发烧友（Audiophile）的本地高保真音乐播放器。
它不仅在底层音频引擎上追求极致的 Bit-Perfect 与低延迟，更在 UI 设计上独树一帜，采用极具风格化的**“复古档案系统 (Archive System) / 打字机界面”**，旨在为用户提供一种沉浸式的音乐收藏与鉴赏体验。

本项目脱胎于传统的 Qt/C++ 播放器架构，全面迁移至 **Rust + Tauri + React** 的现代化技术栈，确保在未来 3~5 年内具备卓越的可维护性与扩展性。

## ✨ 核心特性

### 🎧 发烧级音频引擎
- **WASAPI Exclusive (独占模式)**：绕过 Windows 系统混音器，直接与音频硬件对话，确保零干扰输出。
- **无锁环形缓冲区 (Lock-free Ringbuffer)**：基于高性能 `ringbuf` 实现音频解码与渲染线程的极低延迟数据交换。
- **DSD 支持**：原生支持 DSD 格式解析与高精度 PCM 转换（未来规划支持 DoP 与 Native DSD）。
- **多解码器无缝回退**：首选 `Symphonia` 纯 Rust 极速解码，无缝回退至 `FFmpeg` 以兼容极度生僻的格式与 CUE 轨道分轨。

### 🗄️ 档案系统级交互
- **纯粹的视觉美学**：采用牛皮纸底色、等宽打字机字体（Courier Prime）、斑驳的档案纹理与印章红点缀，杜绝花哨，回归音乐本质。
- **动态终端体验**：独创的“打字机逐字敲击”歌词面板，伴随闪烁的光标，让每一句歌词如同正在被实时录入档案。
- **丝滑过渡**：全面整合 Framer Motion 与 Tailwind CSS 动画，提供极致流畅的路由与交互反馈。

---

## 🏗️ 架构概览

本项目采用典型的**“音频核心平台 + UI Shell”**的松耦合架构：

```text
Seraph Audio Archive
├── 📦 crates/ (Rust 核心层)
│   ├── seraph-core/        # 共享数据类型、领域模型与 EventBus
│   ├── seraph-audio/       # WASAPI 独占输出引擎、设备状态机
│   ├── seraph-dsp/         # 高精度重采样与 DSD 处理
│   ├── seraph-decoder/     # Symphonia / FFmpeg 多级解码调度
│   ├── seraph-playlist/    # 媒体库索引与播放列表管理
│   └── seraph-visualizer/  # 音频频域/时域分析与共享内存桥接
├── 🦀 src-tauri/ (系统壳层)
│   └── src/main.rs         # Tauri IPC 桥接、窗口管理与系统托盘
└── ⚛️ src/ (React 视图层)
    └── components/         # 纯粹的 UI 投影层 (UI Projection Layer)
```

## 🛠️ 本地开发指南

### 前置依赖
- Node.js (v18+)
- Rust & Cargo (最新 Stable 版本)
- Windows SDK (用于编译 WASAPI 依赖)

### 1. 安装依赖

```bash
npm install
```

### 2. 纯前端开发模式 (UI Mocking)
无需编译庞大的 Rust 核心，极速开发 UI 组件。所有的 IPC 调用将自动使用 Fallback 数据。

```bash
npm run dev
```

### 3. 全量桌面开发模式 (Tauri)
整合真实音频引擎运行。首次运行会触发 `cargo build`，可能需要几分钟。

```bash
npm run tauri:dev
```

### 4. 生产环境构建

```bash
npm run tauri:build
```

---

## 🗺️ 未来路线图 (Roadmap)

我们正以稳健的步伐推进 Seraph 架构的演进：

- [x] **Phase 1**: 完成基础设施重构 (React + Tauri + Rust 骨架)
- [x] **Phase 2**: WASAPI 独占输出与 SPSC 音频管线连通
- [x] **Phase 3**: 实现“档案系统”风格 UI 核心视觉（如打字机歌词组件）
- [ ] **Phase 4**: 完善 Bit-perfect PCM 路径（音量旁路、格式硬锁定）
- [ ] **Phase 5**: 设备插拔 (Device Lost) 恢复状态机
- [ ] **Phase 6**: 无缝播放 (Gapless Playback) 及 Sample-accurate Transition
- [ ] **Phase 7**: DSD 原生透传 (Native DSD) 与 DoP 支持

---

## 📜 协议

基于 [MIT License](LICENSE) 开源。欢迎各类 Issue 与 Pull Request 共同完善这座听觉档案库！
