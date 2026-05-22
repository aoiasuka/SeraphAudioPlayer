// =============================================================================
//  platform/mmdevice/DeviceEnumerator.cpp
//
//  封装 IMMDeviceEnumerator + IMMNotificationClient。
//  设计要点:
//    - DeviceEnumerator 持有一个 IMMDeviceEnumerator;listener 注册时创建
//      一个内部 NotificationClient(实现 IMMNotificationClient)挂上去
//    - WASAPI 通知线程会回调到 NotificationClient,后者转调用户 listener
//    - 用户 listener 必须自己保证内部 thread-safe(回调发生在 MMDevice 内部线程)
// =============================================================================
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
#include <mmdeviceapi.h>
#include <endpointvolume.h>
#include <functiondiscoverykeys_devpkey.h>

#include "DeviceEnumerator.h"

#include <atomic>
#include <mutex>
#include <sstream>

#pragma comment(lib, "ole32.lib")

namespace apx {

namespace {

DeviceState map_state(DWORD s) noexcept
{
    switch (s) {
    case DEVICE_STATE_ACTIVE:     return DeviceState::Active;
    case DEVICE_STATE_DISABLED:   return DeviceState::Disabled;
    case DEVICE_STATE_NOTPRESENT: return DeviceState::NotPresent;
    case DEVICE_STATE_UNPLUGGED:  return DeviceState::Unplugged;
    default:                      return DeviceState::Active;
    }
}

std::wstring prop_wstr(IPropertyStore* props, REFPROPERTYKEY key)
{
    std::wstring out;
    if (!props) return out;
    PROPVARIANT pv; PropVariantInit(&pv);
    if (SUCCEEDED(props->GetValue(key, &pv)) && pv.vt == VT_LPWSTR && pv.pwszVal) {
        out = pv.pwszVal;
    }
    PropVariantClear(&pv);
    return out;
}

bool fill_device_info(IMMDevice* dev, DeviceInfo& info)
{
    LPWSTR id = nullptr;
    if (FAILED(dev->GetId(&id)) || !id) return false;
    info.id = id;
    CoTaskMemFree(id);

    DWORD state = DEVICE_STATE_ACTIVE;
    if (SUCCEEDED(dev->GetState(&state))) info.state = map_state(state);

    IPropertyStore* props = nullptr;
    if (SUCCEEDED(dev->OpenPropertyStore(STGM_READ, &props)) && props) {
        info.friendly_name = prop_wstr(props, PKEY_Device_FriendlyName);
        info.desc          = prop_wstr(props, PKEY_DeviceInterface_FriendlyName);
        props->Release();
    }
    return true;
}

inline bool iequals_contains(const std::wstring& hay, const std::wstring& needle)
{
    if (needle.empty()) return true;
    auto lower = [](std::wstring s) {
        for (auto& c : s) c = static_cast<wchar_t>(::towlower(c));
        return s;
    };
    return lower(hay).find(lower(needle)) != std::wstring::npos;
}

} // namespace

// =============================================================================
// IMMNotificationClient 实现(内部类)
// =============================================================================

struct DeviceEnumerator::Impl {
    IMMDeviceEnumerator*   enumerator = nullptr;
    class NotifClient*     notif      = nullptr;     // 注册成功后非空
    bool                   com_init_self = false;

    std::mutex             listener_mutex;
    IDeviceChangeListener* listener = nullptr;

    mutable std::mutex     err_mutex;
    std::wstring           last_error;

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

    // 派发到用户 listener(回调线程上下文)
    void dispatch_added (const std::wstring& id) {
        std::lock_guard<std::mutex> lk(listener_mutex);
        if (listener) listener->onDeviceAdded(id);
    }
    void dispatch_removed (const std::wstring& id) {
        std::lock_guard<std::mutex> lk(listener_mutex);
        if (listener) listener->onDeviceRemoved(id);
    }
    void dispatch_state_changed (const std::wstring& id, DeviceState s) {
        std::lock_guard<std::mutex> lk(listener_mutex);
        if (listener) listener->onDeviceStateChanged(id, s);
    }
    void dispatch_default_changed (const std::wstring& id, DefaultRole r) {
        std::lock_guard<std::mutex> lk(listener_mutex);
        if (listener) listener->onDefaultDeviceChanged(id, r);
    }
};

class NotifClient final : public IMMNotificationClient {
public:
    explicit NotifClient(DeviceEnumerator::Impl* owner) : owner_(owner) {}

