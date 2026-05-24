// =============================================================================
//  platform/wasapi/WasapiExclusiveOutput.cpp
//
//  pImpl 内持有 COM 接口与渲染线程。所有 COM 调用都在 open() 或渲染线程中,
//  控制线程(GUI 线程)不必是 MTA;CoInitializeEx 用 best-effort 策略。
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

#include "WasapiExclusiveOutput.h"
#include "core/format/AudioFormat.h"

#include <atomic>
#include <mutex>
#include <sstream>
#include <thread>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "avrt.lib")
#pragma comment(lib, "ksuser.lib")
#pragma comment(lib, "uuid.lib")

namespace apx::wasapi {

namespace {

constexpr REFERENCE_TIME kRefTimesPerSec = 10'000'000;

const wchar_t* hr_brief(HRESULT hr) noexcept
{
    switch (hr) {
    case S_OK:                                       return L"S_OK";
    case AUDCLNT_E_NOT_INITIALIZED:                  return L"NOT_INITIALIZED";
    case AUDCLNT_E_ALREADY_INITIALIZED:              return L"ALREADY_INITIALIZED";
    case AUDCLNT_E_WRONG_ENDPOINT_TYPE:              return L"WRONG_ENDPOINT_TYPE";
    case AUDCLNT_E_DEVICE_INVALIDATED:               return L"DEVICE_INVALIDATED";
    case AUDCLNT_E_NOT_STOPPED:                      return L"NOT_STOPPED";
    case AUDCLNT_E_BUFFER_TOO_LARGE:                 return L"BUFFER_TOO_LARGE";
    case AUDCLNT_E_OUT_OF_ORDER:                     return L"OUT_OF_ORDER";
    case AUDCLNT_E_UNSUPPORTED_FORMAT:               return L"UNSUPPORTED_FORMAT";
    case AUDCLNT_E_INVALID_DEVICE_PERIOD:            return L"INVALID_DEVICE_PERIOD";
    case AUDCLNT_E_INVALID_SIZE:                     return L"INVALID_SIZE";
    case AUDCLNT_E_DEVICE_IN_USE:                    return L"DEVICE_IN_USE";
    case AUDCLNT_E_BUFFER_OPERATION_PENDING:         return L"BUFFER_OPERATION_PENDING";
    case AUDCLNT_E_THREAD_NOT_REGISTERED:            return L"THREAD_NOT_REGISTERED";
    case AUDCLNT_E_EXCLUSIVE_MODE_NOT_ALLOWED:       return L"EXCLUSIVE_NOT_ALLOWED";
    case AUDCLNT_E_ENDPOINT_CREATE_FAILED:           return L"ENDPOINT_CREATE_FAILED";
    case AUDCLNT_E_SERVICE_NOT_RUNNING:              return L"SERVICE_NOT_RUNNING";
    case AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED:          return L"BUFFER_SIZE_NOT_ALIGNED";
    case AUDCLNT_E_EVENTHANDLE_NOT_SET:              return L"EVENTHANDLE_NOT_SET";
    default:                                         return L"?";
    }
}

bool build_wfx(const AudioFormat& fmt, WAVEFORMATEXTENSIBLE& w) noexcept
{
    if (!fmt.valid()) return false;
    ZeroMemory(&w, sizeof(w));
    w.Format.wFormatTag      = WAVE_FORMAT_EXTENSIBLE;
    w.Format.nChannels       = fmt.channels;
    w.Format.nSamplesPerSec  = fmt.sample_rate;
    w.Format.wBitsPerSample  = fmt.bits_per_sample;
    w.Format.nBlockAlign     = static_cast<WORD>(fmt.channels * (fmt.bits_per_sample / 8));
    w.Format.nAvgBytesPerSec = fmt.sample_rate * w.Format.nBlockAlign;
    w.Format.cbSize          = sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX);
    w.Samples.wValidBitsPerSample = fmt.valid_bits;
    w.dwChannelMask          = fmt.channel_mask ? fmt.channel_mask
                                                : default_channel_mask(fmt.channels);
    switch (fmt.sample_type) {
    case SampleType::Int16:
    case SampleType::Int24Packed:
    case SampleType::Int32:    w.SubFormat = KSDATAFORMAT_SUBTYPE_PCM;        break;
    case SampleType::Float32:  w.SubFormat = KSDATAFORMAT_SUBTYPE_IEEE_FLOAT; break;
    case SampleType::DsdLsb8:
        // KSDATAFORMAT_SUBTYPE_DSD = {0x00000003-0cea-0010-8000-00aa00389b71}
        // Win10 1709+ 内核理论支持,实际可用度极依赖 DAC 驱动是否暴露 DSD 端点;
        // 多数消费 USB DAC 走 ASIO Native DSD,而非 WASAPI。本播放器暂不构造
        // 该 SubFormat — 留待真有兼容设备时启用。
        return false;
    }
    return true;
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

// =============================================================================

struct WasapiExclusiveOutput::Impl {
    // ---- COM 资源 ----
    IMMDeviceEnumerator* enumerator = nullptr;
    IMMDevice*           device     = nullptr;
    IAudioClient*        client     = nullptr;
    IAudioRenderClient*  render     = nullptr;
    HANDLE               event      = nullptr;
    bool                 com_init_self = false;  // 我们调用了 CoInitializeEx 且需要配对

