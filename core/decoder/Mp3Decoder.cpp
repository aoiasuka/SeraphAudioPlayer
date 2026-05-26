// =============================================================================
//  core/decoder/Mp3Decoder.cpp
//
//  dr_mp3.h 的 implementation 翻译单元。
// =============================================================================

#include "Mp3Decoder.h"

#if APX_HAVE_DR_MP3

#if defined(_MSC_VER)
#  pragma warning(push)
#  pragma warning(disable: 4127)
#  pragma warning(disable: 4244)
#  pragma warning(disable: 4245)
#  pragma warning(disable: 4267)
#  pragma warning(disable: 4310)
#  pragma warning(disable: 4456)
#  pragma warning(disable: 4457)
#  pragma warning(disable: 4701)
#  pragma warning(disable: 4703)
#  pragma warning(disable: 5045)
#endif

#define DR_MP3_IMPLEMENTATION
#include "dr_libs/dr_mp3.h"

#if defined(_MSC_VER)
#  pragma warning(pop)
#endif

#include <atomic>
#include <sstream>
#include <thread>

namespace apx {

namespace {
// 文件 < 此阈值同步扫描总帧数(开销低);否则丢到后台,
// totalFrames() 在扫描完成前返回 0
constexpr long kSyncScanThresholdBytes = 32 * 1024 * 1024;   // 32 MiB
} // namespace

struct Mp3Decoder::Impl {
    drmp3                mp3{};
    bool                 opened = false;
    bool                 want_float = false;       // open() 时锁定;影响 fmt 与 read 路径
    AudioFormat          fmt{};
    std::atomic<std::int64_t> total_frames{0};   // 0 表示"未知/正在扫描"
    std::int64_t         cur_frame    = 0;
    std::uint32_t        frame_bytes  = 0;
    std::wstring         last_error;

    // VBR 大文件用独立 drmp3 实例后台扫描;扫描线程不与 play 路径共享 d_->mp3
    std::thread          scan_thread;
    std::atomic<bool>    scan_running{false};

