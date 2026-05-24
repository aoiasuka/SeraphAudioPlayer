// =============================================================================
//  app/controller/PlayerController.cpp
// =============================================================================
#include "PlayerController.h"

#include "app/playlist/Playlist.h"
#include "core/buffer/RingBuffer.h"
#include "core/decoder/DecoderFactory.h"
#include "core/decoder/IDecoder.h"
#include "core/dsp/Equalizer.h"
#include "core/dsp/Visualizer.h"
#include "core/output/IAudioOutput.h"
#include "platform/mmdevice/DeviceEnumerator.h"
#include "platform/wasapi/WasapiExclusiveOutput.h"
#include "platform/wasapi/WasapiSharedOutput.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstring>
#include <cmath>
#include <limits>
#include <memory>
#include <mutex>
#include <sstream>
#include <thread>
#include <vector>

namespace apx {

namespace {
constexpr int kMonitorIntervalMs = 100;     // positionChanged 频率

template <typename T>
T clamp_sample(double v)
{
    const double lo = static_cast<double>(std::numeric_limits<T>::min());
    const double hi = static_cast<double>(std::numeric_limits<T>::max());
    return static_cast<T>(std::lround(std::clamp(v, lo, hi)));
}

void apply_volume(std::uint8_t* data, std::size_t bytes, const AudioFormat& fmt, double volume)
{
    if (!data || bytes == 0) return;
    // 近似单位增益直通(ReplayGain 可能让 volume > 1,所以两侧都判)
    if (volume >= 0.9995 && volume <= 1.0005) return;
    if (volume <= 0.0005) {
        std::memset(data, 0, bytes);
        return;
    }

    const std::size_t frame_bytes = fmt.frame_bytes();
    if (frame_bytes == 0) return;
    const std::size_t frames = bytes / frame_bytes;
    const std::size_t samples = frames * fmt.channels;

    switch (fmt.sample_type) {
    case SampleType::Int16: {
        auto* p = reinterpret_cast<std::int16_t*>(data);
        for (std::size_t i = 0; i < samples; ++i) {
            p[i] = clamp_sample<std::int16_t>(static_cast<double>(p[i]) * volume);
        }
        break;
    }
    case SampleType::Int24Packed: {
        for (std::size_t i = 0; i < samples; ++i) {
            std::uint8_t* s = data + i * 3;
            std::int32_t v = static_cast<std::int32_t>(s[0])
                | (static_cast<std::int32_t>(s[1]) << 8)
                | (static_cast<std::int32_t>(s[2]) << 16);
            if (v & 0x00800000) v |= static_cast<std::int32_t>(0xFF000000);
            v = static_cast<std::int32_t>(std::lround(std::clamp(
                static_cast<double>(v) * volume, -8388608.0, 8388607.0)));
            s[0] = static_cast<std::uint8_t>(v & 0xFF);
            s[1] = static_cast<std::uint8_t>((v >> 8) & 0xFF);
            s[2] = static_cast<std::uint8_t>((v >> 16) & 0xFF);
        }
        break;
    }
    case SampleType::Int32: {
        auto* p = reinterpret_cast<std::int32_t*>(data);
        for (std::size_t i = 0; i < samples; ++i) {
            p[i] = clamp_sample<std::int32_t>(static_cast<double>(p[i]) * volume);
        }
        break;
    }
    case SampleType::Float32: {
        auto* p = reinterpret_cast<float*>(data);
        for (std::size_t i = 0; i < samples; ++i) {
            p[i] = static_cast<float>(p[i] * volume);
        }
        break;
    }
    case SampleType::DsdLsb8:
        // 原生 DSD 位流不可直接做线性音量;若需要"软"音量控制需在 DAC 侧实现
        // (常规做法:转 PCM → 衰减 → 重新调制回 DSD。代价大,本播放器不做)。
        // 这里直通,UI 应把音量 slider 在 DSD 模式下置灰。
        break;
    }
}

// 先试独占,失败且 allow_shared_fallback 为真时回退共享。
// 失败时 err 累积两段错误信息,result 内容未定义。
// dither/high_quality 仅在走共享路径时应用。
std::unique_ptr<IAudioOutput> open_with_fallback(
    const AudioFormat& fmt, const OpenOptions& opts,
    OpenResult& result, std::wstring& err,
    bool shared_dither, bool shared_high_quality)
{
    auto exc = std::make_unique<wasapi::WasapiExclusiveOutput>();
    if (exc->open(fmt, opts, &result)) return exc;
    err = L"exclusive: " + exc->lastError();
    exc.reset();

    if (!opts.allow_shared_fallback) return nullptr;

    auto sh = std::make_unique<wasapi::WasapiSharedOutput>();
    sh->setDither(shared_dither);
    sh->setHighQuality(shared_high_quality);
    if (sh->open(fmt, opts, &result)) return sh;
    err += L"\nshared: " + sh->lastError();
    return nullptr;
}

} // namespace

// =============================================================================

struct PlayerController::Impl : public IDeviceChangeListener {
    // ---- 控制串行化 ----
    std::mutex                       ctrl_mutex;

    // ---- 资源 ----
    std::unique_ptr<IDecoder>                  decoder;
    std::unique_ptr<RingBuffer>                ring;
    std::unique_ptr<IAudioOutput>              output;   // 独占或共享,运行时决定
    DeviceEnumerator                           devices;
    bool                                       devices_listening = false;
    Equalizer                                  eq;            // 10 段 EQ,默认禁用
    Visualizer                                 viz;           // 可视化数据源(VU + 16 段频谱)

    // ---- 输出策略 ----
    std::atomic<bool> allow_shared_fallback{true};
    std::atomic<bool> shared_dither{true};
    std::atomic<bool> shared_high_quality{true};
    std::atomic<DopMarkerMode> dop_marker_mode{DopMarkerMode::PerFrame};
    std::atomic<PlayerController::DsdMode> dsd_mode{PlayerController::DsdMode::ForceDoP};

