// =============================================================================
//  platform/asio/AsioOutput.cpp
//
//  - 有 ASIO SDK 时:实现真正的 ASIO 输出 (枚举/初始化/缓冲回调/启停)
//  - 无 ASIO SDK 时:全部方法返回失败,enumerate() 返回空 vector
// =============================================================================
#include "AsioOutput.h"

#include <atomic>
#include <mutex>
#include <sstream>
#include <vector>

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#if APX_HAVE_ASIO_SDK

// ASIO SDK 头文件 (用户复制到 third_party/asiosdk)
#  include "asiosdk/common/asio.h"
#  include "asiosdk/host/asiodrivers.h"

namespace apx {

namespace {
AsioDrivers* g_drivers = nullptr;
AsioDrivers& drivers() {
    if (!g_drivers) g_drivers = new AsioDrivers();
    return *g_drivers;
}
} // namespace

struct AsioOutput::Impl {
    int                          deviceIndex = 0;
    bool                         driverLoaded = false;
    AudioFormat                  fmt{};
    OutputState                  st = OutputState::Closed;
    DataCallback                 cb;
    std::wstring                 lastErr;
    std::mutex                   cbMutex;
    long                         inputChannels = 0;
    long                         outputChannels = 0;
    long                         minSize = 0, maxSize = 0, prefSize = 0, granularity = 0;
    std::vector<ASIOBufferInfo>  bufferInfos;
    std::vector<ASIOChannelInfo> channelInfos;
    bool                         buffersCreated = false;
    std::atomic<bool>            running{false};
};

// ASIO 回调由驱动线程发起,需要静态函数 + 单实例桥接
static AsioOutput::Impl* g_active = nullptr;

static void onBufferSwitch(long index, ASIOBool /*processNow*/)
{
    auto* impl = g_active;
    if (!impl || !impl->running.load()) return;
    if (impl->bufferInfos.empty() || impl->channelInfos.empty()) return;

    const long frames = impl->prefSize;
    const int  ch     = static_cast<int>(impl->bufferInfos.size());
    if (ch < 1 || frames < 1) return;

    // 拉一个 PCM 块,然后分发到各通道 buffer
    // 默认按 Int32 PCM (大多数 ASIO 驱动支持 ASIOSTInt32LSB)
    const std::size_t bytes = static_cast<std::size_t>(frames) * ch * 4;
    std::vector<std::uint8_t> tmp(bytes, 0);

    DataCallback cb;
    { std::lock_guard<std::mutex> lk(impl->cbMutex); cb = impl->cb; }
    if (cb) {
        std::size_t got = cb(tmp.data(), bytes);
        if (got < bytes) std::memset(tmp.data() + got, 0, bytes - got);
    }
    // 解交错到每通道 ASIO buffer
    for (int c = 0; c < ch; ++c) {
        auto* dst = static_cast<int32_t*>(impl->bufferInfos[c].buffers[index]);
        for (long f = 0; f < frames; ++f) {
            std::int32_t s;
            std::memcpy(&s, tmp.data() + (f * ch + c) * 4, 4);
            dst[f] = s;
        }
    }
    ASIOOutputReady();
}

static ASIOTime* onBufferSwitchTime(ASIOTime* params, long index, ASIOBool processNow)
{
    onBufferSwitch(index, processNow);
    return params;
}
static void onSampleRateDidChange(ASIOSampleRate /*sr*/) {}
static long onAsioMessage(long selector, long /*value*/, void*, double*)
{
    switch (selector) {
    case kAsioSelectorSupported: return 1;
    case kAsioEngineVersion:     return 2;
    case kAsioResetRequest:      return 0;
    case kAsioBufferSizeChange:  return 0;
    case kAsioResyncRequest:     return 0;
    case kAsioLatenciesChanged:  return 1;
    case kAsioSupportsTimeInfo:  return 1;
    case kAsioSupportsTimeCode:  return 0;
    default: return 0;
    }
}

AsioOutput::AsioOutput()  : d_(std::make_unique<Impl>()) {}
AsioOutput::~AsioOutput() { close(); }

bool AsioOutput::sdkAvailable() { return true; }

std::vector<AsioDeviceInfo> AsioOutput::enumerate()
{
    std::vector<AsioDeviceInfo> out;
    long n = drivers().asioGetNumDev();
    char name[64];
    for (long i = 0; i < n; ++i) {
        if (drivers().asioGetDriverName(i, name, sizeof(name)) == 0) {
            AsioDeviceInfo di;
            di.index = static_cast<int>(i);
            // ASIO 驱动名是 ANSI;按 ACP 转 wide
            int wlen = MultiByteToWideChar(CP_ACP, 0, name, -1, nullptr, 0);
            if (wlen > 0) {
                std::wstring w(wlen - 1, L'\0');
                MultiByteToWideChar(CP_ACP, 0, name, -1, w.data(), wlen);
                di.name = std::move(w);
            }
            out.push_back(std::move(di));
        }
    }
    return out;
}

void AsioOutput::setDeviceIndex(int idx) { d_->deviceIndex = idx; }

bool AsioOutput::open(const AudioFormat& format, const OpenOptions&, OpenResult* result)
{
    close();
    char name[64] = {0};
    if (drivers().asioGetDriverName(d_->deviceIndex, name, sizeof(name)) != 0) {
        d_->lastErr = L"ASIO: getDriverName failed";
        return false;
    }
    if (!drivers().loadDriver(name)) {
        d_->lastErr = L"ASIO: loadDriver failed";
        return false;
    }
    d_->driverLoaded = true;
    if (ASIOInit(nullptr) != ASE_OK) {
        d_->lastErr = L"ASIO: init failed";
        close();
        return false;
    }
    if (ASIOGetChannels(&d_->inputChannels, &d_->outputChannels) != ASE_OK) {
        d_->lastErr = L"ASIO: getChannels failed";
        close();
        return false;
    }
    if (ASIOGetBufferSize(&d_->minSize, &d_->maxSize, &d_->prefSize, &d_->granularity) != ASE_OK) {
        d_->lastErr = L"ASIO: getBufferSize failed";
        close();
        return false;
    }
    if (ASIOSetSampleRate(static_cast<ASIOSampleRate>(format.sample_rate)) != ASE_OK) {
        d_->lastErr = L"ASIO: setSampleRate failed";
        close();
        return false;
    }

    int ch = std::min<int>(format.channels, static_cast<int>(d_->outputChannels));
    d_->bufferInfos.assign(ch, {});
    d_->channelInfos.assign(ch, {});
    for (int i = 0; i < ch; ++i) {
        d_->bufferInfos[i].isInput    = ASIOFalse;
        d_->bufferInfos[i].channelNum = i;
    }

    static ASIOCallbacks cbs;
    cbs.bufferSwitch         = &onBufferSwitch;
    cbs.bufferSwitchTimeInfo = &onBufferSwitchTime;
    cbs.sampleRateDidChange  = &onSampleRateDidChange;
    cbs.asioMessage          = &onAsioMessage;

    if (ASIOCreateBuffers(d_->bufferInfos.data(), ch, d_->prefSize, &cbs) != ASE_OK) {
        d_->lastErr = L"ASIO: createBuffers failed";
        close();
        return false;
    }
    d_->buffersCreated = true;

    AudioFormat fmt = format;
    fmt.channels        = static_cast<std::uint16_t>(ch);
    fmt.bits_per_sample = 32;
    fmt.valid_bits      = 32;
    fmt.sample_type     = SampleType::Int32;
    fmt.channel_mask    = default_channel_mask(fmt.channels);
    d_->fmt = fmt;
    d_->st  = OutputState::Stopped;

    if (result) {
        result->actual_format = fmt;
        result->buffer_frames = static_cast<std::uint32_t>(d_->prefSize);
        result->buffer_ms     = (1000.0 * d_->prefSize) / fmt.sample_rate;
        result->period_ms     = result->buffer_ms;
        int wlen = MultiByteToWideChar(CP_ACP, 0, name, -1, nullptr, 0);
        if (wlen > 0) {
            std::wstring w(wlen - 1, L'\0');
            MultiByteToWideChar(CP_ACP, 0, name, -1, w.data(), wlen);
            result->device_name = std::move(w);
        }
        result->device_id = L"asio:" + std::to_wstring(d_->deviceIndex);
    }
    return true;
}

void AsioOutput::close()
{
    stop();
    if (d_->buffersCreated) { ASIODisposeBuffers(); d_->buffersCreated = false; }
    if (d_->driverLoaded)   { ASIOExit(); drivers().removeCurrentDriver(); d_->driverLoaded = false; }
    d_->bufferInfos.clear();
    d_->channelInfos.clear();
    d_->st = OutputState::Closed;
    if (g_active == d_.get()) g_active = nullptr;
}

bool AsioOutput::start()
{
    if (d_->st != OutputState::Stopped) {
        d_->lastErr = L"ASIO: not in Stopped state";
        return false;
    }
    g_active = d_.get();
    d_->running.store(true);
    if (ASIOStart() != ASE_OK) {
        d_->lastErr = L"ASIO: start failed";
        d_->running.store(false);
        d_->st = OutputState::Error;
        return false;
    }
    d_->st = OutputState::Running;
    return true;
}

void AsioOutput::stop()
{
    if (d_->st == OutputState::Running) {
        d_->running.store(false);
        ASIOStop();
        d_->st = OutputState::Stopped;
    }
    if (g_active == d_.get()) g_active = nullptr;
}

OutputState  AsioOutput::state()     const { return d_->st; }
std::wstring AsioOutput::lastError() const { return d_->lastErr; }
void         AsioOutput::setDataCallback(DataCallback cb)
{
    std::lock_guard<std::mutex> lk(d_->cbMutex);
    d_->cb = std::move(cb);
}

} // namespace apx

#else  // !APX_HAVE_ASIO_SDK  — 桩实现

namespace apx {

struct AsioOutput::Impl {
    std::wstring lastErr = L"ASIO SDK not present (place SDK under third_party/asiosdk/)";
};

AsioOutput::AsioOutput()  : d_(std::make_unique<Impl>()) {}
AsioOutput::~AsioOutput() = default;

bool                          AsioOutput::sdkAvailable() { return false; }
std::vector<AsioDeviceInfo>   AsioOutput::enumerate()    { return {}; }
void                          AsioOutput::setDeviceIndex(int)   {}

bool AsioOutput::open(const AudioFormat&, const OpenOptions&, OpenResult*) { return false; }
void AsioOutput::close()                                   {}
bool AsioOutput::start()                                   { return false; }
void AsioOutput::stop()                                    {}
OutputState  AsioOutput::state()     const                 { return OutputState::Closed; }
std::wstring AsioOutput::lastError() const                 { return d_->lastErr; }
void         AsioOutput::setDataCallback(DataCallback)     {}

} // namespace apx

#endif // APX_HAVE_ASIO_SDK