    // ---- 协商结果 ----
    AudioFormat          requested{};
    WAVEFORMATEXTENSIBLE wfx{};
    UINT32               buffer_frames = 0;
    UINT32               frame_bytes   = 0;
    REFERENCE_TIME       device_period = 0;
    std::wstring         device_name;
    std::wstring         device_id;

    // ---- 状态 ----
    std::atomic<OutputState> state{OutputState::Closed};
    std::atomic<bool>        running{false};
    std::thread              render_thread;
    DataCallback             callback;
    ErrorCallback            err_callback;

    // ---- 渲染统计(原子,monitor / UI 可读) ----
    std::atomic<std::uint64_t> stat_periods{0};
    std::atomic<std::uint64_t> stat_frames{0};
    std::atomic<std::uint64_t> stat_underruns{0};
    std::atomic<std::uint64_t> stat_glitch_frames{0};

    mutable std::mutex   err_mutex;
    std::wstring         last_error;

    // ---- 助手 ----
    void set_error(const wchar_t* what, HRESULT hr)
    {
        std::wostringstream ss;
        ss << what << L" failed: hr=0x" << std::hex << static_cast<unsigned long>(hr)
           << L" (" << hr_brief(hr) << L")";
        std::lock_guard<std::mutex> lk(err_mutex);
        last_error = ss.str();
    }
    void set_error_msg(const std::wstring& msg)
    {
        std::lock_guard<std::mutex> lk(err_mutex);
        last_error = msg;
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
        if (com_init_self) {
            CoUninitialize();
            com_init_self = false;
        }
        buffer_frames = 0;
        frame_bytes   = 0;
    }

    void render_proc();
};

// -----------------------------------------------------------------------------

WasapiExclusiveOutput::WasapiExclusiveOutput()
    : d_(std::make_unique<Impl>()) {}

WasapiExclusiveOutput::~WasapiExclusiveOutput()
{
    close();
}

OutputState WasapiExclusiveOutput::state() const
{
    return d_->state.load(std::memory_order_acquire);
}

std::wstring WasapiExclusiveOutput::lastError() const
{
    return d_->get_error();
}

void WasapiExclusiveOutput::setDataCallback(DataCallback cb)
{
    d_->callback = std::move(cb);
}

void WasapiExclusiveOutput::setErrorCallback(ErrorCallback cb)
{
    d_->err_callback = std::move(cb);
}

RenderStats WasapiExclusiveOutput::renderStats() const
{
    RenderStats r;
    r.periods_total = d_->stat_periods.load(std::memory_order_acquire);
    r.frames_total  = d_->stat_frames.load(std::memory_order_acquire);
    r.underruns     = d_->stat_underruns.load(std::memory_order_acquire);
    r.glitch_frames = d_->stat_glitch_frames.load(std::memory_order_acquire);
    return r;
}

// -----------------------------------------------------------------------------

bool WasapiExclusiveOutput::open(const AudioFormat& fmt, const OpenOptions& opts, OpenResult* result)
{
    if (d_->state.load() != OutputState::Closed) {
        d_->set_error_msg(L"open() called in non-Closed state");
        return false;
    }
    if (!fmt.valid()) {
        d_->set_error_msg(L"AudioFormat is invalid");
        return false;
    }
    d_->requested = fmt;

    // ---------- 1) COM (best-effort MTA) ----------
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (hr == RPC_E_CHANGED_MODE) {
        // 调用线程已是 STA(常见于 Qt GUI 线程),WASAPI 多数 API 仍可工作
        d_->com_init_self = false;
    } else if (SUCCEEDED(hr)) {
        d_->com_init_self = true;   // S_OK 才需要 CoUninitialize 配对
        if (hr == S_FALSE) d_->com_init_self = false;
    } else {
        d_->set_error(L"CoInitializeEx", hr);
        return false;
    }

    // ---------- 2) 枚举 + 选设备 ----------
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

    // ---------- 3) 激活 IAudioClient ----------
    hr = d_->device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                              reinterpret_cast<void**>(&d_->client));
    if (FAILED(hr)) { d_->set_error(L"IMMDevice::Activate", hr); goto Fail; }