    // ---- 当前媒体 ----
    std::wstring  current_path;
    AudioFormat   current_fmt{};
    std::int64_t  total_frames = 0;
    std::uint32_t frame_bytes  = 0;
    double        buffer_ms    = 0.0;
    std::atomic<double> volume{1.0};

    // ---- Cue 区段:item 限定的播放范围(以源文件坐标系) ----
    // cue_start_frame: 用户 seek(0) 时实际定位的帧;duration 也基于此与 cue_end 计算
    // cue_end_frame  : INT64_MAX 表示无上限;producer 到达此帧即视为 EOF
    std::atomic<std::int64_t> cue_start_frame{0};
    std::atomic<std::int64_t> cue_end_frame{std::numeric_limits<std::int64_t>::max()};

    // ---- ReplayGain ----
    std::atomic<PlayerController::ReplayGainMode> rg_mode{
        PlayerController::ReplayGainMode::Off};
    std::atomic<double> rg_preamp_db{0.0};
    // 由 setTrackReplayGain/setAlbumReplayGain 写入,以原始 dB / 线性 peak 形式存放;
    // NaN 表示标签里没有这个字段。
    std::atomic<double> rg_track_gain_db{std::numeric_limits<double>::quiet_NaN()};
    std::atomic<double> rg_track_peak{std::numeric_limits<double>::quiet_NaN()};
    std::atomic<double> rg_album_gain_db{std::numeric_limits<double>::quiet_NaN()};
    std::atomic<double> rg_album_peak{std::numeric_limits<double>::quiet_NaN()};

    // 计算"当前 effective 增益" (线性,与 user volume 相乘)
    double effective_replaygain() const noexcept {
        const auto mode = rg_mode.load(std::memory_order_acquire);
        if (mode == PlayerController::ReplayGainMode::Off) {
            // pre-amp 仍允许在 Off 时叠加吗?约定:不允许,避免意外失真
            return 1.0;
        }
        const double gdb = (mode == PlayerController::ReplayGainMode::Album)
            ? rg_album_gain_db.load() : rg_track_gain_db.load();
        const double pk  = (mode == PlayerController::ReplayGainMode::Album)
            ? rg_album_peak.load()   : rg_track_peak.load();
        if (std::isnan(gdb)) return 1.0;     // 标签没值 → 不应用
        const double preamp_db = rg_preamp_db.load();
        double gain = std::pow(10.0, (gdb + preamp_db) / 20.0);
        // peak 防 clipping:若 gain * peak > 1.0,缩到 1/peak
        if (!std::isnan(pk) && pk > 0.0 && gain * pk > 1.0) {
            gain = 1.0 / pk;
        }
        return gain;
    }

    // ---- 当前设备 ----
    std::wstring  pending_device_id;        // setDevice() 之前缓存的目标 id
    std::wstring  active_device_id;
    std::wstring  active_device_name;

    // ---- 设备热插拔恢复 ----
    // listener 回调在 MMDevice 内部线程,只设置 flag;monitor 线程负责真正恢复
    std::atomic<bool> recovery_pending{false};
    int               recovery_failures = 0;     // 仅 monitor 访问
    std::atomic<std::uint64_t> recovery_total{0}; // 自加载以来的成功恢复次数

    // ---- 状态 ----
    std::atomic<PlayerState> state{PlayerState::Idle};
    mutable std::mutex       err_mutex;
    std::wstring             last_error;

    // ---- producer 线程 ----
    std::thread              prod_thread;
    std::mutex               prod_mutex;
    std::condition_variable  prod_cv;
    std::atomic<bool>        prod_running{false};
    std::atomic<bool>        prod_paused{false};
    std::atomic<bool>        prod_seek_pending{false};
    std::atomic<std::int64_t> seek_target_frame{0};
    std::atomic<bool>        decoder_eof{false};

    // ---- monitor 线程 ----
    std::thread              mon_thread;
    std::atomic<bool>        mon_running{false};

    // ---- 回调 ----
    std::mutex                       cb_mutex;
    StateChangedCb                   cb_state;
    PositionChangedCb                cb_pos;
    EndedCb                          cb_ended;
    ErrorCb                          cb_error;
    TrackChangedCb                   cb_track;
    PcmTapCb                         cb_pcm;
    PcmProcessorCb                   cb_proc;

    // ---- Gapless 预载 ----
    // next_decoder 由控制线程在 enqueueNext 中填充;producer 线程在当前 decoder
    // EOF 时把它拿过来替换 d_->decoder。访问受 next_mutex 保护。
    std::mutex                       next_mutex;
    std::unique_ptr<IDecoder>        next_decoder;
    std::wstring                     next_path;

    // ---- 实用 ----
    void set_error_msg(const std::wstring& m)
    {
        {
            std::lock_guard<std::mutex> lk(err_mutex);
            last_error = m;
        }
        // 错误回调
        ErrorCb cb;
        { std::lock_guard<std::mutex> lk(cb_mutex); cb = cb_error; }
        if (cb) cb(m);
    }
    std::wstring get_error() const
    {
        std::lock_guard<std::mutex> lk(err_mutex);
        return last_error;
    }
    void clear_error()
    {
        std::lock_guard<std::mutex> lk(err_mutex);
        last_error.clear();
    }

    void set_state(PlayerState s)
    {
        const PlayerState old = state.exchange(s, std::memory_order_acq_rel);
        if (old == s) return;
        StateChangedCb cb;
        { std::lock_guard<std::mutex> lk(cb_mutex); cb = cb_state; }
        if (cb) cb(s);
    }

    void fire_position(double sec)
    {
        PositionChangedCb cb;
        { std::lock_guard<std::mutex> lk(cb_mutex); cb = cb_pos; }
        if (cb) cb(sec);
    }
    void fire_ended()
    {
        EndedCb cb;
        { std::lock_guard<std::mutex> lk(cb_mutex); cb = cb_ended; }
        if (cb) cb();
    }
    void fire_track_changed(const std::wstring& p)
    {
        TrackChangedCb cb;
        { std::lock_guard<std::mutex> lk(cb_mutex); cb = cb_track; }
        if (cb) cb(p);
    }

