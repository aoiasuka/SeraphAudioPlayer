# Seraph Audio Player

Premium local HiFi audio player — Rust + Tauri + React.

参考 `初始化.md` 中确定的架构：放弃 Qt/C++，以 Rust 重写底层、Tauri 做系统壳、React 做 UI，WASAPI Exclusive + DSD 保留。

> 当前音频路径已接入：本地文件会经 Rust 解码、重采样/声道适配后进入无锁 ring buffer，再由系统共享输出或 WASAPI Exclusive 渲染线程消费。DSD 目前使用 PCM Conversion；DoP、Native DSD、ASIO 和 bit-perfect 旁路尚未开放。

## 技术栈

| 层 | 选型 |
|---|---|
| 系统壳 | Tauri v2 |
| 音频核心 | Rust workspace（多 crate） |
| 解码 | Symphonia 主解码 + FFmpeg CLI fallback |
| 重采样 | 窗口化 sinc 重采样（线性重采样保留为 fallback） |
| 输出 | WASAPI Exclusive / 系统共享输出（ASIO 尚未开放） |
| 前端 | React 18 + TypeScript + Vite |
| 样式 | Tailwind CSS v3 + shadcn/ui |
| 动画 | Framer Motion |
| 图标 | Lucide |
| 状态 | Zustand（UI 投影层） |

## 目录结构

```
crates/
  seraph-core/        共享类型 / 事件总线 / 状态机
  seraph-audio/       播放引擎 / 输出设备 / WASAPI 独占
  seraph-dsp/         重采样 / DSD 转换 trait
  seraph-decoder/     Symphonia / FFmpeg / DSD 解码
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

1. 完善 bit-perfect PCM 路径：音量旁路、格式锁定、避免不必要的 f32/重采样转换
2. DSD 分模式实现：PCM Conversion / DoP / Native DSD
3. Gapless 与 Device Lost 恢复
4. 将 WASAPI backend trait 与当前渲染 worker 收口
5. ASIO 输出路径
6. 更完整的真实音频格式回归测试

## 参考文件

- `初始化.md` —— 架构迁移方案讨论
- `synapse_hifi_music_player (1).html` —— UI 设计稿

## 最近更新

- **UI 优化**：为歌词面板（LyricsPanel）的当前播放行加入了匹配“档案系统”风格的**打字机逐字敲击效果**，并配有闪烁光标，提升了界面的动态生命力与复古氛围。
