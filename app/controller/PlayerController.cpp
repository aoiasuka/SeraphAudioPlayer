// =============================================================================
//  app/controller/PlayerController.cpp
// =============================================================================
#include "PlayerController.h"

#include "core/buffer/RingBuffer.h"
#include "core/decoder/DecoderFactory.h"
#include "core/decoder/IDecoder.h"
#include "core/output/IAudioOutput.h"
#include "platform/mmdevice/DeviceEnumerator.h"
#include "platform/wasapi/WasapiExclusiveOutput.h"

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
    if (!data || bytes == 0 || volume >= 0.9995) return;
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
    }
}
} // namespace

// =============================================================================

struct PlayerController::Impl {
    // ---- 控制串行化 ----
    std::mutex                       ctrl_mutex;

    // ---- 资源 ----
    std::unique_ptr<IDecoder>                  decoder;
    std::unique_ptr<RingBuffer>                ring;
    std::unique_ptr<wasapi::WasapiExclusiveOutput> output;
    DeviceEnumerator                           devices;

    // ---- 当前媒体 ----
    std::wstring  current_path;
    AudioFormat   current_fmt{};
    std::int64_t  total_frames = 0;
    std::uint32_t frame_bytes  = 0;
    double        buffer_ms    = 0.0;
    std::atomic<double> volume{1.0};

    // ---- 当前设备 ----
    std::wstring  pending_device_id;        // setDevice() 之前缓存的目标 id
    std::wstring  active_device_id;
    std::wstring  active_device_name;

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
    PcmTapCb                         cb_pcm;
    PcmProcessorCb                   cb_proc;

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

    // 当前播放位置(以已被设备消费的帧数估算)
    double estimated_position_sec() const
    {
        if (!decoder || frame_bytes == 0 || current_fmt.sample_rate == 0) return 0.0;
        const std::int64_t decoded_frames = decoder->currentFrame();
        const std::int64_t in_ring_frames = ring
            ? static_cast<std::int64_t>(ring->readable() / frame_bytes)
            : 0;
        const std::int64_t played = decoded_frames - in_ring_frames;
        const std::int64_t clamped = (played < 0) ? 0 : played;
        return static_cast<double>(clamped) / current_fmt.sample_rate;
    }

    // 释放当前会话(decoder/output/ring/线程)
    void teardown_session()
    {
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

        current_path.clear();
        current_fmt = {};
        total_frames = 0;
        frame_bytes  = 0;
        buffer_ms    = 0.0;
        active_device_id.clear();
        active_device_name.clear();
        decoder_eof.store(false);
        prod_seek_pending.store(false);
    }

    // 启动 producer + monitor 线程(只调一次,在 loadFile 成功后)
    void start_workers();
    void producer_loop();
    void monitor_loop();
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
        const std::size_t got = decoder->read(buf.data(), want);
        if (got == 0) {
            decoder_eof.store(true, std::memory_order_release);
            continue;
        }
        // DSP 处理 (EQ 等) — 先于 tap,让可视化反映最终听到的声音
        {
            PcmProcessorCb proc;
            { std::lock_guard<std::mutex> lk(cb_mutex); proc = cb_proc; }
            if (proc) proc(buf.data(), got, current_fmt);
        }
        // PCM tap (用于可视化等)
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
            // 输出端错误
            if (output && output->state() == OutputState::Error) {
                set_state(PlayerState::Error);
                set_error_msg(L"output entered Error state: " + output->lastError());
                prod_paused.store(true);
                if (output) output->stop();
            }
        }
    }
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
    return static_cast<double>(d_->total_frames) / d_->current_fmt.sample_rate;
}
AudioFormat  PlayerController::format()      const { return d_->current_fmt; }
std::wstring PlayerController::currentFile() const { return d_->current_path; }
std::wstring PlayerController::lastError()   const { return d_->get_error(); }
std::wstring PlayerController::currentDeviceId()   const { return d_->active_device_id; }
std::wstring PlayerController::currentDeviceName() const { return d_->active_device_name; }