    // 当前播放位置(以已被设备消费的帧数估算)。Cue 模式下减去起点偏移。
    double estimated_position_sec() const
    {
        if (!decoder || frame_bytes == 0 || current_fmt.sample_rate == 0) return 0.0;
        const std::int64_t decoded_frames = decoder->currentFrame();
        const std::int64_t in_ring_frames = ring
            ? static_cast<std::int64_t>(ring->readable() / frame_bytes)
            : 0;
        const std::int64_t played = decoded_frames - in_ring_frames
                                  - cue_start_frame.load(std::memory_order_acquire);
        const std::int64_t clamped = (played < 0) ? 0 : played;
        return static_cast<double>(clamped) / current_fmt.sample_rate;
    }

    // 释放当前会话(decoder/output/ring/线程)
    void teardown_session()
    {
        // 取消设备 listener,防止恢复线程在关闭过程中又被触发
        if (devices_listening) {
            devices.unregisterListener();
            devices_listening = false;
        }
        recovery_pending.store(false);
        recovery_failures = 0;

        // 停 monitor
        mon_running.store(false, std::memory_order_release);
        if (mon_thread.joinable()) mon_thread.join();

        // 停 producer
        prod_running.store(false, std::memory_order_release);
        prod_paused.store(false);
        prod_cv.notify_all();
        if (prod_thread.joinable()) prod_thread.join();

        if (output) { output->stop(); output->close(); output.reset(); }
        if (decoder) { decoder->close(); decoder.reset(); }
        ring.reset();

        // 丢弃任何已排队的下一首
        {
            std::lock_guard<std::mutex> lk(next_mutex);
            if (next_decoder) { next_decoder->close(); next_decoder.reset(); }
            next_path.clear();
        }

        current_path.clear();
        current_fmt = {};
        total_frames = 0;
        frame_bytes  = 0;
        buffer_ms    = 0.0;
        active_device_id.clear();
        active_device_name.clear();
        decoder_eof.store(false);
        prod_seek_pending.store(false);
        cue_start_frame.store(0);
        cue_end_frame.store(std::numeric_limits<std::int64_t>::max());
        recovery_total.store(0);
    }

    // 启动 producer + monitor 线程(只调一次,在 loadFile 成功后)
    void start_workers();
    void producer_loop();
    void monitor_loop();

    // 在 monitor 线程里执行的会话恢复 (output 进入 Error 或设备热插拔时)
    void try_recovery();

    // 阻塞等待 ring 至少装入 threshold_ms 等价的数据,或 decoder EOF,或超时。
    // 用于 play()/setDevice()/recovery 启动 output 前的预热,避免首段静音。
    bool wait_for_ring(double threshold_ms, int timeout_ms);

