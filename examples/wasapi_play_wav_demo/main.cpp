// =============================================================================
//  examples/wasapi_play_wav_demo
//
//  解码 WAV → RingBuffer → WASAPI 独占输出 → 默认或指定设备播放。
//
//  用法:
//      wasapi_play_wav_demo.exe [-d <id|name-substring>] <file.wav>
//      wasapi_play_wav_demo.exe --list-devices
//
//  示例:
//      wasapi_play_wav_demo D:\Music\song.wav
//      wasapi_play_wav_demo -d "Topping" D:\Music\song.wav
//      wasapi_play_wav_demo -d "{0.0.0.00000000}.{guid}" D:\Music\song.wav
// =============================================================================

#include "core/decoder/DecoderFactory.h"
#include "core/decoder/IDecoder.h"
#include "core/buffer/RingBuffer.h"
#include "core/output/IAudioOutput.h"
#include "platform/wasapi/WasapiExclusiveOutput.h"
#include "platform/mmdevice/DeviceEnumerator.h"

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <cwctype>
#include <thread>
#include <vector>

namespace {
std::atomic<bool> g_interrupted{false};
void on_sigint(int) { g_interrupted.store(true, std::memory_order_release); }

void print_usage()
{
    std::fwprintf(stderr,
        L"usage: wasapi_play_wav_demo [-d <id|name>] <file.wav>\n"
        L"       wasapi_play_wav_demo --list-devices\n");
}

const wchar_t* state_name(apx::DeviceState s)
{
    switch (s) {
    case apx::DeviceState::Active:     return L"Active";
    case apx::DeviceState::Disabled:   return L"Disabled";
    case apx::DeviceState::NotPresent: return L"NotPresent";
    case apx::DeviceState::Unplugged:  return L"Unplugged";
    }
    return L"?";
}
} // namespace

