// =============================================================================
//  platform/wasapi/WasapiSharedOutput.cpp
//
//  与 WasapiExclusiveOutput 大量结构相同,关键差异:
//    1) Initialize 走 AUDCLNT_SHAREMODE_SHARED + EVENTCALLBACK
//    2) 设备格式 = IAudioClient::GetMixFormat (Windows mixer 决定,不可改)
//    3) 内部 FormatConverter 把 callback 期望的 SOURCE 帧转成 device 帧
//    4) buffer_frames 不是固定的,GetCurrentPadding 决定每次能写多少
// =============================================================================
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <avrt.h>
#include <ksmedia.h>
#include <functiondiscoverykeys_devpkey.h>

#include "WasapiSharedOutput.h"
#include "core/dsp/FormatConverter.h"
#include "core/format/AudioFormat.h"

#include <atomic>
#include <mutex>
#include <sstream>
#include <thread>
#include <vector>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "avrt.lib")

namespace apx::wasapi {

namespace {

constexpr REFERENCE_TIME kRefTimesPerSec = 10'000'000;

// 把 WAVEFORMATEX/EXTENSIBLE 翻译成 apx::AudioFormat
bool wfx_to_audio_format(const WAVEFORMATEX* wfx, AudioFormat& out)
{
    if (!wfx) return false;
    out = {};
    out.sample_rate     = wfx->nSamplesPerSec;
    out.channels        = wfx->nChannels;
    out.bits_per_sample = wfx->wBitsPerSample;
    out.valid_bits      = wfx->wBitsPerSample;

    GUID subfmt = {};
    bool is_float = false;
    if (wfx->wFormatTag == WAVE_FORMAT_EXTENSIBLE
        && wfx->cbSize >= sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX)) {
        const auto* wex = reinterpret_cast<const WAVEFORMATEXTENSIBLE*>(wfx);
        out.valid_bits  = wex->Samples.wValidBitsPerSample
                        ? wex->Samples.wValidBitsPerSample : wfx->wBitsPerSample;
        out.channel_mask = wex->dwChannelMask;
        subfmt = wex->SubFormat;
        is_float = (subfmt == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT);
    } else if (wfx->wFormatTag == WAVE_FORMAT_IEEE_FLOAT) {
        is_float = true;
    } else if (wfx->wFormatTag != WAVE_FORMAT_PCM) {
        return false;
    }

    if (is_float) {
        if (wfx->wBitsPerSample != 32) return false;
        out.sample_type = SampleType::Float32;
        out.valid_bits  = 32;
    } else {
        switch (wfx->wBitsPerSample) {
        case 16: out.sample_type = SampleType::Int16;       out.valid_bits = 16; break;
        case 24: out.sample_type = SampleType::Int24Packed; out.valid_bits = 24; break;
        case 32: out.sample_type = SampleType::Int32;                            break;
        default: return false;
        }
    }
    return out.valid();
}

std::wstring read_device_name(IMMDevice* dev)
{
    std::wstring name;
    IPropertyStore* props = nullptr;
    if (SUCCEEDED(dev->OpenPropertyStore(STGM_READ, &props)) && props) {
        PROPVARIANT pv; PropVariantInit(&pv);
        if (SUCCEEDED(props->GetValue(PKEY_Device_FriendlyName, &pv)) && pv.vt == VT_LPWSTR) {
            name = pv.pwszVal ? pv.pwszVal : L"";
        }
        PropVariantClear(&pv);
        props->Release();
    }
    return name;
}

} // namespace

struct WasapiSharedOutput::Impl {
    IMMDeviceEnumerator* enumerator = nullptr;
    IMMDevice*           device     = nullptr;
    IAudioClient*        client     = nullptr;
    IAudioRenderClient*  render     = nullptr;
    HANDLE               event      = nullptr;
    WAVEFORMATEX*        mix_wfx    = nullptr;     // CoTaskMemFree
    bool                 com_init_self = false;

    AudioFormat          source_fmt{};
    AudioFormat          device_fmt{};
    UINT32               buffer_frames = 0;        // 整个客户端缓冲(共享模式由 OS 决定)
    UINT32               src_frame_bytes = 0;
    UINT32               dst_frame_bytes = 0;
    REFERENCE_TIME       device_period = 0;
    std::wstring         device_name;
    std::wstring         device_id;

