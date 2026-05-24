// =============================================================================
//  core/decoder/MfMediaDecoder.cpp
//
//  通过 Source Reader API 读音频。设计:
//    1) MFStartup 一次 (引用计数,可重入)
//    2) MFCreateSourceReaderFromURL 打开文件
//    3) SetCurrentMediaType 把输出固定到 PCM Int16
//    4) ReadSample 循环 → 累积 leftover_ → 满足 read 请求即返回
//
//  Seek 用 MF 的 100ns 单位:_seek 把 frame → 100ns,SetCurrentPosition,清 leftover。
// =============================================================================
#include "MfMediaDecoder.h"

#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <mfapi.h>
#include <mfidl.h>
#include <mfreadwrite.h>
#include <propvarutil.h>

#include <atomic>
#include <sstream>
#include <vector>

#pragma comment(lib, "mfplat.lib")
#pragma comment(lib, "mfreadwrite.lib")
#pragma comment(lib, "mfuuid.lib")
#pragma comment(lib, "propsys.lib")

namespace apx {

namespace {

// 进程级 MFStartup 引用计数,避免并发 decoder 反复 Startup/Shutdown 引爆 MF
std::atomic<int> g_mf_refs{0};
std::atomic<bool> g_mf_started{false};

bool mf_startup_ref()
{
    int prev = g_mf_refs.fetch_add(1, std::memory_order_acq_rel);
    if (prev == 0) {
        HRESULT hr = MFStartup(MF_VERSION, MFSTARTUP_LITE);
        if (FAILED(hr)) {
            g_mf_refs.fetch_sub(1, std::memory_order_acq_rel);
            return false;
        }
        g_mf_started.store(true, std::memory_order_release);
    }
    return true;
}
void mf_shutdown_ref()
{
    int prev = g_mf_refs.fetch_sub(1, std::memory_order_acq_rel);
    if (prev == 1) {
        if (g_mf_started.exchange(false)) MFShutdown();
    }
}

} // namespace

struct MfMediaDecoder::Impl {
    IMFSourceReader* reader = nullptr;
    bool             mf_started = false;       // 本实例是否持有 MF 引用
    bool             com_init   = false;

    AudioFormat      fmt{};
    std::int64_t     total_frames = 0;
    std::int64_t     cur_frame    = 0;
    std::uint32_t    frame_bytes  = 0;
    bool             eof          = false;

    // ReadSample 给我们的字节有可能比 read 请求多;尾巴存到这里
    std::vector<std::uint8_t> leftover;
    std::size_t      leftover_pos = 0;

    std::wstring     last_error;