int wmain(int argc, wchar_t** argv)
{
    using namespace apx;

    std::wstring dev_hint;     // -d 参数
    std::wstring path;
    bool list_only = false;

    for (int i = 1; i < argc; ++i) {
        std::wstring a = argv[i];
        if (a == L"--list-devices") { list_only = true; }
        else if (a == L"-d" || a == L"--device") {
            if (i + 1 >= argc) { print_usage(); return 2; }
            dev_hint = argv[++i];
        }
        else if (!a.empty() && a[0] == L'-') {
            std::fwprintf(stderr, L"unknown option: %s\n", a.c_str());
            print_usage();
            return 2;
        }
        else {
            path = a;
        }
    }

    if (list_only) {
        DeviceEnumerator de;
        const auto list = de.listRenderEndpoints(true);
        std::wprintf(L"渲染设备共 %zu 个:\n", list.size());
        for (const auto& d : list) {
            std::wprintf(L"  [%-10s] %s%s\n    id = %s\n",
                         state_name(d.state),
                         d.friendly_name.c_str(),
                         d.is_default_console() ? L"  (DEFAULT)" : L"",
                         d.id.c_str());
        }
        return 0;
    }

    if (path.empty()) { print_usage(); return 2; }
    std::signal(SIGINT, on_sigint);

    // ---------- 1) 解析 -d → 真实 device_id ----------
    OpenOptions opts;
    if (!dev_hint.empty()) {
        DeviceEnumerator de;
        // 1.1 当作精确 id 试一次(GetDevice 直接调)
        auto info = de.findById(dev_hint);
        // 1.2 当作 friendly name 子串
        if (!info) info = de.findByNameSubstring(dev_hint);
        if (!info) {
            std::fwprintf(stderr,
                L"[FAIL] 未找到匹配设备: %s\n"
                L"       提示:用 --list-devices 查看所有设备\n",
                dev_hint.c_str());
            return 1;
        }
        if (info->state != DeviceState::Active) {
            std::fwprintf(stderr,
                L"[FAIL] 设备 %s 当前状态为 %s,无法打开\n",
                info->friendly_name.c_str(), state_name(info->state));
            return 1;
        }
        opts.device_id = info->id;
        std::wprintf(L"[ OK ] 指定设备:%s\n", info->friendly_name.c_str());
    }

    // ---------- 2) 创建并打开解码器 ----------
    auto decoder = DecoderFactory::createForFile(path);
    if (!decoder) {
        std::fwprintf(stderr, L"[FAIL] 无可用解码器(支持 .wav/.wave): %s\n", path.c_str());
        return 1;
    }
    if (!decoder->open(path)) {
        std::fwprintf(stderr, L"[FAIL] decoder open: %s\n", decoder->lastError().c_str());
        return 1;
    }
    const AudioFormat fmt = decoder->format();
    const double duration_s = decoder->totalFrames() / static_cast<double>(fmt.sample_rate);

    std::wprintf(L"[ OK ] File:     %s\n", path.c_str());
    std::wprintf(L"[ OK ] Format:   %s\n", fmt.to_wstring().c_str());
    std::wprintf(L"[ OK ] Duration: %.3f s (%lld frames)\n",
                 duration_s, static_cast<long long>(decoder->totalFrames()));

    // ---------- 3) 打开 WASAPI ----------
    wasapi::WasapiExclusiveOutput out;
    OpenResult info{};
    if (!out.open(fmt, opts, &info)) {
        std::fwprintf(stderr, L"[FAIL] WASAPI open: %s\n", out.lastError().c_str());
        return 1;
    }
    std::wprintf(L"[ OK ] Device:   %s\n", info.device_name.c_str());
    std::wprintf(L"[ OK ] Buffer:   %u frames (%.2f ms),周期 %.2f ms\n",
                 info.buffer_frames, info.buffer_ms, info.period_ms);

    // ---------- 4) RingBuffer + 回调 ----------
    RingBuffer ring(static_cast<std::size_t>(fmt.bytes_per_second() * 3 / 2));
    out.setDataCallback([&ring](std::uint8_t* dst, std::size_t bytes) -> std::size_t {
        return ring.read(dst, bytes);
    });

    // ---------- 5) 生产者线程 ----------
    std::atomic<bool> producer_run{true};
    std::atomic<bool> eof{false};
    std::thread producer([&]{
        constexpr std::size_t kBatch = 16 * 1024;
        std::vector<std::uint8_t> buf(kBatch);
        while (producer_run.load(std::memory_order_acquire) && !eof.load()) {
            std::size_t free_bytes = ring.writable();
            if (free_bytes < fmt.frame_bytes()) {
                std::this_thread::sleep_for(std::chrono::milliseconds(2));
                continue;
            }
            std::size_t want = (free_bytes < kBatch) ? free_bytes : kBatch;
            want -= (want % fmt.frame_bytes());
            if (want == 0) { std::this_thread::sleep_for(std::chrono::milliseconds(2)); continue; }
            const std::size_t got = decoder->read(buf.data(), want);
            if (got == 0) { eof.store(true, std::memory_order_release); break; }
            std::size_t written = 0;
            while (written < got && producer_run.load(std::memory_order_acquire)) {
                const std::size_t w = ring.write(buf.data() + written, got - written);
                written += w;
                if (w == 0) std::this_thread::sleep_for(std::chrono::milliseconds(2));
            }
        }
    });

    std::this_thread::sleep_for(std::chrono::milliseconds(80));

    if (!out.start()) {
        std::fwprintf(stderr, L"[FAIL] WASAPI start: %s\n", out.lastError().c_str());
        producer_run.store(false); producer.join();
        out.close(); decoder->close();
        return 1;
    }
    std::wprintf(L"[ OK ] Playing... (Ctrl+C 退出)\n");

    while (!g_interrupted.load(std::memory_order_acquire)) {
        if (out.state() != OutputState::Running) break;
        if (eof.load(std::memory_order_acquire) && ring.readable() == 0) break;
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    std::this_thread::sleep_for(
        std::chrono::milliseconds(static_cast<int>(info.buffer_ms) + 30));

    out.stop();
    producer_run.store(false, std::memory_order_release);
    producer.join();
    out.close();
    decoder->close();

    if (g_interrupted.load()) std::wprintf(L"[INFO] 用户中断,已停止\n");
    else                       std::wprintf(L"[ OK ] 播放完毕\n");
    return 0;
}
