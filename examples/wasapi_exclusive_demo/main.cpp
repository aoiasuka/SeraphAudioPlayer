// =============================================================================
//  wasapi_exclusive_demo
//  ---------------------------------------------------------------------------
//  AudioPlayerX86 项目的 WASAPI 独占模式技术验证 demo。
//
//  目标:用最短的代码完整跑通独占模式的标准链路,作为后续 platform/wasapi/
//  模块封装的参考。本程序不依赖任何第三方库,纯 Win32 + Core Audio API。
//
//  流程:
//    1) CoInitializeEx (MTA)
//    2) 枚举默认渲染设备 (IMMDeviceEnumerator)
//    3) 激活 IAudioClient
//    4) 用 WAVEFORMATEXTENSIBLE 协商独占模式格式
//       - 优先尝试 16-bit / 44.1 kHz PCM
//       - 失败则回退 32-bit float / 44.1 kHz
//    5) 取设备默认周期,Initialize(EXCLUSIVE | EVENTCALLBACK)
//       - 处理 AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED 重试
//    6) CreateEvent + SetEventHandle
//    7) GetService(IAudioRenderClient)
//    8) AvSetMmThreadCharacteristicsW("Pro Audio") 提升优先级
//    9) 预填充一帧静音,Start
//   10) 事件循环 5 秒:每次事件 → GetBuffer → 生成正弦波 → ReleaseBuffer
//   11) Stop + 清理
//
//  编译运行后:在默认音频设备上听到 5 秒 1 kHz 正弦波。
//  控制台会打印协商出的格式、缓冲区大小、设备周期等关键信息。
// =============================================================================

#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mmdeviceapi.h>
#include <audioclient.h>
#include <avrt.h>
#include <ksmedia.h>
#include <functiondiscoverykeys_devpkey.h>

#include <cstdio>
#include <cstdint>
#include <cmath>

// 部分 SDK / 编译器组合下,CLSID/IID 常量需要显式实例化
#include <initguid.h>

#pragma comment(lib, "ole32.lib")
#pragma comment(lib, "avrt.lib")

namespace {

constexpr double  kPi          = 3.14159265358979323846;
constexpr int     kPlaySeconds = 5;
constexpr double  kToneFreqHz  = 1000.0;
constexpr double  kAmplitude   = 0.2;          // -14 dBFS,温和不刺耳

constexpr REFERENCE_TIME kRefTimesPerSec = 10'000'000; // 100ns 单位

// ---------- 工具:HRESULT 友好打印 ----------
const wchar_t* hr_brief(HRESULT hr)
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

void log_ok  (const wchar_t* fmt, ...) { va_list a; va_start(a,fmt); std::fputws(L"[ OK ] ", stdout); std::vfwprintf(stdout, fmt, a); std::fputwc(L'\n', stdout); va_end(a); }
void log_info(const wchar_t* fmt, ...) { va_list a; va_start(a,fmt); std::fputws(L"[INFO] ", stdout); std::vfwprintf(stdout, fmt, a); std::fputwc(L'\n', stdout); va_end(a); }
void log_warn(const wchar_t* fmt, ...) { va_list a; va_start(a,fmt); std::fputws(L"[WARN] ", stdout); std::vfwprintf(stdout, fmt, a); std::fputwc(L'\n', stdout); va_end(a); }
void log_err (const wchar_t* fmt, ...) { va_list a; va_start(a,fmt); std::fputws(L"[FAIL] ", stderr); std::vfwprintf(stderr, fmt, a); std::fputwc(L'\n', stderr); va_end(a); }

#define HR_FAIL(hr, what) do { \
    log_err(L"%s failed: hr=0x%08lX (%s)", L##what, (unsigned long)(hr), hr_brief(hr)); \
    goto Cleanup; \
} while (0)

#define HR_CHECK(hr, what) do { if (FAILED(hr)) HR_FAIL(hr, what); } while (0)

// ---------- 格式构造 ----------
void build_format_pcm16(WAVEFORMATEXTENSIBLE& w, DWORD sr, WORD ch)
{
    ZeroMemory(&w, sizeof(w));
    w.Format.wFormatTag      = WAVE_FORMAT_EXTENSIBLE;
    w.Format.nChannels       = ch;
    w.Format.nSamplesPerSec  = sr;
    w.Format.wBitsPerSample  = 16;
    w.Format.nBlockAlign     = (WORD)(ch * 2);
    w.Format.nAvgBytesPerSec = sr * w.Format.nBlockAlign;
    w.Format.cbSize          = sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX);
    w.Samples.wValidBitsPerSample = 16;
    w.dwChannelMask          = (ch == 2) ? (SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT) : 0;
    w.SubFormat              = KSDATAFORMAT_SUBTYPE_PCM;
}