    FormatConverter      conv;
    std::vector<std::uint8_t> src_scratch;   // 从 DataCallback 拉到的源格式数据

    std::atomic<OutputState> state{OutputState::Closed};
    std::atomic<bool>        running{false};
    std::thread              render_thread;
    DataCallback             callback;
    ErrorCallback            err_callback;

    // ---- 渲染统计 ----
    std::atomic<std::uint64_t> stat_periods{0};
    std::atomic<std::uint64_t> stat_frames{0};
    std::atomic<std::uint64_t> stat_underruns{0};
    std::atomic<std::uint64_t> stat_glitch_frames{0};

    mutable std::mutex   err_mutex;
    std::wstring         last_error;

    void set_error(const wchar_t* what, HRESULT hr)
    {
        std::wostringstream ss;
        ss << what << L" failed: hr=0x" << std::hex << static_cast<unsigned long>(hr);
        std::lock_guard<std::mutex> lk(err_mutex);
        last_error = ss.str();
    }
    void set_error_msg(const std::wstring& m)
    {
        std::lock_guard<std::mutex> lk(err_mutex);
        last_error = m;
    }
    std::wstring get_error() const
    {
        std::lock_guard<std::mutex> lk(err_mutex);
        return last_error;
    }

    void release_all() noexcept
    {
        if (render)     { render->Release();     render = nullptr; }
        if (client)     { client->Release();     client = nullptr; }
        if (device)     { device->Release();     device = nullptr; }
        if (enumerator) { enumerator->Release(); enumerator = nullptr; }
        if (event)      { CloseHandle(event);    event = nullptr; }
        if (mix_wfx)    { CoTaskMemFree(mix_wfx); mix_wfx = nullptr; }
        if (com_init_self) {
            CoUninitialize();
            com_init_self = false;
        }
        buffer_frames = 0;
        src_frame_bytes = dst_frame_bytes = 0;
    }

    void render_proc();
};

// -----------------------------------------------------------------------------

WasapiSharedOutput::WasapiSharedOutput() : d_(std::make_unique<Impl>()) {}
WasapiSharedOutput::~WasapiSharedOutput() { close(); }

OutputState  WasapiSharedOutput::state()     const { return d_->state.load(std::memory_order_acquire); }
std::wstring WasapiSharedOutput::lastError() const { return d_->get_error(); }
void         WasapiSharedOutput::setDataCallback(DataCallback cb) { d_->callback = std::move(cb); }
void         WasapiSharedOutput::setErrorCallback(ErrorCallback cb) { d_->err_callback = std::move(cb); }
RenderStats  WasapiSharedOutput::renderStats() const
{
    RenderStats r;
    r.periods_total = d_->stat_periods.load(std::memory_order_acquire);
    r.frames_total  = d_->stat_frames.load(std::memory_order_acquire);
    r.underruns     = d_->stat_underruns.load(std::memory_order_acquire);
    r.glitch_frames = d_->stat_glitch_frames.load(std::memory_order_acquire);
    return r;
}

