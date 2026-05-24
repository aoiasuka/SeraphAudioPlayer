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
#include "core/dsd/DopMode.h"

#include <cstdint>
#include <functional>
#include <memory>
#include <string>

namespace apx {

class Equalizer;
class Visualizer;
struct PlaylistItem;

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

    // 加载一个 PlaylistItem。与 loadFile(item.path) 等效,但额外应用 Cue 区段:
    //   item.cue_start_sec > 0 → 自动 seek 到起点
    //   item.cue_end_sec   > cue_start → 到达 end 时 producer 视作 EOF
    // position()/duration()/seek() 之后都按"track 内坐标"工作 (从 0 计起)。
    bool loadItem(const PlaylistItem& item);

    // 预载下一首,实现无缝衔接。要求下一首的 AudioFormat 与当前完全一致,
    // 否则函数失败,gapless 不可达 (此时应在 onEnded 回调里走 load+play 的常规路径)。
    //  - 必须在当前会话 Playing/Paused/Stopped 状态调用
    //  - path 为空 → 清除已排队的下一首
    //  - 当前轨道 EOF 时,producer 自动切换到下一首,不停止 output,触发 onTrackChanged
    bool enqueueNext(const std::wstring& path);
    std::wstring queuedNext() const;

    // ---------- 播放控制 ----------
    bool play();    // Stopped/Paused/Ended → Playing
    bool pause();   // Playing → Paused
    bool stop();    // 任何活动状态 → Stopped(位置归零)
    void setVolume(double volume); // 0.0 - 1.0, 在输出回调中做 PCM 缩放
    double volume() const;

    // ---------- ReplayGain ----------
    enum class ReplayGainMode : std::uint8_t {
        Off    = 0,  // 不应用
        Track  = 1,  // 用 REPLAYGAIN_TRACK_GAIN
        Album  = 2,  // 用 REPLAYGAIN_ALBUM_GAIN
    };
    // 模式与 pre-amp (dB);Off 模式下 pre-amp 仍可作为基础增益使用,但不推荐。
    void           setReplayGainMode(ReplayGainMode m);
    ReplayGainMode replayGainMode() const;
    void           setReplayGainPreampDb(double db);
    double         replayGainPreampDb() const;

    // 应用当前轨道的 ReplayGain 标签值;loadFile 后由 UI 读 MetadataReader
    // 拿到 rg_*_gain_db / rg_*_peak,再调本方法。NaN 表示该字段不可用。
    // peak <= 0 视为 1.0 (无 clipping 保护)。
    void setTrackReplayGain(double gain_db, double peak);
    void setAlbumReplayGain(double gain_db, double peak);

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
    // Gapless 衔接到下一首时触发,参数是新轨道的 file path
    using TrackChangedCb    = std::function<void(const std::wstring& /*new_path*/)>;
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
    void setOnTrackChanged   (TrackChangedCb    cb);
    void setOnPcmTap         (PcmTapCb          cb);
    void setPcmProcessor     (PcmProcessorCb    cb);

    // ---------- 内置 DSP ----------
    // 10 段 EQ,默认禁用。返回的引用与本控制器同生命周期,UI 用 setGain/setEnabled
    // 配置即可,无需关心调用线程
    Equalizer& equalizer();

    // 内置可视化:producer 自动 push,UI 调 snapshot 即可
    Visualizer& visualizer();

    // ---------- 输出策略 ----------
    // 独占模式不支持源格式时,是否尝试共享模式 (低保真,但能出声)。
    // 默认开启;Hi-Fi 用户可关闭以保证"要么 bit-perfect 要么失败"。
    void setAllowSharedFallback(bool on);
    bool allowSharedFallback() const;

    // 共享路径专属设置 (独占模式不受影响)。Immediate:dither 切换立即生效,
    // highQuality 仅在下次 open 协商时生效。这两个 setter 即可在 Playing 中调用。
    void setSharedDither(bool on);
    bool sharedDither() const;
    void setSharedHighQuality(bool on);
    bool sharedHighQuality() const;

    // DSD → DoP 的 marker 模式。立即生效:有 DSF/DFF decoder 活动则同步给它,
    // 同时记录为偏好,下次打开 DSD 文件时会沿用。
    void setDopMarkerMode(DopMarkerMode mode);
    DopMarkerMode dopMarkerMode() const;

    // DSD 输出模式。
    //   ForceDoP    (默认): DSD decoder 输出 DoP 24-bit PCM, WASAPI 协商普通 PCM
    //   ForceNative      : DSD decoder 输出 raw LSB8 packed,WASAPI 协商 SUBTYPE_DSD
    //   Auto             : 先试 Native,失败回退 DoP (新文件加载时探测)
    // 实际能用 Native 取决于 DAC 在 WASAPI 端点是否暴露 DSD format;
    // 多数 USB DAC 走 ASIO Native,WASAPI Native 主要在专业声卡上可用。
    enum class DsdMode : std::uint8_t {
        ForceDoP    = 0,
        ForceNative = 1,
        Auto        = 2,
    };
    void    setDsdMode(DsdMode m);
    DsdMode dsdMode() const;

    // ---------- 渲染统计 ----------
    // 取实时快照;若无活动 output 则返回全 0。recovery_count 由 controller 维护,
    // 反映自加载以来发生过几次自动会话恢复。
    struct Stats {
        std::uint64_t periods_total  = 0;
        std::uint64_t frames_total   = 0;
        std::uint64_t underruns      = 0;
        std::uint64_t glitch_frames  = 0;
        std::uint64_t recovery_count = 0;
    };
    Stats stats() const;

private:
    struct Impl;
    std::unique_ptr<Impl> d_;
};

} // namespace apx