    // ---- IDeviceChangeListener ----
    // 注意:以下回调在 MMDevice 内部线程触发;只做轻量的 flag 设置
    void onDeviceStateChanged(const std::wstring& id, DeviceState new_state) override {
        // 当前正在使用的设备进入非 Active → 安排恢复
        if (!active_device_id.empty() && id == active_device_id
            && new_state != DeviceState::Active) {
            recovery_pending.store(true, std::memory_order_release);
        }
    }
    void onDeviceRemoved(const std::wstring& id) override {
        if (!active_device_id.empty() && id == active_device_id) {
            recovery_pending.store(true, std::memory_order_release);
        }
    }
    void onDefaultDeviceChanged(const std::wstring& id, DefaultRole role) override {
        // 仅当用户没有显式选择设备(pending_device_id 为空)时,跟随默认设备变化
        if (!pending_device_id.empty()) return;
        if (!has_role(role, DefaultRole::Console)) return;
        if (id.empty() || id == active_device_id) return;
        recovery_pending.store(true, std::memory_order_release);
    }
};

// -----------------------------------------------------------------------------

void PlayerController::Impl::start_workers()
{
    prod_running.store(true, std::memory_order_release);
    prod_paused.store(true, std::memory_order_release);    // Loaded 默认暂停
    decoder_eof.store(false);
    prod_thread = std::thread([this]{ producer_loop(); });

    mon_running.store(true, std::memory_order_release);
    mon_thread = std::thread([this]{ monitor_loop(); });
}

void PlayerController::Impl::producer_loop()
{
    constexpr std::size_t kBatch = 16 * 1024;
    std::vector<std::uint8_t> buf(kBatch);

    while (prod_running.load(std::memory_order_acquire)) {
        // 暂停处理
        {
            std::unique_lock<std::mutex> lk(prod_mutex);
            prod_cv.wait(lk, [&]{
                return !prod_running.load() ||
                       (!prod_paused.load() && !prod_seek_pending.load());
            });
        }
        if (!prod_running.load()) break;

        // seek 处理(seek 时主线程已置 paused=true 并清空 ring)
        if (prod_seek_pending.exchange(false)) {
            const std::int64_t target = seek_target_frame.load();
            if (decoder) decoder->seek(target);
            decoder_eof.store(false);
            eq.reset();    // 清 biquad 历史样本,避免跨段产生瞬态
            viz.reset();
            // 仍由主线程统一恢复 paused 状态,这里不主动 resume
            continue;
        }

        if (decoder_eof.load()) {
            // EOF 后保持线程存活,等用户 seek/stop 唤醒
            std::this_thread::sleep_for(std::chrono::milliseconds(20));
            continue;
        }

        // 检查 ring 是否有空间
        if (!ring) { std::this_thread::sleep_for(std::chrono::milliseconds(5)); continue; }
        std::size_t free_bytes = ring->writable();
        if (free_bytes < frame_bytes) {
            std::this_thread::sleep_for(std::chrono::milliseconds(2));
            continue;
        }
        std::size_t want = std::min(free_bytes, kBatch);
        want -= (want % frame_bytes);
        if (want == 0) { std::this_thread::sleep_for(std::chrono::milliseconds(2)); continue; }

        if (!decoder) break;

        // Cue 区段:到达 end 之前可能还要读最后一小段,read 后再裁
        const std::int64_t cue_end = cue_end_frame.load(std::memory_order_acquire);
        const std::int64_t cur_dec = decoder->currentFrame();
        if (cur_dec >= cue_end) {
            // 触发 gapless 切换或常规 EOF;走与 got==0 同一路径
            std::unique_ptr<IDecoder> swap;
            std::wstring              swap_path;
            {
                std::lock_guard<std::mutex> lk(next_mutex);
                if (next_decoder && next_decoder->format() == current_fmt) {
                    swap = std::move(next_decoder);
                    swap_path = next_path;
                    next_path.clear();
                }
            }
            if (swap) {
                if (decoder) decoder->close();
                decoder = std::move(swap);
                {
                    std::lock_guard<std::mutex> lk(next_mutex);
                    current_path = swap_path;
                }
                cue_start_frame.store(0);
                cue_end_frame.store(std::numeric_limits<std::int64_t>::max());
                decoder_eof.store(false, std::memory_order_release);
                eq.reset();
                viz.reset();
                fire_track_changed(swap_path);
                continue;
            }
            decoder_eof.store(true, std::memory_order_release);
            continue;
        }
        // 不要读过 cue_end 一帧:把 want 截到 (cue_end - cur_dec) * frame_bytes
        const std::int64_t cue_room = cue_end - cur_dec;
        if (cue_room < std::numeric_limits<std::int64_t>::max()
            && static_cast<std::int64_t>(want / frame_bytes) > cue_room) {
            want = static_cast<std::size_t>(cue_room) * frame_bytes;
            if (want == 0) {
                decoder_eof.store(true, std::memory_order_release);
                continue;
            }
        }

        const std::size_t got = decoder->read(buf.data(), want);
        if (got == 0) {
            // 当前 decoder EOF。若已排队下一首且格式匹配,无缝切换 → gapless。
            std::unique_ptr<IDecoder> swap;
            std::wstring              swap_path;
            {
                std::lock_guard<std::mutex> lk(next_mutex);
                if (next_decoder && next_decoder->format() == current_fmt) {
                    swap = std::move(next_decoder);
                    swap_path = next_path;
                    next_path.clear();
                }
            }
            if (swap) {
                if (decoder) decoder->close();
                decoder = std::move(swap);
                {
                    std::lock_guard<std::mutex> lk(next_mutex);
                    current_path = swap_path;
                }
                decoder_eof.store(false, std::memory_order_release);
                eq.reset();
                viz.reset();
                fire_track_changed(swap_path);
                continue;
            }
            decoder_eof.store(true, std::memory_order_release);
            continue;
        }
        // DSP 链:先内置 EQ(默认禁用,enable 后才耗 CPU),再用户 processor;
        // 顺序保证 tap / Visualizer 看到的是最终送到 ring 的声音
        if (eq.enabled()) eq.process(buf.data(), got, current_fmt);
        {
            PcmProcessorCb proc;
            { std::lock_guard<std::mutex> lk(cb_mutex); proc = cb_proc; }
            if (proc) proc(buf.data(), got, current_fmt);
        }
        // 喂可视化(内部 mutex,UI 线程可随时取 snapshot)
        viz.push(buf.data(), got, current_fmt);
        // PCM tap (用于额外的可视化等)
        {
            PcmTapCb tap;
            { std::lock_guard<std::mutex> lk(cb_mutex); tap = cb_pcm; }
            if (tap) tap(buf.data(), got, current_fmt);
        }
        std::size_t written = 0;
        while (written < got && prod_running.load(std::memory_order_acquire)) {
            const std::size_t w = ring->write(buf.data() + written, got - written);
            written += w;
            if (w == 0) std::this_thread::sleep_for(std::chrono::milliseconds(2));
        }
    }
}

void PlayerController::Impl::monitor_loop()
{
    while (mon_running.load(std::memory_order_acquire)) {
        std::this_thread::sleep_for(std::chrono::milliseconds(kMonitorIntervalMs));
        const PlayerState s = state.load();

        // 设备热插拔 / 默认设备变化 → 尝试恢复;
        // output 进入 Error 也走同一路径(常见原因之一就是设备失效)
        const bool out_err = output && output->state() == OutputState::Error;
        if (recovery_pending.exchange(false) || out_err) {
            try_recovery();
            continue;
        }

        if (s == PlayerState::Playing) {
            fire_position(estimated_position_sec());

            // EOF + ring 排干 → Ended
            if (decoder_eof.load() && ring && ring->readable() == 0) {
                // 给设备 buffer 一点时间把尾帧播完
                std::this_thread::sleep_for(
                    std::chrono::milliseconds(static_cast<int>(buffer_ms) + 30));
                if (output) output->stop();
                prod_paused.store(true);
                set_state(PlayerState::Ended);
                fire_ended();
                continue;
            }
        }
    }
}

// =============================================================================
// 设备热插拔 / 默认设备变化 / output Error → 重建会话
// 在 monitor 线程上执行,串行 + 失败上限保护
// =============================================================================

bool PlayerController::Impl::wait_for_ring(double threshold_ms, int timeout_ms)
{
    if (!ring || !current_fmt.valid() || frame_bytes == 0) return false;
    const std::size_t bps = current_fmt.bytes_per_second();
    if (bps == 0) return false;
    const std::size_t need =
        static_cast<std::size_t>(static_cast<double>(bps) * threshold_ms / 1000.0);
    const auto deadline = std::chrono::steady_clock::now()
                        + std::chrono::milliseconds(timeout_ms);
    for (;;) {
        if (ring->readable() >= need) return true;
        if (decoder_eof.load(std::memory_order_acquire)) {
            // 短文件直接 EOF,也算 "已就绪"
            return ring->readable() > 0;
        }
        if (std::chrono::steady_clock::now() >= deadline) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }
    return ring->readable() >= need;
}

void PlayerController::Impl::try_recovery()
{
    // ctrl_mutex 防止与用户线程的 loadFile/setDevice/seek 并发
    std::unique_lock<std::mutex> lk(ctrl_mutex, std::try_to_lock);
    if (!lk.owns_lock()) {
        // 用户线程正在改状态;让位,下次 tick 再来
        recovery_pending.store(true, std::memory_order_release);
        return;
    }
    if (!decoder || !current_fmt.valid()) return;        // 无活动会话
    if (recovery_failures >= 5) {
        // 连续失败上限:停止尝试,避免无限循环占线
        set_error_msg(L"device recovery aborted (too many failures)");
        set_state(PlayerState::Error);
        return;
    }

    const PlayerState s_before = state.load();
    const bool was_playing = (s_before == PlayerState::Playing);
    const double pos = estimated_position_sec();

    if (output) { output->stop(); output->close(); }
    prod_paused.store(true, std::memory_order_release);
    if (ring) ring->clear();

    OpenOptions opts;
    opts.device_id = pending_device_id;       // 空 → 默认;非空 → 严格 id
    opts.allow_shared_fallback = allow_shared_fallback.load();
    OpenResult result{};
    std::wstring open_err;
    auto new_out = open_with_fallback(current_fmt, opts, result, open_err,
                                      shared_dither.load(std::memory_order_acquire),
                                      shared_high_quality.load(std::memory_order_acquire));
    if (!new_out) {
        recovery_failures += 1;
        set_error_msg(L"device recovery: WASAPI open failed:\n  " + open_err);
        // 下次 tick 继续重试,直到达到上限
        recovery_pending.store(true, std::memory_order_release);
        std::this_thread::sleep_for(std::chrono::milliseconds(200));
        return;
    }
    RingBuffer* ring_ptr = ring.get();
    auto* impl = this;
    new_out->setDataCallback([ring_ptr, impl](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
        const std::size_t got = ring_ptr->read(dst, bytes);
        const double gain = impl->volume.load(std::memory_order_relaxed)
                          * impl->effective_replaygain();
        apply_volume(dst, got, impl->current_fmt, gain);
        return got;
    });
    new_out->setErrorCallback([impl](const std::wstring&) {
        // 渲染线程进 Error → 立即让 monitor 启动恢复
        impl->recovery_pending.store(true, std::memory_order_release);
    });

    output             = std::move(new_out);
    active_device_id   = result.device_id;
    active_device_name = result.device_name;
    buffer_ms          = result.buffer_ms;

    // 回到原播放位置(Cue 模式下 pos 已经是 track 内坐标,要补 cue_start)
    const std::int64_t target =
        static_cast<std::int64_t>(pos * current_fmt.sample_rate)
        + cue_start_frame.load();
    if (decoder) decoder->seek(target);
    decoder_eof.store(false);

    if (was_playing) {
        prod_paused.store(false, std::memory_order_release);
        prod_cv.notify_all();
        // 等 ring 至少装满一个设备 buffer 再 Start,避免首段静音
        wait_for_ring(buffer_ms, 500);
        if (!output->start()) {
            recovery_failures += 1;
            set_error_msg(L"device recovery: output start failed: " + output->lastError());
            recovery_pending.store(true, std::memory_order_release);
            return;
        }
        set_state(PlayerState::Playing);
    } else {
        set_state(PlayerState::Stopped);
    }
    recovery_failures = 0;     // 一次成功就清零
    recovery_total.fetch_add(1, std::memory_order_release);
}

// =============================================================================
// 公共 API
// =============================================================================

PlayerController::PlayerController()
    : d_(std::make_unique<Impl>())
{
}

PlayerController::~PlayerController()
{
    unloadFile();
}

PlayerState  PlayerController::state()       const { return d_->state.load(); }
double       PlayerController::position()    const { return d_->estimated_position_sec(); }
double       PlayerController::duration()    const {
    if (!d_->decoder || d_->current_fmt.sample_rate == 0) return 0.0;
    // 每次都向 decoder 询问 total_frames:VBR MP3/OGG 在后台扫描完成后会更新
    std::int64_t n = d_->decoder->totalFrames();
    // 应用 Cue 区段限制
    const std::int64_t start = d_->cue_start_frame.load();
    const std::int64_t end   = d_->cue_end_frame.load();
    if (end != std::numeric_limits<std::int64_t>::max()) n = std::min(n, end);
    n -= start;
    if (n < 0) n = 0;
    return static_cast<double>(n) / d_->current_fmt.sample_rate;
}
AudioFormat  PlayerController::format()      const { return d_->current_fmt; }
std::wstring PlayerController::currentFile() const {
    // current_path 在 gapless 切换时由 producer 改写;读侧也加锁保证一致性
    std::lock_guard<std::mutex> lk(d_->next_mutex);
    return d_->current_path;
}
std::wstring PlayerController::lastError()   const { return d_->get_error(); }
std::wstring PlayerController::currentDeviceId()   const { return d_->active_device_id; }
std::wstring PlayerController::currentDeviceName() const { return d_->active_device_name; }

void PlayerController::setOnStateChanged   (StateChangedCb    cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_state = std::move(cb); }
void PlayerController::setOnPositionChanged(PositionChangedCb cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_pos   = std::move(cb); }
void PlayerController::setOnEnded          (EndedCb           cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_ended = std::move(cb); }
void PlayerController::setOnError          (ErrorCb           cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_error = std::move(cb); }
void PlayerController::setOnTrackChanged   (TrackChangedCb    cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_track = std::move(cb); }
void PlayerController::setOnPcmTap         (PcmTapCb          cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_pcm   = std::move(cb); }
void PlayerController::setPcmProcessor     (PcmProcessorCb    cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_proc  = std::move(cb); }

Equalizer& PlayerController::equalizer() { return d_->eq; }
Visualizer& PlayerController::visualizer() { return d_->viz; }

void PlayerController::setAllowSharedFallback(bool on) { d_->allow_shared_fallback.store(on); }
bool PlayerController::allowSharedFallback() const     { return d_->allow_shared_fallback.load(); }

void PlayerController::setSharedDither(bool on)
{
    d_->shared_dither.store(on, std::memory_order_release);
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    if (d_->output) d_->output->setDither(on);
}
bool PlayerController::sharedDither() const { return d_->shared_dither.load(std::memory_order_acquire); }

void PlayerController::setSharedHighQuality(bool on)
{
    d_->shared_high_quality.store(on, std::memory_order_release);
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    if (d_->output) d_->output->setHighQuality(on);
}
bool PlayerController::sharedHighQuality() const { return d_->shared_high_quality.load(std::memory_order_acquire); }

void PlayerController::setDopMarkerMode(DopMarkerMode mode)
{
    d_->dop_marker_mode.store(mode, std::memory_order_release);
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    if (d_->decoder) d_->decoder->setDopMarkerMode(mode);
}
DopMarkerMode PlayerController::dopMarkerMode() const { return d_->dop_marker_mode.load(std::memory_order_acquire); }

void PlayerController::setDsdMode(DsdMode m)
{
    d_->dsd_mode.store(m, std::memory_order_release);
    // 不动正在播放的会话(切换 native/DoP 需要重新协商 WASAPI 端点,
    // 下次 loadFile 时生效).
}
PlayerController::DsdMode PlayerController::dsdMode() const { return d_->dsd_mode.load(std::memory_order_acquire); }

PlayerController::Stats PlayerController::stats() const
{
    Stats s;
    if (d_->output) {
        const RenderStats r = d_->output->renderStats();
        s.periods_total = r.periods_total;
        s.frames_total  = r.frames_total;
        s.underruns     = r.underruns;
        s.glitch_frames = r.glitch_frames;
    }
    s.recovery_count = d_->recovery_total.load(std::memory_order_acquire);
    return s;
}

void PlayerController::setReplayGainMode(ReplayGainMode m) { d_->rg_mode.store(m); }
PlayerController::ReplayGainMode PlayerController::replayGainMode() const { return d_->rg_mode.load(); }
void   PlayerController::setReplayGainPreampDb(double db) { d_->rg_preamp_db.store(db); }
double PlayerController::replayGainPreampDb() const       { return d_->rg_preamp_db.load(); }
void PlayerController::setTrackReplayGain(double gain_db, double peak)
{
    d_->rg_track_gain_db.store(gain_db);
    d_->rg_track_peak.store(peak);
}
void PlayerController::setAlbumReplayGain(double gain_db, double peak)
{
    d_->rg_album_gain_db.store(gain_db);
    d_->rg_album_peak.store(peak);
}

// -----------------------------------------------------------------------------

bool PlayerController::loadFile(const std::wstring& path)
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);

    // 先清掉旧会话
    d_->teardown_session();
    d_->clear_error();

    auto dec = DecoderFactory::createForFile(path);
    if (!dec) {
        d_->set_error_msg(L"No decoder for file: " + path);
        d_->set_state(PlayerState::Idle);
        return false;
    }
    dec->setDopMarkerMode(d_->dop_marker_mode.load(std::memory_order_acquire));
    if (!dec->open(path)) {
        d_->set_error_msg(L"decoder open failed: " + dec->lastError());
        d_->set_state(PlayerState::Idle);
        return false;
    }

    // DSD 输出模式:如果开启 Native/Auto,让 decoder 切到 raw LSB8 packed。
    // ForceDoP 不动 (decoder 默认 DoP).
    const auto dsd_mode = d_->dsd_mode.load(std::memory_order_acquire);
    bool want_native = (dsd_mode == DsdMode::ForceNative || dsd_mode == DsdMode::Auto);
    if (want_native) {
        if (!dec->setNativeDsd(true) && dsd_mode == DsdMode::ForceNative) {
            d_->set_error_msg(L"ForceNative requested but decoder doesn't support raw DSD");
            d_->set_state(PlayerState::Idle);
            return false;
        }
        // Auto + decoder 不支持 raw → 静默回 DoP
    }

    AudioFormat fmt = dec->format();
    if (!fmt.valid()) {
        d_->set_error_msg(L"decoder returned invalid format");
        d_->set_state(PlayerState::Idle);
        return false;
    }

    OpenOptions opts;
    opts.device_id = d_->pending_device_id;       // 上一次 setDevice 选的(可能为空)
    opts.allow_shared_fallback = d_->allow_shared_fallback.load();
    OpenResult result{};
    std::wstring open_err;
    auto out = open_with_fallback(fmt, opts, result, open_err,
                                  d_->shared_dither.load(std::memory_order_acquire),
                                  d_->shared_high_quality.load(std::memory_order_acquire));

    // Auto: native 协商失败时回 DoP 再试一次
    if (!out && want_native && fmt.sample_type == SampleType::DsdLsb8
        && dsd_mode == DsdMode::Auto) {
        dec->setNativeDsd(false);
        fmt = dec->format();
        open_err.clear();
        out = open_with_fallback(fmt, opts, result, open_err,
                                 d_->shared_dither.load(std::memory_order_acquire),
                                 d_->shared_high_quality.load(std::memory_order_acquire));
    }

    if (!out) {
        std::wostringstream ss;
        ss << L"WASAPI open failed:\n  " << open_err;
        ss << L"\n  请求格式: " << fmt.to_wstring();
        if (opts.device_id.empty()) ss << L"\n  目标设备: <系统默认>";
        else                         ss << L"\n  目标设备 id: " << opts.device_id;
        d_->set_error_msg(ss.str());
        d_->set_state(PlayerState::Idle);
        return false;
    }

    // ring ~1.5s 容量
    auto ring = std::make_unique<RingBuffer>(
        static_cast<std::size_t>(fmt.bytes_per_second() * 3 / 2));

    // 设置数据回调 → ring.read
    {
        RingBuffer* ring_ptr = ring.get();
        auto* impl = d_.get();
        out->setDataCallback([ring_ptr, impl](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
            const std::size_t got = ring_ptr->read(dst, bytes);
            const double gain = impl->volume.load(std::memory_order_relaxed)
                              * impl->effective_replaygain();
            apply_volume(dst, got, impl->current_fmt, gain);
            return got;
        });
        out->setErrorCallback([impl](const std::wstring&) {
            impl->recovery_pending.store(true, std::memory_order_release);
        });
    }

    d_->decoder           = std::move(dec);
    d_->output            = std::move(out);
    d_->ring              = std::move(ring);
    d_->current_path      = path;
    d_->current_fmt       = fmt;
    d_->total_frames      = d_->decoder->totalFrames();
    d_->frame_bytes       = fmt.frame_bytes();
    d_->buffer_ms         = result.buffer_ms;
    d_->active_device_id  = result.device_id;
    d_->active_device_name= result.device_name;

    // 注册设备热插拔监听 (失败仅记录,不影响主流程)
    if (!d_->devices_listening) {
        d_->devices_listening = d_->devices.registerListener(d_.get());
    }

    d_->start_workers();
    d_->set_state(PlayerState::Stopped);
    return true;
}

