// =============================================================================
//  ui/bridge/ShortcutsViewModel.cpp
// =============================================================================
#include "ShortcutsViewModel.h"

#include <QSettings>
#include <QVariantMap>

namespace apx::ui {

ShortcutsViewModel::ShortcutsViewModel(QObject* parent)
    : QObject(parent)
{
    // 注册顺序即 UI 渲染顺序;同一 group 内连续。
    registerDefault("play_pause",      "播放 / 暂停",                "播放控制", "Space");
    registerDefault("next",            "下一首",                     "播放控制", "Right");
    registerDefault("prev",            "上一首",                     "播放控制", "Left");
    registerDefault("vol_up",          "音量 +5",                    "播放控制", "Up");
    registerDefault("vol_down",        "音量 -5",                    "播放控制", "Down");
    registerDefault("mute",            "静音切换",                   "播放控制", "M");
    registerDefault("like_current",    "喜欢 / 取消喜欢当前曲目",    "播放控制", "Ctrl+L");
    registerDefault("cycle_repeat",    "切换循环模式",               "播放控制", "Ctrl+R");
    registerDefault("toggle_shuffle",  "切换随机",                   "播放控制", "Ctrl+S");

    registerDefault("toggle_queue",    "打开 / 收起队列抽屉",        "界面",     "Ctrl+Q");
    registerDefault("open_queue_page", "打开队列视图",               "界面",     "Ctrl+Shift+Q");
    registerDefault("toggle_eq",       "均衡器",                     "界面",     "Ctrl+E");
    registerDefault("escape",          "返回 / 关闭抽屉 / 退出全屏", "界面",     "Escape");
    registerDefault("toggle_fullscreen","切换全屏",                  "界面",     "F11");
    registerDefault("show_shortcuts",  "显示快捷键帮助",             "界面",     "F1");
    registerDefault("open_search",     "全局搜索",                   "界面",     "Ctrl+F");

    registerDefault("nav_home",        "首页",                       "导航",     "1");
    registerDefault("nav_library",     "音乐库",                     "导航",     "2");
    registerDefault("nav_playlist",    "歌单",                       "导航",     "3");
    registerDefault("nav_artist",      "歌手",                       "导航",     "4");
    registerDefault("nav_album",       "专辑",                       "导航",     "5");
    registerDefault("nav_history",     "最近播放",                   "导航",     "6");
    registerDefault("nav_liked",       "我喜欢的",                   "导航",     "7");
    registerDefault("nav_settings",    "设置",                       "导航",     "Ctrl+,");

    load();
}

void ShortcutsViewModel::registerDefault(const QString& id, const QString& label,
                                        const QString& group, const QString& def)
{
    m_defaults.push_back({id, label, group, def});
}

QString ShortcutsViewModel::defaultKeyFor(const QString& id) const
{
    for (const auto& d : m_defaults) if (d.id == id) return d.def;
    return {};
}

QString ShortcutsViewModel::keyFor(const QString& id) const
{
    auto it = m_overrides.find(id);
    if (it != m_overrides.end()) return it.value();
    return defaultKeyFor(id);
}

QVariantMap ShortcutsViewModel::keymap() const
{
    QVariantMap m;
    for (const auto& d : m_defaults) m.insert(d.id, keyFor(d.id));
    return m;
}

QVariantList ShortcutsViewModel::groups() const
{
    QVariantList groupList;
    // 按注册顺序累加,group 名第一次出现即建分组
    QMap<QString, int> groupIdx;
    for (const auto& d : m_defaults) {
        if (!groupIdx.contains(d.group)) {
            QVariantMap g;
            g["title"] = d.group;
            g["items"] = QVariantList{};
            groupIdx[d.group] = groupList.size();
            groupList.append(g);
        }
        QVariantMap item;
        item["id"]      = d.id;
        item["label"]   = d.label;
        item["key"]     = keyFor(d.id);
        item["default"] = d.def;
        item["custom"]  = m_overrides.contains(d.id);
        QVariantMap g = groupList[groupIdx[d.group]].toMap();
        QVariantList items = g["items"].toList();
        items.append(item);
        g["items"] = items;
        groupList[groupIdx[d.group]] = g;
    }
    return groupList;
}

void ShortcutsViewModel::setKey(const QString& id, const QString& seq)
{
    if (defaultKeyFor(id).isEmpty()) return;  // 未知 id
    if (seq.isEmpty() || seq == defaultKeyFor(id)) {
        m_overrides.remove(id);
    } else {
        m_overrides[id] = seq;
    }
    emit keymapChanged();
    if (!m_loading) save();
}

void ShortcutsViewModel::resetKey(const QString& id)
{
    if (!m_overrides.contains(id)) return;
    m_overrides.remove(id);
    emit keymapChanged();
    if (!m_loading) save();
}

void ShortcutsViewModel::resetAll()
{
    if (m_overrides.isEmpty()) return;
    m_overrides.clear();
    emit keymapChanged();
    if (!m_loading) save();
}

void ShortcutsViewModel::load()
{
    m_loading = true;
    QSettings s;
    s.beginGroup("shortcuts");
    for (const auto& d : m_defaults) {
        const QString v = s.value(d.id).toString();
        if (!v.isEmpty() && v != d.def) m_overrides[d.id] = v;
    }
    s.endGroup();
    m_loading = false;
    emit keymapChanged();
}

void ShortcutsViewModel::save() const
{
    QSettings s;
    s.beginGroup("shortcuts");
    s.remove("");  // 清掉旧 group 内容,只写当前 overrides
    for (auto it = m_overrides.constBegin(); it != m_overrides.constEnd(); ++it) {
        s.setValue(it.key(), it.value());
    }
    s.endGroup();
}

} // namespace apx::ui