    // ---------- 4) 构造 WAVEFORMATEXTENSIBLE 并查询是否支持 ----------
    if (!build_wfx(fmt, d_->wfx)) {
        d_->set_error_msg(L"build_wfx failed (invalid AudioFormat)");
        goto Fail;
    }
    hr = d_->client->IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                       reinterpret_cast<WAVEFORMATEX*>(&d_->wfx),
                                       nullptr);
    if (hr != S_OK) {
        d_->set_error(L"IsFormatSupported (exclusive)", hr);
        goto Fail;
    }

    // ---------- 5) 设备周期 + Initialize(独占 + 事件驱动)----------
    {
        REFERENCE_TIME min_period = 0;
        hr = d_->client->GetDevicePeriod(&d_->device_period, &min_period);
        if (FAILED(hr)) { d_->set_error(L"GetDevicePeriod", hr); goto Fail; }
    }

    hr = d_->client->Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                d_->device_period, d_->device_period,
                                reinterpret_cast<WAVEFORMATEX*>(&d_->wfx), nullptr);

    if (hr == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED) {
        // 对齐重试:按 GetBufferSize 返回的帧数反算 REFERENCE_TIME,重新 Activate+Initialize
        UINT32 aligned_frames = 0;
        if (SUCCEEDED(d_->client->GetBufferSize(&aligned_frames)) && aligned_frames > 0) {
            const REFERENCE_TIME aligned = static_cast<REFERENCE_TIME>(
                (static_cast<double>(kRefTimesPerSec) / d_->wfx.Format.nSamplesPerSec) * aligned_frames + 0.5);
            d_->client->Release();
            d_->client = nullptr;
            hr = d_->device->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                                      reinterpret_cast<void**>(&d_->client));
            if (SUCCEEDED(hr)) {
                hr = d_->client->Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                            AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                            aligned, aligned,
                                            reinterpret_cast<WAVEFORMATEX*>(&d_->wfx), nullptr);
            }
        }
    }
    if (FAILED(hr)) { d_->set_error(L"IAudioClient::Initialize", hr); goto Fail; }

    hr = d_->client->GetBufferSize(&d_->buffer_frames);
    if (FAILED(hr)) { d_->set_error(L"GetBufferSize", hr); goto Fail; }

    // ---------- 6) 事件 + Render 服务 ----------
    d_->event = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!d_->event) {
        d_->set_error(L"CreateEvent", HRESULT_FROM_WIN32(GetLastError()));
        goto Fail;
    }
    hr = d_->client->SetEventHandle(d_->event);
    if (FAILED(hr)) { d_->set_error(L"SetEventHandle", hr); goto Fail; }

    hr = d_->client->GetService(__uuidof(IAudioRenderClient),
                                reinterpret_cast<void**>(&d_->render));
    if (FAILED(hr)) { d_->set_error(L"GetService(IAudioRenderClient)", hr); goto Fail; }

    // ---------- 完成 ----------
    d_->frame_bytes = d_->wfx.Format.nBlockAlign;
    d_->state.store(OutputState::Stopped, std::memory_order_release);
    // 统计清零(独占会话开始)
    d_->stat_periods.store(0);
    d_->stat_frames.store(0);
    d_->stat_underruns.store(0);
    d_->stat_glitch_frames.store(0);

    if (result) {
        result->actual_format  = fmt;
        result->buffer_frames  = d_->buffer_frames;
        result->buffer_ms      = d_->buffer_frames * 1000.0 / d_->wfx.Format.nSamplesPerSec;
        result->period_ms      = d_->device_period / 10000.0;
        result->device_name    = d_->device_name;
        result->device_id      = d_->device_id;
    }
    return true;

Fail:
    d_->release_all();
    d_->state.store(OutputState::Closed, std::memory_order_release);
    return false;
}

// -----------------------------------------------------------------------------

