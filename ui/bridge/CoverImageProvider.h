// =============================================================================
//  ui/bridge/CoverImageProvider.h
//
//  QQuickImageProvider 子类,把音频文件中的内嵌封面提供给 QML。
//
//  用法:
//      QML 端写 `Image { source: "image://covers/" + encodeURIComponent(path) }`
//      即可从对应文件读取并解码 PICTURE block。
// =============================================================================
#pragma once

#include <QQuickImageProvider>
#include <QImage>
#include <QString>
#include <QMutex>
#include <QHash>
#include <QList>

namespace apx::ui {

class CoverImageProvider : public QQuickImageProvider {
public:
    CoverImageProvider();

    // id 即 URL 中 image://covers/<id> 的 <id> 部分;我们约定 id 就是文件本地路径
    // (已 URL 解码)。
    QImage requestImage(const QString& id, QSize* size, const QSize& requestedSize) override;

    // 主动让 QML 重新加载:可调用此方法触发缓存失效。
    // (这里仅做内部缓存使,不影响 QQuickImageProvider 的缓存机制。)
    void clearCache();

private:
    QMutex mtx_;
    QHash<QString, QImage> cache_;
    // LRU 顺序：尾部最近使用，头部最早。需要驱逐时从头部丢。
    QList<QString>         lru_;
    static constexpr int kMaxCache = 64;   // 适度增大，缓解大歌单频繁切换的反复读盘
};

} // namespace apx::ui
