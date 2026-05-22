// =============================================================================
//  app/controller/PlayerController.h
//
//  高层播放控制器:把 IDecoder + RingBuffer + IAudioOutput + DeviceEnumerator
//  封装成一个有完整状态机的"播放器对象"。UI 层只接触这个类。
//
//  状态机:
//
//      ┌─────────────── unloadFile() ───────────────────┐
//      ▼                                                │
//    Idle ──load()──▶ Stopped ──play()──▶ Playing       │
//                       ▲ ▲                │ │          │
//                       │ │      pause()   │ ▼          │
//                       │ └────────────────┼ Paused     │
//                       │   stop()         │ │          │
//                       │                  │ │ play()   │
//                       │                  │ ▼          │
//                       │                  Playing      │
//                       │                                │
//                       └──── stop() ──── Ended ◀── EOF │
//                                          │            │
//                                          └ play()(从头) ─┘
//
//  线程模型:
//    - 控制 API(loadFile/play/pause/stop/seek/setDevice/...)由"控制线程"
//      串行调用;内部用 ctrl_mutex 保证串行
//    - 内部 producer 线程:从 decoder 拉数据写入 RingBuffer
//    - 内部 monitor 线程:每 ~100ms 触发 positionChanged 回调,检测 EOF/错误
//    - WASAPI 渲染线程:由 WasapiExclusiveOutput 内部管理
//
//  回调线程:
//    - 所有 callback 都"由内部线程触发",UI 实现需自行投递到主线程
//      (例如 Qt:在回调中 emit signal,Qt 会跨线程转发)
// =============================================================================
#pragma once

#include "app/controller/PlayerState.h"
#include "core/format/AudioFormat.h"

#include <cstdint>
#include <functional>
#include <memory>
#include <string>

namespace apx {

class PlayerController {
public:
    PlayerController();
    ~PlayerController();

    PlayerController(const PlayerController&)            = delete;
    PlayerController& operator=(const PlayerController&) = delete;

    // ---------- 文件 ----------
    // 失败:回 Idle/Error,lastError() 含原因。
    bool loadFile(const std::wstring& path);
    void unloadFile();

    // ---------- 播放控制 ----------
    bool play();    // Stopped/Paused/Ended → Playing
    bool pause();   // Playing → Paused
    bool stop();    // 任何活动状态 → Stopped(位置归零)
    void setVolume(double volume); // 0.0 - 1.0, 在输出回调中做 PCM 缩放
    double volume() const;

    // 秒级 seek。当前为软 seek:清空 RingBuffer + decoder->seek,
    // 设备 buffer 内残留(~10ms)会先出声再听到新位置。
    bool seek(double seconds);

    // ---------- 设备 ----------
    // device_id 空 → 默认。当前实现:仅在 Stopped/Paused/Ended/Idle 时切换;
    // Playing 状态下会先 stop()。
    bool setDevice(const std::wstring& device_id);
    std::wstring currentDeviceId() const;
    std::wstring currentDeviceName() const;

    // ---------- 查询 ----------
    PlayerState  state()        const;
    double       position()     const;   // 秒
    double       duration()     const;   // 秒(无文件时返回 0)
    AudioFormat  format()       const;
    std::wstring currentFile()  const;
    std::wstring lastError()    const;

    // ---------- 事件回调 ----------
    using StateChangedCb    = std::function<void(PlayerState)>;
    using PositionChangedCb = std::function<void(double /*seconds*/)>;
    using EndedCb           = std::function<void()>;
    using ErrorCb           = std::function<void(const std::wstring&)>;
    // producer 线程 tap:每次从 decoder 拿到新 PCM 块时回调(在写入 RingBuffer 之前)。
    // 注意:在 producer 线程调用,实现需自行同步,且不可阻塞。
    using PcmTapCb          = std::function<void(const std::uint8_t* data,
                                                 std::size_t bytes,
                                                 const AudioFormat& fmt)>;
    // DSP 处理钩子:in-place 修改 PCM 数据,在 PcmTap 之后、写 RingBuffer 之前。
    // 调用者保证只在 producer 线程使用,自行做并发同步。
    using PcmProcessorCb    = std::function<void(std::uint8_t* data,
                                                 std::size_t bytes,
                                                 const AudioFormat& fmt)>;

    void setOnStateChanged   (StateChangedCb    cb);
    void setOnPositionChanged(PositionChangedCb cb);
    void setOnEnded          (EndedCb           cb);
    void setOnError          (ErrorCb           cb);
    void setOnPcmTap         (PcmTapCb          cb);
    void setPcmProcessor     (PcmProcessorCb    cb);

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