void WasapiExclusiveOutput::close()
{
    if (d_->state.load() == OutputState::Running) stop();
    d_->release_all();
    d_->state.store(OutputState::Closed, std::memory_order_release);
}

bool WasapiExclusiveOutput::start()
{
    const OutputState st = d_->state.load();
    if (st != OutputState::Stopped) {
        d_->set_error_msg(L"start() requires Stopped state");
        return false;
    }
    if (!d_->callback) {
        d_->set_error_msg(L"start() called without DataCallback");
        return false;
    }

    // 预填充一次 buffer,避免 Start 后立刻欠载
    {
        BYTE* p = nullptr;
        HRESULT hr = d_->render->GetBuffer(d_->buffer_frames, &p);
        if (FAILED(hr)) { d_->set_error(L"Initial GetBuffer", hr); return false; }
        const std::size_t bytes = static_cast<std::size_t>(d_->buffer_frames) * d_->frame_bytes;
        std::size_t got = d_->callback(reinterpret_cast<std::uint8_t*>(p), bytes);
        if (got > bytes) got = bytes;
        if (got < bytes) std::memset(p + got, 0, bytes - got);
        d_->render->ReleaseBuffer(d_->buffer_frames, 0);
    }

    HRESULT hr = d_->client->Start();
    if (FAILED(hr)) { d_->set_error(L"IAudioClient::Start", hr); return false; }

    d_->running.store(true, std::memory_order_release);
    d_->state.store(OutputState::Running, std::memory_order_release);
    d_->render_thread = std::thread([this]{ d_->render_proc(); });
    return true;
}

void WasapiExclusiveOutput::stop()
{
    if (d_->state.load() != OutputState::Running) return;

    d_->running.store(false, std::memory_order_release);
    if (d_->event) SetEvent(d_->event);                 // 唤醒可能在 wait 的线程
    if (d_->render_thread.joinable()) d_->render_thread.join();

    if (d_->client) d_->client->Stop();
    d_->state.store(OutputState::Stopped, std::memory_order_release);
}

// -----------------------------------------------------------------------------

void WasapiExclusiveOutput::Impl::render_proc()
{
    // 渲染线程独立 COM init(MTA),与控制线程解耦
    HRESULT hr_com = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    bool thread_com_init = SUCCEEDED(hr_com) && hr_com != S_FALSE;

    DWORD  task_idx = 0;
    HANDLE mm_task  = AvSetMmThreadCharacteristicsW(L"Pro Audio", &task_idx);
    // 提权失败不致命,继续以普通线程优先级运行

    const std::size_t bytes_per_period =
        static_cast<std::size_t>(buffer_frames) * frame_bytes;

    auto fire_error = [this]() {
        if (err_callback) err_callback(get_error());
    };

    while (running.load(std::memory_order_acquire)) {
        const DWORD wr = WaitForSingleObject(event, 2000);
        if (!running.load(std::memory_order_acquire)) break;
        if (wr != WAIT_OBJECT_0) {
            // 超时通常意味着设备掉线;退出并标记 Error
            set_error_msg(L"WASAPI render-thread wait timeout");
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }

        BYTE* p = nullptr;
        HRESULT hr = render->GetBuffer(buffer_frames, &p);
        if (FAILED(hr)) {
            set_error(L"GetBuffer (render)", hr);
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }

        std::size_t got = 0;
        if (callback) got = callback(reinterpret_cast<std::uint8_t*>(p), bytes_per_period);
        if (got > bytes_per_period) got = bytes_per_period;
        if (got < bytes_per_period) {
            // 欠载:静音补齐,避免脏数据噪声
            std::memset(p + got, 0, bytes_per_period - got);
            stat_underruns.fetch_add(1, std::memory_order_release);
            stat_glitch_frames.fetch_add((bytes_per_period - got) / frame_bytes,
                                         std::memory_order_release);
        }
        stat_periods.fetch_add(1, std::memory_order_release);
        stat_frames.fetch_add(buffer_frames, std::memory_order_release);

        hr = render->ReleaseBuffer(buffer_frames, 0);
        if (FAILED(hr)) {
            set_error(L"ReleaseBuffer (render)", hr);
            state.store(OutputState::Error, std::memory_order_release);
            fire_error();
            break;
        }
    }

    if (mm_task) AvRevertMmThreadCharacteristics(mm_task);
    if (thread_com_init) CoUninitialize();
}

} // namespace apx::wasapi
