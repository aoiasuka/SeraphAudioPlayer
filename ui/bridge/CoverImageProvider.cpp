// =============================================================================
//  ui/bridge/CoverImageProvider.cpp
// =============================================================================
#include "CoverImageProvider.h"
#include "core/metadata/MetadataReader.h"

#include <QMutexLocker>
#include <QByteArray>
#include <QUrl>

namespace apx::ui {

CoverImageProvider::CoverImageProvider()
    : QQuickImageProvider(QQuickImageProvider::Image)
{
}

QImage CoverImageProvider::requestImage(const QString& id, QSize* size, const QSize& requestedSize)
{
    QString path = QUrl::fromPercentEncoding(id.toUtf8());

    // 命中缓存
    {
        QMutexLocker lock(&mtx_);
        auto it = cache_.constFind(path);
        if (it != cache_.constEnd()) {
            const QImage& img = it.value();
            if (size) *size = img.size();
            if (requestedSize.isValid() && (requestedSize.width() > 0 || requestedSize.height() > 0)) {
                return img.scaled(requestedSize.boundedTo(img.size()),
                                  Qt::KeepAspectRatio, Qt::SmoothTransformation);
            }
            return img;
        }
    }

    // 从文件读取 PICTURE
    QImage out;
    auto cov = apx::MetadataReader::readCover(path.toStdWString());
    if (cov && !cov->data.empty()) {
        QImage decoded;
        decoded.loadFromData(reinterpret_cast<const uchar*>(cov->data.data()),
                             static_cast<int>(cov->data.size()));
        if (!decoded.isNull()) out = decoded;
    }

    // 失败时返回空 QImage (QML Image 会显示空白,组件可用 status 处理)
    {
        QMutexLocker lock(&mtx_);
        if (cache_.size() >= kMaxCache) {
            // 简单 LRU:随便丢一个最早进入的
            cache_.erase(cache_.begin());
        }
        cache_.insert(path, out);
    }

    if (size) *size = out.size();
    if (!out.isNull() && requestedSize.isValid() &&
        (requestedSize.width() > 0 || requestedSize.height() > 0)) {
        return out.scaled(requestedSize.boundedTo(out.size()),
                          Qt::KeepAspectRatio, Qt::SmoothTransformation);
    }
    return out;
}

void CoverImageProvider::clearCache()
{
    QMutexLocker lock(&mtx_);
    cache_.clear();
}

} // namespace apx::ui