void PlayerController::setOnStateChanged   (StateChangedCb    cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_state = std::move(cb); }
void PlayerController::setOnPositionChanged(PositionChangedCb cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_pos   = std::move(cb); }
void PlayerController::setOnEnded          (EndedCb           cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_ended = std::move(cb); }
void PlayerController::setOnError          (ErrorCb           cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_error = std::move(cb); }
void PlayerController::setOnPcmTap         (PcmTapCb          cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_pcm   = std::move(cb); }
void PlayerController::setPcmProcessor     (PcmProcessorCb    cb) { std::lock_guard<std::mutex> lk(d_->cb_mutex); d_->cb_proc  = std::move(cb); }

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
    if (!dec->open(path)) {
        d_->set_error_msg(L"decoder open failed: " + dec->lastError());
        d_->set_state(PlayerState::Idle);
        return false;
    }

    const AudioFormat fmt = dec->format();
    if (!fmt.valid()) {
        d_->set_error_msg(L"decoder returned invalid format");
        d_->set_state(PlayerState::Idle);
        return false;
    }

    auto out = std::make_unique<wasapi::WasapiExclusiveOutput>();
    OpenOptions opts;
    opts.device_id = d_->pending_device_id;       // 上一次 setDevice 选的(可能为空)
    OpenResult result{};
    if (!out->open(fmt, opts, &result)) {
        // 把请求的格式 + 目标设备一并塞进错误信息,UI 才能给出有用提示
        std::wostringstream ss;
        ss << L"WASAPI open failed: " << out->lastError();
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
            apply_volume(dst, got, impl->current_fmt, impl->volume.load(std::memory_order_relaxed));
            return got;
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

    // Ended → seek 到 0 重头开始
    if (s == PlayerState::Ended) {
        if (d_->decoder) d_->decoder->seek(0);
        d_->decoder_eof.store(false);
        if (d_->ring) d_->ring->clear();
    }

    // 唤醒 producer
    d_->prod_paused.store(false, std::memory_order_release);
    d_->prod_cv.notify_all();

    // 给 producer 短暂预热一点数据
    std::this_thread::sleep_for(std::chrono::milliseconds(60));

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

    if (d_->decoder) d_->decoder->seek(0);
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

    const std::int64_t target = static_cast<std::int64_t>(seconds * d_->current_fmt.sample_rate);
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
        std::this_thread::sleep_for(std::chrono::milliseconds(60));
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
    OpenResult result{};
    auto new_out = std::make_unique<wasapi::WasapiExclusiveOutput>();
    if (!new_out->open(d_->current_fmt, opts, &result)) {
        d_->set_error_msg(L"setDevice: WASAPI open failed: " + new_out->lastError());
        d_->set_state(PlayerState::Error);
        return false;
    }
    RingBuffer* ring_ptr = d_->ring.get();
    auto* impl = d_.get();
    new_out->setDataCallback([ring_ptr, impl](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
        const std::size_t got = ring_ptr->read(dst, bytes);
        apply_volume(dst, got, impl->current_fmt, impl->volume.load(std::memory_order_relaxed));
        return got;
    });
    d_->output             = std::move(new_out);
    d_->active_device_id   = result.device_id;
    d_->active_device_name = result.device_name;
    d_->buffer_ms          = result.buffer_ms;

    // 切设备时位置回到 seek 前的位置(避免 ring 里的旧数据音质混乱)
    const std::int64_t target = static_cast<std::int64_t>(pos * d_->current_fmt.sample_rate);
    if (d_->decoder) d_->decoder->seek(target);
    d_->decoder_eof.store(false);

    if (was_playing) {
        d_->prod_paused.store(false);
        d_->prod_cv.notify_all();
        std::this_thread::sleep_for(std::chrono::milliseconds(60));
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

} // namespace apx