bool WasapiSharedOutput::open(const AudioFormat& fmt, const OpenOptions& opts, OpenResult* result)
{
    if (d_->state.load() != OutputState::Closed) {
        d_->set_error_msg(L"open() called in non-Closed state");
        return false;
    }
    if (!fmt.valid()) { d_->set_error_msg(L"AudioFormat is invalid"); return false; }
    d_->source_fmt = fmt;

    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (hr == RPC_E_CHANGED_MODE) d_->com_init_self = false;
    else if (SUCCEEDED(hr))       d_->com_init_self = (hr == S_OK);
    else { d_->set_error(L"CoInitializeEx", hr); return false; }

    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                          __uuidof(IMMDeviceEnumerator),
                          reinterpret_cast<void**>(&d_->enumerator));
    if (FAILED(hr)) { d_->set_error(L"CoCreateInstance(MMDeviceEnumerator)", hr); goto Fail; }

    if (opts.device_id.empty()) {
        hr = d_->enumerator->GetDefaultAudioEndpoint(eRender, eConsole, &d_->device);
        if (FAILED(hr)) { d_->set_error(L"GetDefaultAudioEndpoint", hr); goto Fail; }
    } else {
        hr = d_->enumerator->GetDevice(opts.device_id.c_str(), &d_->device);
        if (FAILED(hr)) { d_->set_error(L"GetDevice(device_id)", hr); goto Fail; }
    }

    d_->device_name = read_device_name(d_->device);
    {
        LPWSTR id = nullptr;
        if (SUCCEEDED(d_->device->GetId(&id)) && id) {
            d_->device_id = id;
            CoTaskMemFree(id);
        }
    }

    hr = d_->device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                              reinterpret_cast<void**>(&d_->client));
    if (FAILED(hr)) { d_->set_error(L"IMMDevice::Activate", hr); goto Fail; }

    // 共享模式拿 mix format
    hr = d_->client->GetMixFormat(&d_->mix_wfx);
    if (FAILED(hr) || !d_->mix_wfx) { d_->set_error(L"GetMixFormat", hr); goto Fail; }
    if (!wfx_to_audio_format(d_->mix_wfx, d_->device_fmt)) {
        d_->set_error_msg(L"unsupported mix format from device");
        goto Fail;
    }

    {
        REFERENCE_TIME min_period = 0;
        hr = d_->client->GetDevicePeriod(&d_->device_period, &min_period);
        if (FAILED(hr)) { d_->set_error(L"GetDevicePeriod", hr); goto Fail; }
    }

    // 共享模式 Initialize:buffer_duration 给 0 让 OS 选最优值
    hr = d_->client->Initialize(AUDCLNT_SHAREMODE_SHARED,
                                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                0, 0,
                                d_->mix_wfx, nullptr);
    if (FAILED(hr)) { d_->set_error(L"IAudioClient::Initialize (shared)", hr); goto Fail; }

    hr = d_->client->GetBufferSize(&d_->buffer_frames);
    if (FAILED(hr)) { d_->set_error(L"GetBufferSize", hr); goto Fail; }

    d_->event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!d_->event) { d_->set_error(L"CreateEvent", HRESULT_FROM_WIN32(GetLastError())); goto Fail; }
    hr = d_->client->SetEventHandle(d_->event);
    if (FAILED(hr)) { d_->set_error(L"SetEventHandle", hr); goto Fail; }

    hr = d_->client->GetService(__uuidof(IAudioRenderClient),
                                reinterpret_cast<void**>(&d_->render));
    if (FAILED(hr)) { d_->set_error(L"GetService(IAudioRenderClient)", hr); goto Fail; }

    // 准备 FormatConverter:源 → 设备
    if (!d_->conv.configure(fmt, d_->device_fmt)) {
        d_->set_error_msg(L"FormatConverter::configure failed (channels mismatch?)");
        goto Fail;
    }
    d_->src_frame_bytes = fmt.frame_bytes();
    d_->dst_frame_bytes = d_->device_fmt.frame_bytes();
    if (d_->src_frame_bytes == 0 || d_->dst_frame_bytes == 0) {
        d_->set_error_msg(L"frame_bytes == 0");
        goto Fail;
    }
    d_->state.store(OutputState::Stopped, std::memory_order_release);
    (void)opts;     // allow_shared_fallback 只在 PlayerController 用,这里不需要
    d_->stat_periods.store(0);
    d_->stat_frames.store(0);
    d_->stat_underruns.store(0);
    d_->stat_glitch_frames.store(0);

    if (result) {
        result->actual_format  = d_->device_fmt;
        result->buffer_frames  = d_->buffer_frames;
        result->buffer_ms      = d_->buffer_frames * 1000.0 / d_->device_fmt.sample_rate;
        result->period_ms      = d_->device_period / 10000.0;
        result->device_name    = d_->device_name;
        result->device_id      = d_->device_id;
        result->shared_mode    = true;
    }
    return true;

Fail:
    d_->release_all();
    d_->state.store(OutputState::Closed, std::memory_order_release);
    return false;
}

void WasapiSharedOutput::close()
{
    if (d_->state.load() == OutputState::Running) stop();
    d_->release_all();
    d_->state.store(OutputState::Closed, std::memory_order_release);
}

