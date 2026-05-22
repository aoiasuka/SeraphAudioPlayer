// =============================================================================
//  ui/mainwindow/DeviceBridge.h
//
//  把 DeviceEnumerator 的热插拔回调(来自 MMDevice 内部线程)桥接到
//  Qt signal,主线程订阅以刷新设备下拉。
//
//  设计:
//    - 实现 apx::IDeviceChangeListener
//    - 任一事件都仅发出一个粗粒度 devicesChanged 信号,UI 拿到后重新
//      调 snapshotActive() 拉一次列表 —— 简单可靠,事件率极低,无性能负担
// =============================================================================
#pragma once

#include "platform/mmdevice/DeviceEnumerator.h"
#include "platform/mmdevice/DeviceTypes.h"

#include <QObject>
#include <QString>
#include <QVector>
#include <memory>

namespace apx::ui {

struct DeviceItem {
    QString id;
    QString friendly_name;
    bool    is_default_console = false;
    apx::DeviceState state = apx::DeviceState::Active;
};

class DeviceBridge : public QObject, public apx::IDeviceChangeListener {
    Q_OBJECT
public:
    explicit DeviceBridge(QObject* parent = nullptr);
    ~DeviceBridge() override;

    // 启动 / 停止热插拔监听
    void start();
    void stop();

    // 主线程同步拉取一次设备快照(仅 Active)
    QVector<DeviceItem> snapshotActive();

signals:
    // 设备列表可能发生变化 —— UI 应当刷新
    void devicesChanged();

private:
    // IDeviceChangeListener 回调(由 MMDevice 内部线程调入)
    void onDeviceAdded   (const std::wstring& id) override;
    void onDeviceRemoved (const std::wstring& id) override;
    void onDeviceStateChanged(const std::wstring& id, apx::DeviceState s) override;
    void onDefaultDeviceChanged(const std::wstring& id, apx::DefaultRole r) override;

    DeviceEnumerator enumerator_;
    bool             started_ = false;
};

} // namespace apx::ui