void PlayerController::unloadFile()
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    d_->teardown_session();
    d_->set_state(PlayerState::Idle);
}

bool PlayerController::loadItem(const PlaylistItem& item)
{
    if (!loadFile(item.path)) return false;
    // 应用 Cue 区段(在 ctrl_mutex 外做也安全:decoder/fmt 已经稳定;
    // producer 一开始是 paused,seek 与 cue_*_frame 设置之间不会被 race)
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    if (d_->current_fmt.sample_rate == 0) return true;
    const auto sr = d_->current_fmt.sample_rate;
    const std::int64_t start = (item.cue_start_sec > 0.0)
        ? static_cast<std::int64_t>(item.cue_start_sec * sr) : 0;
    const std::int64_t end   = (item.cue_end_sec > item.cue_start_sec)
        ? static_cast<std::int64_t>(item.cue_end_sec * sr)
        : std::numeric_limits<std::int64_t>::max();
    d_->cue_start_frame.store(start);
    d_->cue_end_frame.store(end);
    if (d_->decoder && start > 0) d_->decoder->seek(start);
    return true;
}

// -----------------------------------------------------------------------------

bool PlayerController::play()
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    const PlayerState s = d_->state.load();
    if (s == PlayerState::Idle || s == PlayerState::Error) {
        d_->set_error_msg(L"play(): nothing loaded");
        return false;
    }
    if (s == PlayerState::Playing) return true;

    // Ended → seek 到 cue 起点重头开始
    if (s == PlayerState::Ended) {
        if (d_->decoder) d_->decoder->seek(d_->cue_start_frame.load());
        d_->decoder_eof.store(false);
        if (d_->ring) d_->ring->clear();
    }

    // 唤醒 producer
    d_->prod_paused.store(false, std::memory_order_release);
    d_->prod_cv.notify_all();

    // 等 ring 至少装满一个设备 buffer,避免 output->start() 内部预填空 ring 产生首段静音
    d_->wait_for_ring(d_->buffer_ms, 500);

    // 启动 output(若曾在 Stopped/Paused 状态下,output 处于 Stopped)
    if (d_->output) {
        if (d_->output->state() == OutputState::Stopped) {
            if (!d_->output->start()) {
                d_->set_error_msg(L"output start failed: " + d_->output->lastError());
                d_->set_state(PlayerState::Error);
                return false;
            }
        }
    }
    d_->set_state(PlayerState::Playing);
    return true;
}

