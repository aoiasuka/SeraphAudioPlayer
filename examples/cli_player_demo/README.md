# cli_player_demo

交互式 CLI 播放器,完整验证 `PlayerController` 的状态机、回调、设备切换、seek。

这是 **C 阶段交付**,也是后续 Qt UI 接入前的最后一步纯后端验证。

## 启动

```powershell
.\build\bin\Release\cli_player_demo.exe
# 或带文件:
.\build\bin\Release\cli_player_demo.exe "D:\Music\song.wav"
```

## 命令清单

| 命令 | 说明 |
|------|------|
| `load <path>` | 加载 WAV 文件;可含空格,可加引号 |
| `unload` | 卸载,回到 Idle |
| `play` | Stopped / Paused / Ended → Playing |
| `pause` | Playing → Paused |
| `stop` | 停止并 seek 到 0,回到 Stopped |
| `seek <sec>` | 跳转,例如 `seek 30.5` |
| `device list` | 列出所有渲染设备,标记当前 active |
| `device default` | 切回默认设备 |
| `device <id\|name>` | 按 id 精确或按 name 子串切换 |
| `info` | 打印当前所有状态 |
| `progress on/off` | 切换 [POS] 周期打印 |
| `help` / `quit` | 帮助 / 退出 |

## 演示交互

```
> load D:\Music\test.wav
[ OK ] Loaded: D:\Music\test.wav
       Format: 96000 Hz, 2ch, 24/24-bit int24packed
       Duration: 245.33s
       Device: Topping DX3 Pro
[STATE]    Stopped

> play
[STATE]    Playing
[POS]        1.05s /  245.33s
[POS]        2.05s /  245.33s
...

> pause
[STATE]    Paused

> seek 120
[POS]      120.00s /  245.33s

> play
[STATE]    Playing

> device list
  [00] Active     Speakers (Realtek...)
  [01] Active     Topping DX3 Pro ← active
  [02] Unplugged  Headphones

> device "Speakers"
[ OK ] 设备切换为: Speakers (Realtek...)
[STATE]    Playing      ← 自动恢复播放

> stop
[STATE]    Stopped

> quit
```

## 关键验证点

- **状态机正确性**:每次操作后 `[STATE]` 行准确反映 PlayerController 内部状态
- **位置回报**:`[POS]` 估算 = `decoder.currentFrame - ring.readable / frame_bytes`,与人耳听到的位置误差仅设备 buffer (~10ms)
- **EOF 自动停止**:播到结尾自动发 `[ENDED]` + 转 `Ended`,再 `play` 从头开始
- **运行中切设备**:状态保持 Playing → close 旧 output → 用新设备重开 → 恢复播放,中间产生 ~80ms 静音是正常的(设备协商耗时)
- **seek 软语义**:seek 后会先听到设备 buffer 内残留(~10ms),再听到新位置 —— 独占模式无法立即 flush 设备 buffer

## 已知限制(后续阶段处理)

| 限制 | 触发条件 | 计划 |
|------|----------|------|
| 仅 WAV | `load` 非 .wav 文件 | M2:接入 FFmpeg / libFLAC |
| 暂无播放列表 | 自然 Ended 后停止 | 加 `Playlist` 类 |
| Console UTF-8 输入 | 非 ASCII 路径需先 `chcp 65001` | OK,操作系统级问题 |
| 切设备会有~80ms 静音 | `device <X>` 在 Playing 时 | 后续:无缝设备切换需双 output 交叉淡入淡出 |
