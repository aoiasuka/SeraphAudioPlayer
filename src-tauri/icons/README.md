# Tauri 图标占位

`npm run tauri:build` 需要以下图标文件：

- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- `icon.icns` (macOS)
- `icon.ico`  (Windows)
- `icon.png`

可以用 `npm run tauri icon path/to/source.png` 一键从单张 1024x1024 源图生成全套。

当前为骨架阶段，图标尚未生成；`tauri dev` 不需要图标也能跑，但 `tauri build` 会失败。