    void stop_scan() noexcept {
        scan_running.store(false, std::memory_order_release);
        if (scan_thread.joinable()) scan_thread.join();
    }
};

Mp3Decoder::Mp3Decoder()  : d_(std::make_unique<Impl>()) {}
Mp3Decoder::~Mp3Decoder() { close(); }

bool         Mp3Decoder::isOpen()       const { return d_->opened; }
AudioFormat  Mp3Decoder::format()       const { return d_->fmt; }
std::int64_t Mp3Decoder::totalFrames()  const { return d_->total_frames.load(std::memory_order_acquire); }
std::int64_t Mp3Decoder::currentFrame() const { return d_->cur_frame; }
std::wstring Mp3Decoder::lastError()    const { return d_->last_error; }

void Mp3Decoder::setOutputFloat32(bool on)
{
    // 已 open 后不允许切换(会改变 frame_bytes 与下游格式协商)
    if (d_->opened) return;
    d_->want_float = on;
}
bool Mp3Decoder::outputFloat32() const { return d_->want_float; }

void Mp3Decoder::close()
{
    d_->stop_scan();
    if (d_->opened) { drmp3_uninit(&d_->mp3); d_->opened = false; }
    d_->fmt = {};
    d_->total_frames.store(0, std::memory_order_release);
    d_->cur_frame    = 0;
    d_->frame_bytes  = 0;
}

bool Mp3Decoder::open(const std::wstring& path)
{
    if (d_->opened) close();

    if (!drmp3_init_file_w(&d_->mp3, path.c_str(), nullptr)) {
        d_->last_error = L"drmp3_init_file_w failed: " + path;
        return false;
    }
    d_->opened = true;

    AudioFormat fmt;
    fmt.sample_rate     = d_->mp3.sampleRate;
    fmt.channels        = static_cast<std::uint16_t>(d_->mp3.channels);
    if (d_->want_float) {
        fmt.bits_per_sample = 32;
        fmt.valid_bits      = 32;
        fmt.sample_type     = SampleType::Float32;
    } else {
        fmt.bits_per_sample = 16;
        fmt.valid_bits      = 16;
        fmt.sample_type     = SampleType::Int16;
    }
    fmt.channel_mask    = default_channel_mask(fmt.channels);

    if (!fmt.valid()) {
        d_->last_error = L"MP3 produced invalid AudioFormat";
        drmp3_uninit(&d_->mp3);
        d_->opened = false;
        return false;
    }

    d_->fmt          = fmt;
    d_->frame_bytes  = fmt.frame_bytes();
    d_->cur_frame    = 0;

    // 总帧数策略:小文件同步扫描;大文件后台扫描,避免 open() 阻塞 UI
    // 用 _ftelli64 而非 ftell，32-bit 进程下 long 是 32-bit，>2GB 文件会截断到 -1。
    std::int64_t file_size = 0;
    {
        FILE* fp = nullptr;
        if (_wfopen_s(&fp, path.c_str(), L"rb") == 0 && fp) {
            _fseeki64(fp, 0, SEEK_END);
            file_size = _ftelli64(fp);
            std::fclose(fp);
        }
    }
    if (file_size > 0 && file_size < kSyncScanThresholdBytes) {
        d_->total_frames.store(
            static_cast<std::int64_t>(drmp3_get_pcm_frame_count(&d_->mp3)),
            std::memory_order_release);
    } else {
        // 后台开第二个 drmp3 实例(独立 FILE*),避免与播放共享 d_->mp3
        d_->scan_running.store(true, std::memory_order_release);
        const std::wstring scan_path = path;
        Impl* impl = d_.get();
        d_->scan_thread = std::thread([impl, scan_path]() {
            drmp3 mp3_count{};
            if (!drmp3_init_file_w(&mp3_count, scan_path.c_str(), nullptr)) return;
            // drmp3_get_pcm_frame_count 是阻塞 O(N) 扫描;过程中没有 abort hook,
            // 只能等它跑完(close() 里 join)
            const drmp3_uint64 n = drmp3_get_pcm_frame_count(&mp3_count);
            drmp3_uninit(&mp3_count);
            if (impl->scan_running.load(std::memory_order_acquire)) {
                impl->total_frames.store(static_cast<std::int64_t>(n),
                                         std::memory_order_release);
            }
            impl->scan_running.store(false, std::memory_order_release);
        });
    }
    return true;
}

bool Mp3Decoder::seek(std::int64_t frame)
{
    if (!d_->opened) { d_->last_error = L"not open"; return false; }
    if (frame < 0) frame = 0;
    const std::int64_t total = d_->total_frames.load(std::memory_order_acquire);
    if (total > 0 && frame > total) frame = total;

    if (!drmp3_seek_to_pcm_frame(&d_->mp3, static_cast<drmp3_uint64>(frame))) {
        d_->last_error = L"drmp3_seek_to_pcm_frame failed";
        return false;
    }
    d_->cur_frame = frame;
    return true;
}

std::size_t Mp3Decoder::read(std::uint8_t* dst, std::size_t bytes)
{
    if (!d_->opened || !dst || bytes == 0 || d_->frame_bytes == 0) return 0;

    bytes -= (bytes % d_->frame_bytes);
    if (bytes == 0) return 0;

    const std::size_t frames_wanted =
        static_cast<std::size_t>(bytes / d_->frame_bytes);

    drmp3_uint64 frames_read = 0;
    if (d_->want_float) {
        frames_read = drmp3_read_pcm_frames_f32(
            &d_->mp3,
            static_cast<drmp3_uint64>(frames_wanted),
            reinterpret_cast<float*>(dst));
    } else {
        frames_read = drmp3_read_pcm_frames_s16(
            &d_->mp3,
            static_cast<drmp3_uint64>(frames_wanted),
            reinterpret_cast<drmp3_int16*>(dst));
    }

    d_->cur_frame += static_cast<std::int64_t>(frames_read);
    return static_cast<std::size_t>(frames_read) * d_->frame_bytes;
}

} // namespace apx

#else  // !APX_HAVE_DR_MP3 — dr_mp3.h 不存在时,提供桩实现保证链接

namespace apx {

struct Mp3Decoder::Impl {};

Mp3Decoder::Mp3Decoder()  : d_(std::make_unique<Impl>()) {}
Mp3Decoder::~Mp3Decoder() = default;

bool         Mp3Decoder::open(const std::wstring&) { return false; }
void         Mp3Decoder::close()                 {}
bool         Mp3Decoder::isOpen()       const     { return false; }
AudioFormat  Mp3Decoder::format()       const     { return {}; }
std::int64_t Mp3Decoder::totalFrames()  const     { return 0; }
std::int64_t Mp3Decoder::currentFrame() const     { return 0; }
bool         Mp3Decoder::seek(std::int64_t)       { return false; }
std::size_t  Mp3Decoder::read(std::uint8_t*, std::size_t) { return 0; }
std::wstring Mp3Decoder::lastError()    const     { return L"MP3 support not compiled in"; }
void         Mp3Decoder::setOutputFloat32(bool)   {}
bool         Mp3Decoder::outputFloat32()  const   { return false; }

} // namespace apx

#endif // APX_HAVE_DR_MP3
