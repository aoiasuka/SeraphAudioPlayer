// =============================================================================
//  examples/wasapi_devices_demo
//
//  验证 DeviceEnumerator:
//    1) 启动时列出所有渲染端点
//    2) 注册监听器,等待 30 秒,实时打印热插拔事件
//       (插拔耳机/USB DAC 时控制台会有 onDeviceStateChanged / onDefaultDeviceChanged)
// =============================================================================

#include "platform/mmdevice/DeviceEnumerator.h"

#include <atomic>
#include <chrono>
#include <csignal>
#include <cstdio>
#include <mutex>
#include <thread>

namespace {
std::atomic<bool> g_interrupted{false};
void on_sigint(int) { g_interrupted.store(true, std::memory_order_release); }

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
const wchar_t* role_name(apx::DefaultRole r)
{
    if (apx::has_role(r, apx::DefaultRole::Console))        return L"Console";
    if (apx::has_role(r, apx::DefaultRole::Multimedia))     return L"Multimedia";
    if (apx::has_role(r, apx::DefaultRole::Communications)) return L"Communications";
    return L"None";
}
} // namespace

// 监听器实现:把回调内容序列化打印,内部自带 mutex 防止与主线程交错
class PrintListener final : public apx::IDeviceChangeListener {
public:
    void onDeviceAdded(const std::wstring& id) override {
        std::lock_guard<std::mutex> lk(m_);
        std::wprintf(L"[EVT] DeviceAdded         id=%s\n", id.c_str());
    }
    void onDeviceRemoved(const std::wstring& id) override {
        std::lock_guard<std::mutex> lk(m_);
        std::wprintf(L"[EVT] DeviceRemoved       id=%s\n", id.c_str());
    }
    void onDeviceStateChanged(const std::wstring& id, apx::DeviceState s) override {
        std::lock_guard<std::mutex> lk(m_);
        std::wprintf(L"[EVT] StateChanged → %-10s id=%s\n", state_name(s), id.c_str());
    }
    void onDefaultDeviceChanged(const std::wstring& id, apx::DefaultRole r) override {
        std::lock_guard<std::mutex> lk(m_);
        std::wprintf(L"[EVT] DefaultChanged(%-14s) id=%s\n", role_name(r), id.c_str());
    }
private:
    std::mutex m_;
};

int wmain()
{
    using namespace apx;
    std::signal(SIGINT, on_sigint);

    DeviceEnumerator de;
    std::wprintf(L"=== 当前渲染端点 ===\n");
    const auto list = de.listRenderEndpoints(true);
    int idx = 0;
    for (const auto& d : list) {
        std::wprintf(L"  [%02d] %-10s %s%s\n        id   = %s\n",
                     idx++,
                     state_name(d.state),
                     d.friendly_name.c_str(),
                     d.is_default_console() ? L"   (DEFAULT)" : L"",
                     d.id.c_str());
        if (!d.desc.empty())
            std::wprintf(L"        if   = %s\n", d.desc.c_str());
    }
    std::wprintf(L"\n默认 id: %s\n\n", de.defaultRenderId().c_str());

    PrintListener listener;
    if (!de.registerListener(&listener)) {
        std::fwprintf(stderr, L"[FAIL] registerListener: %s\n", de.lastError().c_str());
        return 1;
    }
    std::wprintf(L"=== 监听 30 秒(可拔插耳机/USB DAC 触发事件,Ctrl+C 提前退出) ===\n");

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(30);
    while (!g_interrupted.load() && std::chrono::steady_clock::now() < deadline) {
        std::this_thread::sleep_for(std::chrono::milliseconds(100));
    }
    de.unregisterListener();

    std::wprintf(L"\n[ OK ] 完成\n");
    return 0;
}
