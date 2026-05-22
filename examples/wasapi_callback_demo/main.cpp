// =============================================================================
//  examples/wasapi_callback_demo
//
//  验证三件事:
//    1) AudioFormat 与 WAVEFORMATEXTENSIBLE 之间的协商
//    2) SPSC RingBuffer 在两线程间无锁传递 PCM
//    3) WasapiExclusiveOutput 通过回调从 RingBuffer 拉数据
//
//  线程模型:
//    [Producer Thread]  生成 1 kHz 正弦波 → RingBuffer.write()
//    [Render Thread]    DataCallback ← RingBuffer.read() → WASAPI GetBuffer
//
//  这就是最终 PlayerEngine 的核心数据通路雏形。
// =============================================================================

#include "core/format/AudioFormat.h"
#include "core/buffer/RingBuffer.h"
#include "core/output/IAudioOutput.h"
#include "platform/wasapi/WasapiExclusiveOutput.h"

#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <thread>
#include <vector>

namespace {
constexpr double kPi      = 3.14159265358979323846;
constexpr double kFreqHz  = 1000.0;
constexpr double kAmp     = 0.2;
constexpr int    kPlaySec = 5;
} // namespace

// 简单的 16-bit 立体声正弦生成器(producer 用)
struct Sine16 {
    double  phase     = 0.0;
    double  phase_inc = 0.0;
    uint16_t channels = 2;

    void init(double freq, uint32_t sr, uint16_t ch) {
        phase = 0.0;
        phase_inc = 2.0 * kPi * freq / static_cast<double>(sr);
        channels = ch;
    }
    void render(int16_t* dst, size_t frames) {
        for (size_t i = 0; i < frames; ++i) {
            const int16_t v = static_cast<int16_t>(kAmp * std::sin(phase) * 32767.0);
            phase += phase_inc;
            if (phase >= 2.0 * kPi) phase -= 2.0 * kPi;
            for (uint16_t c = 0; c < channels; ++c)
                dst[i * channels + c] = v;
        }
    }
};

int wmain()
{
    using namespace apx;

    const AudioFormat fmt = AudioFormat::pcm16(44100, 2);
    std::wprintf(L"[INFO] Requested format: %s\n", fmt.to_wstring().c_str());

    // 1 秒容量(实际向上取整到 2 的幂 ≈ 256 KB)
    RingBuffer ring(fmt.bytes_per_second());
    std::wprintf(L"[ OK ] RingBuffer capacity: %zu bytes (~%.0f ms)\n",
                 ring.capacity(),
                 ring.capacity() * 1000.0 / fmt.bytes_per_second());

    // ---------- 打开 WASAPI 独占输出 ----------
    wasapi::WasapiExclusiveOutput out;
    OpenResult info{};
    if (!out.open(fmt, OpenOptions{}, &info)) {
        std::fwprintf(stderr, L"[FAIL] open: %s\n", out.lastError().c_str());
        return 1;
    }
    std::wprintf(L"[ OK ] Device: %s\n", info.device_name.c_str());
    std::wprintf(L"[ OK ] Buffer: %u frames (%.2f ms), device period %.2f ms\n",
                 info.buffer_frames, info.buffer_ms, info.period_ms);

    // 注册回调:从 RingBuffer 读字节;不够则返回实际字节数,输出端会静音补齐
    out.setDataCallback([&ring](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
        return ring.read(dst, bytes);
    });

    // ---------- 启动生产者线程(模拟解码器) ----------
    std::atomic<bool> producer_run{true};
    std::thread producer([&]{
        Sine16 gen; gen.init(kFreqHz, fmt.sample_rate, fmt.channels);
        constexpr size_t kBatchFrames = 1024;
        std::vector<int16_t> buf(kBatchFrames * fmt.channels);

        while (producer_run.load(std::memory_order_acquire)) {
            gen.render(buf.data(), kBatchFrames);
            const std::size_t total = kBatchFrames * fmt.frame_bytes();
            const std::uint8_t* p = reinterpret_cast<const std::uint8_t*>(buf.data());
            std::size_t left = total;
            while (left > 0 && producer_run.load(std::memory_order_acquire)) {
                const std::size_t w = ring.write(p, left);
                p    += w;
                left -= w;
                if (w == 0) {
                    // 缓冲已满,让出 CPU(真实场景这里换成 cond_var 等待)
                    std::this_thread::sleep_for(std::chrono::milliseconds(1));
                }
            }
        }
    });

    // 给生产者一点提前量,避免 start() 时 RingBuffer 空着导致预填充全静音
    std::this_thread::sleep_for(std::chrono::milliseconds(20));

    // ---------- 启动渲染 ----------
    if (!out.start()) {
        std::fwprintf(stderr, L"[FAIL] start: %s\n", out.lastError().c_str());
        producer_run.store(false);
        producer.join();
        out.close();
        return 1;
    }
    std::wprintf(L"[ OK ] Playback started — %d s of %.0f Hz tone\n", kPlaySec, kFreqHz);

    // 主线程睡眠,WASAPI 渲染线程与生产者线程协作完成播放
    std::this_thread::sleep_for(std::chrono::seconds(kPlaySec));

    // ---------- 收尾 ----------
    out.stop();
    producer_run.store(false, std::memory_order_release);
    producer.join();
    out.close();

    std::wprintf(L"[ OK ] Done.\n");
    return 0;
}