void build_format_float32(WAVEFORMATEXTENSIBLE& w, DWORD sr, WORD ch)
{
    ZeroMemory(&w, sizeof(w));
    w.Format.wFormatTag      = WAVE_FORMAT_EXTENSIBLE;
    w.Format.nChannels       = ch;
    w.Format.nSamplesPerSec  = sr;
    w.Format.wBitsPerSample  = 32;
    w.Format.nBlockAlign     = (WORD)(ch * 4);
    w.Format.nAvgBytesPerSec = sr * w.Format.nBlockAlign;
    w.Format.cbSize          = sizeof(WAVEFORMATEXTENSIBLE) - sizeof(WAVEFORMATEX);
    w.Samples.wValidBitsPerSample = 32;
    w.dwChannelMask          = (ch == 2) ? (SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT) : 0;
    w.SubFormat              = KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
}

// ---------- 正弦波填充 ----------
struct SineGen {
    double phase    = 0.0;
    double phaseInc = 0.0;
    bool   isFloat  = false;
    WORD   channels = 2;

    void prepare(double freqHz, DWORD sampleRate, bool isFloatFmt, WORD ch)
    {
        phase    = 0.0;
        phaseInc = 2.0 * kPi * freqHz / (double)sampleRate;
        isFloat  = isFloatFmt;
        channels = ch;
    }

    void render(BYTE* dst, UINT32 frames)
    {
        for (UINT32 i = 0; i < frames; ++i) {
            const double s = kAmplitude * std::sin(phase);
            phase += phaseInc;
            if (phase >= 2.0 * kPi) phase -= 2.0 * kPi;

            if (isFloat) {
                float* p = reinterpret_cast<float*>(dst) + i * channels;
                for (WORD c = 0; c < channels; ++c) p[c] = (float)s;
            } else {
                int16_t* p = reinterpret_cast<int16_t*>(dst) + i * channels;
                const int16_t v = (int16_t)(s * 32767.0);
                for (WORD c = 0; c < channels; ++c) p[c] = v;
            }
        }
    }
};

// ---------- 打印设备友好名 ----------
void print_device_name(IMMDevice* dev)
{
    IPropertyStore* props = nullptr;
    if (FAILED(dev->OpenPropertyStore(STGM_READ, &props)) || !props) return;
    PROPVARIANT pv; PropVariantInit(&pv);
    if (SUCCEEDED(props->GetValue(PKEY_Device_FriendlyName, &pv)) && pv.vt == VT_LPWSTR) {
        log_ok(L"Default render device: %s", pv.pwszVal);
    }
    PropVariantClear(&pv);
    props->Release();
}

} // namespace

// =============================================================================

