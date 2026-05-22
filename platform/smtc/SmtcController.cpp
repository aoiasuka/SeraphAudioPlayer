// =============================================================================
//  platform/smtc/SmtcController.cpp
// =============================================================================
#include "SmtcController.h"

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

// C++/WinRT
#include <winrt/base.h>
#include <winrt/Windows.Foundation.h>
#include <winrt/Windows.Media.h>
#include <winrt/Windows.Storage.Streams.h>

#include <SystemMediaTransportControlsInterop.h>

namespace apx {

namespace WM  = winrt::Windows::Media;
namespace WSS = winrt::Windows::Storage::Streams;
namespace WF  = winrt::Windows::Foundation;

struct SmtcController::Impl {
    WM::SystemMediaTransportControls smtc{nullptr};
    WM::SystemMediaTransportControlsTimelineProperties timeline{};
    winrt::event_token buttonToken{};
    ButtonHandler cb;
    bool initialized = false;
    bool comInitialized = false;
};

SmtcController::SmtcController()
    : d_(std::make_unique<Impl>())
{
}

SmtcController::~SmtcController()
{
    shutdown();
}

bool SmtcController::initialize(void* hwndPtr)
{
    if (d_->initialized) return true;
    HWND hwnd = reinterpret_cast<HWND>(hwndPtr);
    if (!hwnd) return false;

    // 这里保证 COM(MTA 或 STA 由父线程决定) 已经初始化;
    // Qt 主线程通常已经初始化 STA。若未初始化,这里尝试初始化为 MTA。
    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    d_->comInitialized = SUCCEEDED(hr);   // RPC_E_CHANGED_MODE 也算"已就绪"
    if (hr == RPC_E_CHANGED_MODE) d_->comInitialized = false;

    try {
        // SystemMediaTransportControlsInterop 是必需的接口,通过
        // get_activation_factory 拿到。
        auto factory = winrt::get_activation_factory<
            WM::SystemMediaTransportControls,
            ISystemMediaTransportControlsInterop>();

        WM::SystemMediaTransportControls smtc{nullptr};
        winrt::check_hresult(factory->GetForWindow(
            hwnd,
            winrt::guid_of<WM::SystemMediaTransportControls>(),
            winrt::put_abi(smtc)));

        smtc.IsEnabled(true);
        smtc.IsPlayEnabled(true);
        smtc.IsPauseEnabled(true);
        smtc.IsStopEnabled(true);
        smtc.IsNextEnabled(true);
        smtc.IsPreviousEnabled(true);
        smtc.PlaybackStatus(WM::MediaPlaybackStatus::Closed);

        // 注册按钮事件
        d_->buttonToken = smtc.ButtonPressed(
            [this]
            (WM::SystemMediaTransportControls const&,
             WM::SystemMediaTransportControlsButtonPressedEventArgs const& args) {
                if (!d_ || !d_->cb) return;
                switch (args.Button()) {
                case WM::SystemMediaTransportControlsButton::Play:     d_->cb(SmtcButton::Play); break;
                case WM::SystemMediaTransportControlsButton::Pause:    d_->cb(SmtcButton::Pause); break;
                case WM::SystemMediaTransportControlsButton::Stop:     d_->cb(SmtcButton::Stop); break;
                case WM::SystemMediaTransportControlsButton::Next:     d_->cb(SmtcButton::Next); break;
                case WM::SystemMediaTransportControlsButton::Previous: d_->cb(SmtcButton::Previous); break;
                default: break;
                }
            });

        d_->smtc = std::move(smtc);
        d_->initialized = true;
        return true;
    } catch (winrt::hresult_error const&) {
        d_->initialized = false;
        return false;
    } catch (...) {
        d_->initialized = false;
        return false;
    }
}

void SmtcController::shutdown()
{
    if (d_ && d_->initialized && d_->smtc) {
        try {
            if (d_->buttonToken.value != 0) {
                d_->smtc.ButtonPressed(d_->buttonToken);
                d_->buttonToken = {};
            }
            d_->smtc.IsEnabled(false);
        } catch (...) {}
        d_->smtc = nullptr;
        d_->initialized = false;
    }
    if (d_ && d_->comInitialized) {
        CoUninitialize();
        d_->comInitialized = false;
    }
}

void SmtcController::setMetadata(const std::wstring& title,
                                 const std::wstring& artist,
                                 const std::wstring& album)
{
    if (!d_->initialized) return;
    try {
        auto updater = d_->smtc.DisplayUpdater();
        updater.Type(WM::MediaPlaybackType::Music);
        auto music = updater.MusicProperties();
        music.Title(winrt::hstring{title});
        music.Artist(winrt::hstring{artist});
        music.AlbumTitle(winrt::hstring{album});
        updater.Update();
    } catch (...) {}
}

void SmtcController::setStatus(SmtcStatus s)
{
    if (!d_->initialized) return;
    WM::MediaPlaybackStatus m;
    switch (s) {
    case SmtcStatus::Closed:  m = WM::MediaPlaybackStatus::Closed;  break;
    case SmtcStatus::Stopped: m = WM::MediaPlaybackStatus::Stopped; break;
    case SmtcStatus::Playing: m = WM::MediaPlaybackStatus::Playing; break;
    case SmtcStatus::Paused:  m = WM::MediaPlaybackStatus::Paused;  break;
    default:                  m = WM::MediaPlaybackStatus::Closed;  break;
    }
    try { d_->smtc.PlaybackStatus(m); } catch (...) {}
}

void SmtcController::setThumbnail(const std::uint8_t* data, std::size_t size)
{
    if (!d_->initialized) return;
    try {
        auto updater = d_->smtc.DisplayUpdater();
        if (!data || size == 0) {
            updater.Thumbnail(nullptr);
            updater.Update();
            return;
        }
        WSS::InMemoryRandomAccessStream stream;
        {
            WSS::DataWriter writer(stream);
            writer.WriteBytes(winrt::array_view<uint8_t const>(
                data, data + size));
            writer.StoreAsync().get();
            writer.DetachStream();
        }
        stream.Seek(0);
        auto refStream = WSS::RandomAccessStreamReference::CreateFromStream(stream);
        updater.Thumbnail(refStream);
        updater.Update();
    } catch (...) {}
}

void SmtcController::setTimeline(double position_sec, double duration_sec)
{
    if (!d_->initialized) return;
    try {
        d_->timeline.StartTime(WF::TimeSpan{0});
        d_->timeline.EndTime(WF::TimeSpan{
            static_cast<int64_t>(duration_sec * 10'000'000.0)});
        d_->timeline.MinSeekTime(WF::TimeSpan{0});
        d_->timeline.MaxSeekTime(WF::TimeSpan{
            static_cast<int64_t>(duration_sec * 10'000'000.0)});
        d_->timeline.Position(WF::TimeSpan{
            static_cast<int64_t>(position_sec * 10'000'000.0)});
        d_->smtc.UpdateTimelineProperties(d_->timeline);
    } catch (...) {}
}

void SmtcController::setOnButton(ButtonHandler cb)
{
    d_->cb = std::move(cb);
}

} // namespace apx