bool WasapiSharedOutput::start()
{
    if (d_->state.load() != OutputState::Stopped) {
        d_->set_error_msg(L"start() requires Stopped state");
        return false;
    }
    if (!d_->callback) {
        d_->set_error_msg(L"start() called without DataCallback");
        return false;
    }

    HRESULT hr = d_->client->Start();
    if (FAILED(hr)) { d_->set_error(L"IAudioClient::Start", hr); return false; }

    d_->running.store(true, std::memory_order_release);
    d_->state.store(OutputState::Running, std::memory_order_release);
    d_->render_thread = std::thread([this]{ d_->render_proc(); });
    return true;
}

void WasapiSharedOutput::stop()
{
    if (d_->state.load() != OutputState::Running) return;
    d_->running.store(false, std::memory_order_release);
    if (d_->event) SetEvent(d_->event);
    if (d_->render_thread.joinable()) d_->render_thread.join();
    if (d_->client) d_->client->Stop();
    d_->state.store(OutputState::Stopped, std::memory_order_release);
}

void WasapiSharedOutput::Impl::render_proc()
{
    HRESULT hr_com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    bool thread_com_init = SUCCEEDED(hr_com) && hr_com != S_FALSE;

    DWORD task_idx = 0;
    HANDLE mm_task = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_idx);

    auto fire_error = [this]() {
        if (err_callback) err_callback(get_error());
    };

    while (running.load(std::memory_order_acquire)) {
        const DWORD wr = WaitForSingleObject(event, 2000);
        if (!running.load(std::memory_order_acquire)) break;
        if (wr != WAIT_OBJECT_0) {
            set_error_msg(L"WASAPI render-thread wait timeout (shared)");
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }

        UINT32 padding = 0;
        HRESULT hr = client->GetCurrentPadding(&padding);
        if (FAILED(hr)) {
            set_error(L"GetCurrentPadding", hr);
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }
        const UINT32 frames_avail = buffer_frames - padding;
        if (frames_avail == 0) continue;

        BYTE* p = nullptr;
        hr = render->GetBuffer(frames_avail, &p);
        if (FAILED(hr)) {
            set_error(L"GetBuffer (shared)", hr);
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }

        // 估计需要多少源帧来产生 frames_avail 个目标帧
        const double ratio = (double)device_fmt.sample_rate / (double)source_fmt.sample_rate;
        std::size_t need_src_frames = static_cast<std::size_t>(
            (double)frames_avail / ratio) + 2;
        const std::size_t need_src_bytes = need_src_frames * src_frame_bytes;
        if (src_scratch.size() < need_src_bytes) src_scratch.resize(need_src_bytes);

        std::size_t got_src_bytes = 0;
        if (callback) got_src_bytes = callback(src_scratch.data(), need_src_bytes);
        if (got_src_bytes > need_src_bytes) got_src_bytes = need_src_bytes;

        const std::size_t got_src_frames = got_src_bytes / src_frame_bytes;
        const std::size_t produced = conv.process(
            src_scratch.data(), got_src_frames,
            reinterpret_cast<std::uint8_t*>(p), frames_avail);

        const std::size_t produced_bytes = produced * dst_frame_bytes;
        const std::size_t want_bytes     = static_cast<std::size_t>(frames_avail) * dst_frame_bytes;
        if (produced_bytes < want_bytes) {
            std::memset(p + produced_bytes, 0, want_bytes - produced_bytes);
            stat_underruns.fetch_add(1, std::memory_order_release);
            stat_glitch_frames.fetch_add(
                (want_bytes - produced_bytes) / dst_frame_bytes,
                std::memory_order_release);
        }
        stat_periods.fetch_add(1, std::memory_order_release);
        stat_frames.fetch_add(frames_avail, std::memory_order_release);

        hr = render->ReleaseBuffer(frames_avail, 0);
        if (FAILED(hr)) {
            set_error(L"ReleaseBuffer (shared)", hr);
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }
    }

    if (mm_task) AvRevertMmThreadCharacteristics(mm_task);
    if (thread_com_init) CoUninitialize();
}

} // namespace apx::wasapi