    // IUnknown
    ULONG STDMETHODCALLTYPE AddRef() override {
        return static_cast<ULONG>(InterlockedIncrement(&ref_));
    }
    ULONG STDMETHODCALLTYPE Release() override {
        const LONG r = InterlockedDecrement(&ref_);
        if (r == 0) delete this;
        return static_cast<ULONG>(r);
    }
    HRESULT STDMETHODCALLTYPE QueryInterface(REFIID iid, void** out) override {
        if (!out) return E_POINTER;
        if (iid == __uuidof(IUnknown) || iid == __uuidof(IMMNotificationClient)) {
            *out = static_cast<IMMNotificationClient*>(this);
            AddRef();
            return S_OK;
        }
        *out = nullptr;
        return E_NOINTERFACE;
    }

    // IMMNotificationClient
    HRESULT STDMETHODCALLTYPE OnDeviceStateChanged(LPCWSTR id, DWORD new_state) override {
        if (owner_ && id) owner_->dispatch_state_changed(id, map_state(new_state));
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE OnDeviceAdded(LPCWSTR id) override {
        if (owner_ && id) owner_->dispatch_added(id);
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE OnDeviceRemoved(LPCWSTR id) override {
        if (owner_ && id) owner_->dispatch_removed(id);
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE OnDefaultDeviceChanged(EDataFlow flow, ERole role, LPCWSTR id) override {
        if (!owner_ || !id || flow != eRender) return S_OK;
        DefaultRole r = DefaultRole::None;
        switch (role) {
        case eConsole:        r = DefaultRole::Console;        break;
        case eMultimedia:     r = DefaultRole::Multimedia;     break;
        case eCommunications: r = DefaultRole::Communications; break;
        default: break;
        }
        owner_->dispatch_default_changed(id, r);
        return S_OK;
    }
    HRESULT STDMETHODCALLTYPE OnPropertyValueChanged(LPCWSTR, const PROPERTYKEY) override {
        return S_OK;
    }

    // 调用者持有最后引用时通知 owner 被解除
    void detach() { owner_ = nullptr; }

private:
    LONG                     ref_   = 1;
    DeviceEnumerator::Impl*  owner_ = nullptr;
};

// =============================================================================

DeviceEnumerator::DeviceEnumerator()
    : d_(std::make_unique<Impl>())
{
    init();
}

DeviceEnumerator::~DeviceEnumerator()
{
    shutdown();
}

bool DeviceEnumerator::init()
{
    if (d_->enumerator) return true;

    HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    if (hr == RPC_E_CHANGED_MODE) {
        d_->com_init_self = false;
    } else if (SUCCEEDED(hr)) {
        d_->com_init_self = (hr == S_OK);
    } else {
        d_->set_error(L"CoInitializeEx", hr);
        return false;
    }

    hr = CoCreateInstance(__uuidof(MMDeviceEnumerator), nullptr, CLSCTX_ALL,
                          __uuidof(IMMDeviceEnumerator),
                          reinterpret_cast<void**>(&d_->enumerator));
    if (FAILED(hr)) {
        d_->set_error(L"CoCreateInstance(MMDeviceEnumerator)", hr);
        if (d_->com_init_self) { CoUninitialize(); d_->com_init_self = false; }
        return false;
    }
    return true;
}

void DeviceEnumerator::shutdown()
{
    unregisterListener();
    if (d_->enumerator) { d_->enumerator->Release(); d_->enumerator = nullptr; }
    if (d_->com_init_self) { CoUninitialize(); d_->com_init_self = false; }
}

std::wstring DeviceEnumerator::lastError() const { return d_->get_error(); }

std::wstring DeviceEnumerator::defaultRenderId()
{
    if (!d_->enumerator) return L"";
    IMMDevice* dev = nullptr;
    if (FAILED(d_->enumerator->GetDefaultAudioEndpoint(eRender, eConsole, &dev)) || !dev) {
        return L"";
    }
    LPWSTR id = nullptr;
    std::wstring out;
    if (SUCCEEDED(dev->GetId(&id)) && id) { out = id; CoTaskMemFree(id); }
    dev->Release();
    return out;
}

std::vector<DeviceInfo> DeviceEnumerator::listRenderEndpoints(bool include_inactive)
{
    std::vector<DeviceInfo> result;
    if (!d_->enumerator) return result;

    const DWORD mask = include_inactive
        ? (DEVICE_STATE_ACTIVE | DEVICE_STATE_DISABLED | DEVICE_STATE_NOTPRESENT | DEVICE_STATE_UNPLUGGED)
        : DEVICE_STATE_ACTIVE;

    IMMDeviceCollection* coll = nullptr;
    if (FAILED(d_->enumerator->EnumAudioEndpoints(eRender, mask, &coll)) || !coll) {
        d_->set_error_msg(L"EnumAudioEndpoints failed");
        return result;
    }

    // 预取默认 id(三种 role)做标记
    auto def_id = [&](ERole role) -> std::wstring {
        IMMDevice* d = nullptr;
        if (SUCCEEDED(d_->enumerator->GetDefaultAudioEndpoint(eRender, role, &d)) && d) {
            LPWSTR id = nullptr;
            std::wstring s;
            if (SUCCEEDED(d->GetId(&id)) && id) { s = id; CoTaskMemFree(id); }
            d->Release();
            return s;
        }
        return L"";
    };
    const std::wstring def_console        = def_id(eConsole);
    const std::wstring def_multimedia     = def_id(eMultimedia);
    const std::wstring def_communications = def_id(eCommunications);

    UINT n = 0;
    coll->GetCount(&n);
    result.reserve(n);
    for (UINT i = 0; i < n; ++i) {
        IMMDevice* dev = nullptr;
        if (FAILED(coll->Item(i, &dev)) || !dev) continue;
        DeviceInfo info;
        if (fill_device_info(dev, info)) {
            DefaultRole r = DefaultRole::None;
            if (info.id == def_console)        r = r | DefaultRole::Console;
            if (info.id == def_multimedia)     r = r | DefaultRole::Multimedia;
            if (info.id == def_communications) r = r | DefaultRole::Communications;
            info.default_for = r;
            result.push_back(std::move(info));
        }
        dev->Release();
    }
    coll->Release();
    return result;
}

std::optional<DeviceInfo> DeviceEnumerator::findById(const std::wstring& id)
{
    if (!d_->enumerator || id.empty()) return std::nullopt;
    IMMDevice* dev = nullptr;
    if (FAILED(d_->enumerator->GetDevice(id.c_str(), &dev)) || !dev) return std::nullopt;
    DeviceInfo info;
    const bool ok = fill_device_info(dev, info);
    dev->Release();
    if (!ok) return std::nullopt;

    // 默认角色标记
    auto def_id = [&](ERole role) -> std::wstring {
        IMMDevice* d = nullptr;
        if (SUCCEEDED(d_->enumerator->GetDefaultAudioEndpoint(eRender, role, &d)) && d) {
            LPWSTR p = nullptr; std::wstring s;
            if (SUCCEEDED(d->GetId(&p)) && p) { s = p; CoTaskMemFree(p); }
            d->Release();
            return s;
        }
        return L"";
    };
    DefaultRole r = DefaultRole::None;
    if (info.id == def_id(eConsole))        r = r | DefaultRole::Console;
    if (info.id == def_id(eMultimedia))     r = r | DefaultRole::Multimedia;
    if (info.id == def_id(eCommunications)) r = r | DefaultRole::Communications;
    info.default_for = r;
    return info;
}

std::optional<DeviceInfo> DeviceEnumerator::findByNameSubstring(const std::wstring& sub)
{
    if (sub.empty()) return std::nullopt;
    auto list = listRenderEndpoints(true);
    // 第一遍:Active 优先
    for (const auto& d : list)
        if (d.state == DeviceState::Active && iequals_contains(d.friendly_name, sub))
            return d;
    for (const auto& d : list)
        if (iequals_contains(d.friendly_name, sub))
            return d;
    return std::nullopt;
}

bool DeviceEnumerator::registerListener(IDeviceChangeListener* listener)
{
    if (!d_->enumerator) { d_->set_error_msg(L"enumerator not initialized"); return false; }
    unregisterListener();
    {
        std::lock_guard<std::mutex> lk(d_->listener_mutex);
        d_->listener = listener;
    }
    d_->notif = new NotifClient(d_.get());
    HRESULT hr = d_->enumerator->RegisterEndpointNotificationCallback(d_->notif);
    if (FAILED(hr)) {
        d_->set_error(L"RegisterEndpointNotificationCallback", hr);
        d_->notif->detach();
        d_->notif->Release();
        d_->notif = nullptr;
        std::lock_guard<std::mutex> lk(d_->listener_mutex);
        d_->listener = nullptr;
        return false;
    }
    return true;
}

void DeviceEnumerator::unregisterListener()
{
    if (d_->notif && d_->enumerator) {
        d_->enumerator->UnregisterEndpointNotificationCallback(d_->notif);
        d_->notif->detach();
        d_->notif->Release();
        d_->notif = nullptr;
    }
    std::lock_guard<std::mutex> lk(d_->listener_mutex);
    d_->listener = nullptr;
}

} // namespace apx