bool PlayerController::pause()
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    if (d_->state.load() != PlayerState::Playing) return false;
    if (d_->output) d_->output->stop();
    d_->prod_paused.store(true, std::memory_order_release);
    d_->set_state(PlayerState::Paused);
    return true;
}

bool PlayerController::stop()
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    const PlayerState s = d_->state.load();
    if (s == PlayerState::Idle || s == PlayerState::Error) return false;

    if (d_->output) d_->output->stop();
    d_->prod_paused.store(true, std::memory_order_release);

    // stop() = 回到当前 track 的起点 (Cue 模式下不是 0)
    if (d_->decoder) d_->decoder->seek(d_->cue_start_frame.load());
    d_->decoder_eof.store(false);
    if (d_->ring) d_->ring->clear();

    d_->fire_position(0.0);
    d_->set_state(PlayerState::Stopped);
    return true;
}

void PlayerController::setVolume(double volume)
{
    d_->volume.store(std::clamp(volume, 0.0, 1.0), std::memory_order_relaxed);
}

double PlayerController::volume() const
{
    return d_->volume.load(std::memory_order_relaxed);
}

bool PlayerController::seek(double seconds)
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    const PlayerState s = d_->state.load();
    if (s == PlayerState::Idle || s == PlayerState::Error || !d_->decoder) {
        d_->set_error_msg(L"seek(): nothing loaded");
        return false;
    }
    if (seconds < 0) seconds = 0;

    // Cue 区段:seek(0) 对应 cue_start_frame;target 不允许超过 cue_end_frame
    const std::int64_t cue_start = d_->cue_start_frame.load();
    const std::int64_t cue_end   = d_->cue_end_frame.load();
    std::int64_t target = static_cast<std::int64_t>(seconds * d_->current_fmt.sample_rate)
                        + cue_start;
    if (target > cue_end) target = cue_end;
    const bool was_playing = (s == PlayerState::Playing);

    // 暂停 producer 并清 ring
    d_->prod_paused.store(true, std::memory_order_release);
    if (was_playing && d_->output) d_->output->stop();
    if (d_->ring) d_->ring->clear();

    // 把 seek 请求委托给 producer 线程,避免在控制线程调 decoder->seek 时与 producer 抢用
    d_->seek_target_frame.store(target, std::memory_order_release);
    d_->prod_seek_pending.store(true,   std::memory_order_release);
    d_->prod_cv.notify_all();

    // 等 producer 处理完 seek(等到 pending 清 0)
    for (int i = 0; i < 200; ++i) {
        if (!d_->prod_seek_pending.load()) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(2));
    }

    // 恢复
    if (was_playing) {
        d_->prod_paused.store(false, std::memory_order_release);
        d_->prod_cv.notify_all();
        d_->wait_for_ring(d_->buffer_ms, 500);
        if (d_->output && d_->output->state() == OutputState::Stopped) {
            if (!d_->output->start()) {
                d_->set_error_msg(L"output restart after seek: " + d_->output->lastError());
                d_->set_state(PlayerState::Error);
                return false;
            }
        }
        d_->set_state(PlayerState::Playing);
    } else {
        // 非 Playing 时,seek 完保持原状态(Paused/Stopped/Ended → Stopped)
        if (s == PlayerState::Ended) d_->set_state(PlayerState::Stopped);
    }
    d_->fire_position(seconds);
    return true;
}

