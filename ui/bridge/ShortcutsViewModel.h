// =============================================================================
//  ui/bridge/ShortcutsViewModel.h
//
//  键盘快捷键中心。两个目标:
//    1) 把 main.qml 里硬编码的 Shortcut 序列改成可在设置里改键
//    2) 让 ShortcutsDialog 能用 KeySequence editor 录制/恢复默认
//
//  设计:
//    - 维护两份表:
//        m_defaults: id → (label, group, defaultSeq)
//        m_overrides: id → newSeq (空表示用默认)
//    - 对 QML 暴露 keymap() → QVariantMap:id → 当前生效 sequence
//      QML 端: `sequence: shortcutsVM.keymap["play_pause"]`
//      改键后 keymapChanged 信号触发 → Shortcut 重新绑定
//    - 对 QML 暴露 groups() → QVariantList:UI 渲染分组列表
//    - 持久化:QSettings group="shortcuts", key=id, value=seq
//
//  线程模型:UI 线程独占。
// =============================================================================
#pragma once

#include <QObject>
#include <QString>
#include <QVariantMap>
#include <QVariantList>
#include <QMap>
#include <QList>

namespace apx::ui {

class ShortcutsViewModel : public QObject {
    Q_OBJECT
    Q_PROPERTY(QVariantMap  keymap READ keymap NOTIFY keymapChanged)
    Q_PROPERTY(QVariantList groups READ groups NOTIFY keymapChanged)

public:
    explicit ShortcutsViewModel(QObject* parent = nullptr);

    QVariantMap  keymap() const;
    QVariantList groups() const;

    // 取某 action 的当前 sequence;不存在返回空 QString
    Q_INVOKABLE QString keyFor(const QString& id) const;
    // 取某 action 的默认 sequence
    Q_INVOKABLE QString defaultKeyFor(const QString& id) const;
    // 设置 override;空字符串等价于 reset
    Q_INVOKABLE void    setKey(const QString& id, const QString& seq);
    // 恢复默认
    Q_INVOKABLE void    resetKey(const QString& id);
    // 全部恢复默认
    Q_INVOKABLE void    resetAll();

signals:
    void keymapChanged();

private:
    struct Def {
        QString id;
        QString label;
        QString group;
        QString def;       // 默认 sequence
    };

    void registerDefault(const QString& id, const QString& label,
                         const QString& group, const QString& def);
    void load();
    void save() const;

    QList<Def>            m_defaults;     // 注册顺序 (UI 渲染顺序)
    QMap<QString, QString> m_overrides;   // 仅存 != default 的
    bool m_loading = false;
};

} // namespace apx::ui