    void set_err(const wchar_t* what, HRESULT hr) {
        std::wostringstream ss;
        ss << what << L" failed: hr=0x" << std::hex << static_cast<unsigned long>(hr);
        last_error = ss.str();
    }
};

MfMediaDecoder::MfMediaDecoder()  : d_(std::make_unique<Impl>()) {}
MfMediaDecoder::~MfMediaDecoder() { close(); }

bool         MfMediaDecoder::isOpen()       const { return d_->reader != nullptr; }
AudioFormat  MfMediaDecoder::format()       const { return d_->fmt; }
std::int64_t MfMediaDecoder::totalFrames()  const { return d_->total_frames; }
std::int64_t MfMediaDecoder::currentFrame() const { return d_->cur_frame; }
std::wstring MfMediaDecoder::lastError()    const { return d_->last_error; }

void MfMediaDecoder::close()
{
    if (d_->reader) { d_->reader->Release(); d_->reader = nullptr; }
    if (d_->mf_started) { mf_shutdown_ref(); d_->mf_started = false; }
    if (d_->com_init)   { CoUninitialize();  d_->com_init   = false; }
    d_->fmt = {};
    d_->total_frames = 0;
    d_->cur_frame    = 0;
    d_->frame_bytes  = 0;
    d_->eof          = false;
    d_->leftover.clear();
    d_->leftover_pos = 0;
}

bool MfMediaDecoder::open(const std::wstring& path)
{
    if (d_->reader) close();

    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (hr == RPC_E_CHANGED_MODE) d_->com_init = false;
    else if (SUCCEEDED(hr))       d_->com_init = (hr == S_OK);
    else { d_->set_err(L"CoInitializeEx", hr); return false; }

    if (!mf_startup_ref()) {
        d_->last_error = L"MFStartup failed";
        if (d_->com_init) { CoUninitialize(); d_->com_init = false; }
        return false;
    }
    d_->mf_started = true;

    hr = MFCreateSourceReaderFromURL(path.c_str(), nullptr, &d_->reader);
    if (FAILED(hr)) {
        d_->set_err(L"MFCreateSourceReaderFromURL", hr);
        close();
        return false;
    }

    // 只关心音频流
    hr = d_->reader->SetStreamSelection(
        (DWORD)MF_SOURCE_READER_ALL_STREAMS, FALSE);
    if (SUCCEEDED(hr)) {
        hr = d_->reader->SetStreamSelection(
            (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, TRUE);
    }
    if (FAILED(hr)) { d_->set_err(L"SetStreamSelection", hr); close(); return false; }

    // 强制输出为 PCM Int16
    IMFMediaType* want = nullptr;
    hr = MFCreateMediaType(&want);
    if (SUCCEEDED(hr)) hr = want->SetGUID(MF_MT_MAJOR_TYPE, MFMediaType_Audio);
    if (SUCCEEDED(hr)) hr = want->SetGUID(MF_MT_SUBTYPE,    MFAudioFormat_PCM);
    if (SUCCEEDED(hr)) hr = want->SetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE, 16);
    if (FAILED(hr)) {
        if (want) want->Release();
        d_->set_err(L"MFCreateMediaType(want)", hr);
        close(); return false;
    }
    hr = d_->reader->SetCurrentMediaType(
        (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, nullptr, want);
    want->Release();
    if (FAILED(hr)) { d_->set_err(L"SetCurrentMediaType(PCM 16)", hr); close(); return false; }

    // 拿协商后的 sample_rate / channels
    IMFMediaType* cur = nullptr;
    hr = d_->reader->GetCurrentMediaType(
        (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM, &cur);
    if (FAILED(hr)) { d_->set_err(L"GetCurrentMediaType", hr); close(); return false; }
    UINT32 sr = 0, ch = 0, bps = 16;
    cur->GetUINT32(MF_MT_AUDIO_SAMPLES_PER_SECOND, &sr);
    cur->GetUINT32(MF_MT_AUDIO_NUM_CHANNELS,        &ch);
    cur->GetUINT32(MF_MT_AUDIO_BITS_PER_SAMPLE,     &bps);
    cur->Release();

    if (sr == 0 || ch == 0 || bps != 16) {
        d_->last_error = L"MF produced unexpected format";
        close(); return false;
    }

    AudioFormat fmt;
    fmt.sample_rate     = sr;
    fmt.channels        = static_cast<std::uint16_t>(ch);
    fmt.bits_per_sample = 16;
    fmt.valid_bits      = 16;
    fmt.sample_type     = SampleType::Int16;
    fmt.channel_mask    = default_channel_mask(fmt.channels);
    if (!fmt.valid()) { d_->last_error = L"MF: invalid AudioFormat"; close(); return false; }

    d_->fmt         = fmt;
    d_->frame_bytes = fmt.frame_bytes();
    d_->cur_frame   = 0;
    d_->eof         = false;

    // 总帧数 = duration_100ns * sample_rate / 1e7
    d_->total_frames = 0;
    PROPVARIANT pv; PropVariantInit(&pv);
    hr = d_->reader->GetPresentationAttribute(
        (DWORD)MF_SOURCE_READER_MEDIASOURCE, MF_PD_DURATION, &pv);
    if (SUCCEEDED(hr) && pv.vt == VT_UI8) {
        const std::uint64_t dur_100ns = pv.uhVal.QuadPart;
        d_->total_frames = static_cast<std::int64_t>(
            (dur_100ns * static_cast<std::uint64_t>(sr) + 5'000'000ULL) / 10'000'000ULL);
    }
    PropVariantClear(&pv);

    return true;
}

bool MfMediaDecoder::seek(std::int64_t frame)
{
    if (!d_->reader) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    if (d_->total_frames > 0 && frame > d_->total_frames) frame = d_->total_frames;

    // frame → 100ns
    const std::uint64_t pos_100ns =
        (static_cast<std::uint64_t>(frame) * 10'000'000ULL)
        / static_cast<std::uint64_t>(d_->fmt.sample_rate);
    PROPVARIANT pv; PropVariantInit(&pv);
    pv.vt = VT_I8;
    pv.hVal.QuadPart = static_cast<LONGLONG>(pos_100ns);
    HRESULT hr = d_->reader->SetCurrentPosition(GUID_NULL, pv);
    PropVariantClear(&pv);
    if (FAILED(hr)) { d_->set_err(L"SetCurrentPosition", hr); return false; }

    d_->cur_frame    = frame;
    d_->eof          = false;
    d_->leftover.clear();
    d_->leftover_pos = 0;
    return true;
}

std::size_t MfMediaDecoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->reader || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;
    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    std::size_t written = 0;

    auto drain_leftover = [&]() {
        const std::size_t avail = d_->leftover.size() - d_->leftover_pos;
        if (avail == 0) return;
        const std::size_t copy = std::min(avail, bytes - written);
        std::memcpy(dst + written, d_->leftover.data() + d_->leftover_pos, copy);
        written += copy;
        d_->leftover_pos += copy;
        if (d_->leftover_pos >= d_->leftover.size()) {
            d_->leftover.clear();
            d_->leftover_pos = 0;
        }
    };

    drain_leftover();

    while (written < bytes && !d_->eof) {
        DWORD flags = 0;
        IMFSample* sample = nullptr;
        HRESULT hr = d_->reader->ReadSample(
            (DWORD)MF_SOURCE_READER_FIRST_AUDIO_STREAM,
            0, nullptr, &flags, nullptr, &sample);
        if (FAILED(hr)) {
            d_->set_err(L"ReadSample", hr);
            break;
        }
        if (flags & MF_SOURCE_READERF_ENDOFSTREAM) { d_->eof = true; }
        if (!sample) {
            // EOS 时 sample 通常为 null
            if (d_->eof) break;
            continue;
        }
        IMFMediaBuffer* buf = nullptr;
        hr = sample->ConvertToContiguousBuffer(&buf);
        if (FAILED(hr) || !buf) {
            sample->Release();
            d_->set_err(L"ConvertToContiguousBuffer", hr);
            break;
        }
        BYTE* p = nullptr; DWORD len = 0;
        hr = buf->Lock(&p, nullptr, &len);
        if (FAILED(hr) || !p) {
            buf->Release(); sample->Release();
            d_->set_err(L"IMFMediaBuffer::Lock", hr);
            break;
        }
        // 写入 dst,尾巴存 leftover
        const std::size_t copy = std::min<std::size_t>(len, bytes - written);
        std::memcpy(dst + written, p, copy);
        written += copy;
        if (copy < len) {
            d_->leftover.assign(p + copy, p + len);
            d_->leftover_pos = 0;
        }
        buf->Unlock();
        buf->Release();
        sample->Release();
    }

    const std::size_t aligned = (written / d_->frame_bytes) * d_->frame_bytes;
    // 多余的非整帧字节回填到 leftover 头
    if (aligned < written) {
        std::vector<std::uint8_t> tail(dst + aligned, dst + written);
        if (!d_->leftover.empty()) {
            // 极少见:tail + 已有 leftover 都先拿掉,合并再放回
            tail.insert(tail.end(),
                        d_->leftover.begin() + d_->leftover_pos,
                        d_->leftover.end());
            d_->leftover_pos = 0;
        }
        d_->leftover = std::move(tail);
    }
    d_->cur_frame += static_cast<std::int64_t>(aligned / d_->frame_bytes);
    return aligned;
}

} // namespace apx