// -----------------------------------------------------------------------------

bool PlayerController::setDevice(const std::wstring& device_id)
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);
    d_->pending_device_id = device_id;

    const PlayerState s = d_->state.load();
    if (s == PlayerState::Idle || !d_->decoder) {
        // 没有正在播放的会话,记下来给下次 load 用
        return true;
    }

    // 需要重开 output(用新设备)。先停所有活动
    const bool was_playing = (s == PlayerState::Playing);
    const double pos = d_->estimated_position_sec();
    if (d_->output) { d_->output->stop(); d_->output->close(); }
    d_->prod_paused.store(true);
    if (d_->ring) d_->ring->clear();

    OpenOptions opts; opts.device_id = device_id;
    opts.allow_shared_fallback = d_->allow_shared_fallback.load();
    OpenResult result{};
    std::wstring open_err;
    auto new_out = open_with_fallback(d_->current_fmt, opts, result, open_err,
                                      d_->shared_dither.load(std::memory_order_acquire),
                                      d_->shared_high_quality.load(std::memory_order_acquire));
    if (!new_out) {
        d_->set_error_msg(L"setDevice: WASAPI open failed:\n  " + open_err);
        d_->set_state(PlayerState::Error);
        return false;
    }
    RingBuffer* ring_ptr = d_->ring.get();
    auto* impl = d_.get();
    new_out->setDataCallback([ring_ptr, impl](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
        const std::size_t got = ring_ptr->read(dst, bytes);
        const double gain = impl->volume.load(std::memory_order_relaxed)
                          * impl->effective_replaygain();
        apply_volume(dst, got, impl->current_fmt, gain);
        return got;
    });
    new_out->setErrorCallback([impl](const std::wstring&) {
        impl->recovery_pending.store(true, std::memory_order_release);
    });
    d_->output             = std::move(new_out);
    d_->active_device_id   = result.device_id;
    d_->active_device_name = result.device_name;
    d_->buffer_ms          = result.buffer_ms;

    // 切设备时位置回到 seek 前的位置 (Cue 模式下 pos 是 track 内坐标,补 cue_start)
    const std::int64_t target =
        static_cast<std::int64_t>(pos * d_->current_fmt.sample_rate)
        + d_->cue_start_frame.load();
    if (d_->decoder) d_->decoder->seek(target);
    d_->decoder_eof.store(false);

    if (was_playing) {
        d_->prod_paused.store(false);
        d_->prod_cv.notify_all();
        d_->wait_for_ring(d_->buffer_ms, 500);
        if (!d_->output->start()) {
            d_->set_error_msg(L"setDevice: output start failed: " + d_->output->lastError());
            d_->set_state(PlayerState::Error);
            return false;
        }
        d_->set_state(PlayerState::Playing);
    } else {
        d_->set_state(PlayerState::Stopped);
    }
    return true;
}