int wmain()
{
    HRESULT               hr        = S_OK;
    IMMDeviceEnumerator*  pEnum     = nullptr;
    IMMDevice*            pDevice   = nullptr;
    IAudioClient*         pClient   = nullptr;
    IAudioRenderClient*   pRender   = nullptr;
    HANDLE                hEvent    = nullptr;
    HANDLE                hMmTask   = nullptr;
    DWORD                 taskIndex = 0;

    WAVEFORMATEXTENSIBLE  wfx       = {};
    UINT32                bufFrames = 0;
    REFERENCE_TIME        defPeriod = 0;
    REFERENCE_TIME        minPeriod = 0;
    int                   exitCode  = 0;

    log_info(L"=== WASAPI Exclusive Mode Demo ===");

    // 1) COM
    hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    HR_CHECK(hr, "CoInitializeEx");

    // 2) 枚举器 + 默认渲染端点
    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                          __uuidof(IMMDeviceEnumerator), reinterpret_cast<void**>(&pEnum));
    HR_CHECK(hr, "CoCreateInstance(MMDeviceEnumerator)");

    hr = pEnum->GetDefaultAudioEndpoint(eRender, eConsole, &pDevice);
    HR_CHECK(hr, "GetDefaultAudioEndpoint");
    print_device_name(pDevice);

    // 3) 激活 IAudioClient
    hr = pDevice->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                           reinterpret_cast<void**>(&pClient));
    HR_CHECK(hr, "IMMDevice::Activate(IAudioClient)");

    // 4) 格式协商: 优先 16/44.1 PCM,失败回退 32-bit float
    build_format_pcm16(wfx, 44100, 2);
    hr = pClient->IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                    reinterpret_cast<WAVEFORMATEX*>(&wfx),
                                    nullptr);
    if (hr != S_OK) {
        log_warn(L"16-bit PCM 44.1 kHz not supported (hr=0x%08lX), trying 32-bit float...",
                 (unsigned long)hr);
        build_format_float32(wfx, 44100, 2);
        hr = pClient->IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                        reinterpret_cast<WAVEFORMATEX*>(&wfx),
                                        nullptr);
        HR_CHECK(hr, "IsFormatSupported");
    }
    log_ok(L"Negotiated format: %lu Hz, %u-bit %s, %u ch",
           wfx.Format.nSamplesPerSec, wfx.Format.wBitsPerSample,
           (wfx.SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT) ? L"float" : L"int",
           wfx.Format.nChannels);

    // 5) 设备周期 + Initialize (独占 + 事件驱动)
    hr = pClient->GetDevicePeriod(&defPeriod, &minPeriod);
    HR_CHECK(hr, "GetDevicePeriod");
    log_info(L"Device period: default=%.3f ms, min=%.3f ms",
             defPeriod / 10000.0, minPeriod / 10000.0);

    hr = pClient->Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE,
                             AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                             defPeriod, defPeriod,
                             reinterpret_cast<WAVEFORMATEX*>(&wfx), nullptr);

    if (hr == AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED) {
        // 标准重试套路:用 GetBufferSize 拿到设备对齐后的帧数,反算 REFERENCE_TIME 再 Initialize
        log_warn(L"Initialize returned BUFFER_SIZE_NOT_ALIGNED, retrying with aligned size");
        UINT32 alignedFrames = 0;
        if (SUCCEEDED(pClient->GetBufferSize(&alignedFrames)) && alignedFrames > 0) {
            const REFERENCE_TIME aligned = (REFERENCE_TIME)(
                ((double)kRefTimesPerSec / wfx.Format.nSamplesPerSec) * alignedFrames + 0.5);
            pClient->Release();
            pClient = nullptr;
            hr = pDevice->Activate(__uuidof(IAudioClient), CLSCTX_ALL, nullptr,
                                   reinterpret_cast<void**>(&pClient));
            HR_CHECK(hr, "IMMDevice::Activate (retry)");
            hr = pClient->Initialize(AUDCLNT_SHAREMODE_EXCLUSIVE,
                                     AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                                     aligned, aligned,
                                     reinterpret_cast<WAVEFORMATEX*>(&wfx), nullptr);
        }
    }
    HR_CHECK(hr, "IAudioClient::Initialize");

    hr = pClient->GetBufferSize(&bufFrames);
    HR_CHECK(hr, "GetBufferSize");
    log_ok(L"Buffer size: %u frames (%.3f ms)",
           bufFrames, bufFrames * 1000.0 / wfx.Format.nSamplesPerSec);

    // 6) 事件句柄
    hEvent = CreateEventW(nullptr, FALSE, FALSE, nullptr);
    if (!hEvent) { hr = HRESULT_FROM_WIN32(GetLastError()); HR_FAIL(hr, "CreateEvent"); }
    hr = pClient->SetEventHandle(hEvent);
    HR_CHECK(hr, "SetEventHandle");

    // 7) 渲染客户端
    hr = pClient->GetService(__uuidof(IAudioRenderClient),
                             reinterpret_cast<void**>(&pRender));
    HR_CHECK(hr, "GetService(IAudioRenderClient)");

    // 8) AVRT 提权
    hMmTask = AvSetMmThreadCharacteristicsW(L"Pro Audio", &taskIndex);
    if (hMmTask) log_ok(L"MMCSS task 'Pro Audio' attached");
    else         log_warn(L"AvSetMmThreadCharacteristics failed (err=%lu)", GetLastError());

    // 9) 预填充 + Start
    {
        BYTE* pData = nullptr;
        hr = pRender->GetBuffer(bufFrames, &pData);
        HR_CHECK(hr, "Initial GetBuffer");
        // 用首段正弦波预填充,衔接自然
        SineGen warmup;
        warmup.prepare(kToneFreqHz, wfx.Format.nSamplesPerSec,
                       wfx.SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
                       wfx.Format.nChannels);
        warmup.render(pData, bufFrames);
        hr = pRender->ReleaseBuffer(bufFrames, 0);
        HR_CHECK(hr, "Initial ReleaseBuffer");
    }

    hr = pClient->Start();
    HR_CHECK(hr, "IAudioClient::Start");
    log_ok(L"Playback started — %d seconds of %.0f Hz tone", kPlaySeconds, kToneFreqHz);

    // 10) 事件循环
    {
        SineGen gen;
        gen.prepare(kToneFreqHz, wfx.Format.nSamplesPerSec,
                    wfx.SubFormat == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT,
                    wfx.Format.nChannels);

        const UINT64 totalFrames = (UINT64)wfx.Format.nSamplesPerSec * kPlaySeconds;
        UINT64       written     = bufFrames;     // 已经预填充一帧
        UINT32       glitches    = 0;

        while (written < totalFrames) {
            DWORD w = WaitForSingleObject(hEvent, 2000);
            if (w != WAIT_OBJECT_0) {
                log_err(L"Event wait timeout (rc=%lu)", w);
                ++glitches;
                break;
            }
            BYTE* pData = nullptr;
            hr = pRender->GetBuffer(bufFrames, &pData);
            if (FAILED(hr)) {
                log_err(L"GetBuffer in loop: hr=0x%08lX (%s)",
                        (unsigned long)hr, hr_brief(hr));
                break;
            }
            gen.render(pData, bufFrames);
            hr = pRender->ReleaseBuffer(bufFrames, 0);
            if (FAILED(hr)) {
                log_err(L"ReleaseBuffer in loop: hr=0x%08lX", (unsigned long)hr);
                break;
            }
            written += bufFrames;
        }

        log_ok(L"Rendered %llu frames (target %llu), glitches=%u",
               (unsigned long long)written, (unsigned long long)totalFrames, glitches);
    }

    // 等待最后一帧出声
    Sleep((DWORD)(bufFrames * 1000 / wfx.Format.nSamplesPerSec + 50));

    hr = pClient->Stop();
    if (FAILED(hr)) log_warn(L"Stop hr=0x%08lX", (unsigned long)hr);

    log_ok(L"Done.");

Cleanup:
    if (hMmTask) AvRevertMmThreadCharacteristics(hMmTask);
    if (pRender) { pRender->Release(); pRender = nullptr; }
    if (pClient) { pClient->Release(); pClient = nullptr; }
    if (pDevice) { pDevice->Release(); pDevice = nullptr; }
    if (pEnum)   { pEnum->Release();   pEnum   = nullptr; }
    if (hEvent)  { CloseHandle(hEvent); hEvent  = nullptr; }
    CoUninitialize();

    if (FAILED(hr)) exitCode = 1;
    return exitCode;
}
