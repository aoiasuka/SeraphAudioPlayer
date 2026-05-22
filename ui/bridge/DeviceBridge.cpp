// =============================================================================
//  ui/mainwindow/DeviceBridge.cpp
// =============================================================================
#include "DeviceBridge.h"

namespace apx::ui {

DeviceBridge::DeviceBridge(QObject* parent) : QObject(parent) {}

DeviceBridge::~DeviceBridge() { stop(); }

void DeviceBridge::start()
{
    if (started_) return;
    if (enumerator_.registerListener(this)) started_ = true;
}

void DeviceBridge::stop()
{
    if (!started_) return;
    enumerator_.unregisterListener();
    started_ = false;
}

QVector<DeviceItem> DeviceBridge::snapshotActive()
{
    QVector<DeviceItem> out;
    auto list = enumerator_.listRenderEndpoints(false);
    out.reserve(static_cast<int>(list.size()));
    for (const auto& d : list) {
        DeviceItem item;
        item.id                  = QString::fromStdWString(d.id);
        item.friendly_name       = QString::fromStdWString(d.friendly_name);
        item.is_default_console  = d.is_default_console();
        item.state               = d.state;
        out.append(std::move(item));
    }
    return out;
}

// ---- 内部线程回调:统一收敛为单个 devicesChanged 信号 ----

void DeviceBridge::onDeviceAdded   (const std::wstring&)                  { emit devicesChanged(); }
void DeviceBridge::onDeviceRemoved (const std::wstring&)                  { emit devicesChanged(); }
void DeviceBridge::onDeviceStateChanged(const std::wstring&, apx::DeviceState) { emit devicesChanged(); }
void DeviceBridge::onDefaultDeviceChanged(const std::wstring&, apx::DefaultRole) { emit devicesChanged(); }

} // namespace apx::ui