// -----------------------------------------------------------------------------
// Gapless 预载下一首
// -----------------------------------------------------------------------------

bool PlayerController::enqueueNext(const std::wstring& path)
{
    std::lock_guard<std::mutex> lk(d_->ctrl_mutex);

    // 空 path → 清空已排队
    if (path.empty()) {
        std::lock_guard<std::mutex> nk(d_->next_mutex);
        if (d_->next_decoder) { d_->next_decoder->close(); d_->next_decoder.reset(); }
        d_->next_path.clear();
        return true;
    }

    // 必须有当前会话
    if (!d_->decoder || !d_->current_fmt.valid()) {
        d_->set_error_msg(L"enqueueNext: no active session");
        return false;
    }

    auto dec = DecoderFactory::createForFile(path);
    if (!dec) {
        d_->set_error_msg(L"enqueueNext: no decoder for " + path);
        return false;
    }
    dec->setDopMarkerMode(d_->dop_marker_mode.load(std::memory_order_acquire));
    if (!dec->open(path)) {
        d_->set_error_msg(L"enqueueNext: open failed: " + dec->lastError());
        return false;
    }
    if (dec->format() != d_->current_fmt) {
        std::wostringstream ss;
        ss << L"enqueueNext: format mismatch (gapless impossible)\n"
           << L"  当前: " << d_->current_fmt.to_wstring() << L"\n"
           << L"  下一: " << dec->format().to_wstring();
        d_->set_error_msg(ss.str());
        return false;
    }
    {
        std::lock_guard<std::mutex> nk(d_->next_mutex);
        if (d_->next_decoder) d_->next_decoder->close();
        d_->next_decoder = std::move(dec);
        d_->next_path    = path;
    }
    return true;
}

std::wstring PlayerController::queuedNext() const
{
    std::lock_guard<std::mutex> lk(d_->next_mutex);
    return d_->next_path;
}

} // namespace apx
