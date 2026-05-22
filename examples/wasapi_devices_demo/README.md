# wasapi_devices_demo

验证 `DeviceEnumerator` 与热插拔通知。

## 行为

1. **列出所有渲染端点**(Active/Disabled/Unplugged/NotPresent),标记当前默认设备
2. **挂载监听器**,等待 30 秒
3. 在此期间插拔耳机、USB DAC、蓝牙音箱等,控制台会实时打印事件

## 运行

```powershell
.\build\bin\Release\wasapi_devices_demo.exe
```

## 典型输出

```
=== 当前渲染端点 ===
  [00] Active     Speakers (Realtek High Definition Audio)   (DEFAULT)
        id   = {0.0.0.00000000}.{a1b2c3d4-...}
  [01] Active     Topping DX3 Pro
        id   = {0.0.0.00000000}.{e5f6g7h8-...}
  [02] Unplugged  Headphones (Realtek...)
        id   = {0.0.0.00000000}.{...}

默认 id: {0.0.0.00000000}.{a1b2c3d4-...}

=== 监听 30 秒... ===
[EVT] StateChanged → Unplugged  id={0.0.0.00000000}.{e5f6g7h8-...}
[EVT] DefaultChanged(Console)   id={0.0.0.00000000}.{a1b2c3d4-...}
[EVT] DeviceAdded               id={0.0.0.00000000}.{new-guid-...}
[EVT] StateChanged → Active     id={0.0.0.00000000}.{e5f6g7h8-...}
```

## 在 play_wav demo 中使用查到的设备

```powershell
# 1) 列出
.\wasapi_play_wav_demo.exe --list-devices

# 2) 按 friendly name 子串选
.\wasapi_play_wav_demo.exe -d "Topping" "D:\Music\song.wav"

# 3) 按 id 精确选(复制 --list-devices 输出里的 id)
.\wasapi_play_wav_demo.exe -d "{0.0.0.00000000}.{e5f6g7h8-...}" "D:\Music\song.wav"
```

## 线程安全提示

`IDeviceChangeListener` 回调**由 MMDevice 内部线程调入**,与控制线程并发。本 demo 的 `PrintListener` 自带 `std::mutex` 保护 printf 顺序。生产代码中你可能希望把事件投递到主线程的消息队列处理,而不是在回调里直接做重活。
