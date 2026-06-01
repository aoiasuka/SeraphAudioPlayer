# Seraph Audio Player

Premium local HiFi audio player — Rust + Tauri + React.

参考 `初始化.md` 中确定的架构：放弃 Qt/C++，以 Rust 重写底层、Tauri 做系统壳、React 做 UI，WASAPI Exclusive + DSD 保留。

> 当前为 **骨架阶段**：UI 已完整还原 `synapse_hifi_music_player (1).html` 设计稿，Rust 侧只定义了 trait 与模块结构，实际音频路径尚未接通。

## 技术栈

| 层 | 选型 |
|---|---|
| 系统壳 | Tauri v2 |
| 音频核心 | Rust workspace（多 crate） |
| 解码 | symphonia + ffmpeg-next fallback（占位） |
| 重采样 | rubato（占位） |
| 输出 | WASAPI Exclusive（占位，AudioBackend trait 已定义） |
| 前端 | React 18 + TypeScript + Vite |
| 样式 | Tailwind CSS v3 + shadcn/ui |
| 动画 | Framer Motion |
| 图标 | Lucide |
| 状态 | Zustand（UI 投影层） |

## 目录结构

```
crates/
  seraph-core/        共享类型 / 事件总线 / 状态机
  seraph-audio/       AudioBackend trait + WASAPI 占位
  seraph-dsp/         Resampler / DsdConverter trait
  seraph-decoder/     Decoder trait + symphonia 占位
  seraph-playlist/    歌单 / 库
  seraph-visualizer/  FFT / shared ringbuffer
src-tauri/            Tauri shell + IPC bridge
src/                  React 前端
```

## 启动

### 安装依赖

```bash
npm install
```

### 浏览器开发模式（不需要 Rust）

```bash
npm run dev
```

打开 http://localhost:1420 即可看到 UI。所有 IPC 调用走 fallback（仅 console.log），用于纯前端迭代。

### Tauri 桌面模式

```bash
npm run tauri:dev
```

首次运行会触发 `cargo build`，需要等几分钟。

### 类型检查

```bash
npm run typecheck
```

### Rust 编译

```bash
cargo build
```

## 验证清单

- [ ] `cargo build` 通过
- [ ] `npm run typecheck` 通过
- [ ] `npm run dev` 浏览器看到完整 UI
- [ ] `npm run tauri:dev` 桌面窗口正常打开
- [ ] 视觉对照 `synapse_hifi_music_player (1).html` 一致

## 下一步路线

1. 实现 `WasapiExclusive` backend（独占模式 + Bit-Perfect）
2. 接 `symphonia` 解码 FLAC，跑通"打开本地文件 → 出声"
3. 把 mock playlist 换成本地文件扫描
4. AppDataDir 持久化（`%APPDATA%/com.seraph.audio/`）
5. DSD（DoP/Native/PCM 三种）
6. Gapless 与 Device Lost 恢复

## 参考文件

- `初始化.md` —— 架构迁移方案讨论
- `synapse_hifi_music_player (1).html` —— UI 设计稿
