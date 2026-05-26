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

    // 空路径直接返回空，避免无意义的 I/O 与日志噪声
    if (path.isEmpty()) {
        if (size) *size = QSize();
        return QImage();
    }

    // 命中缓存
    {
        QMutexLocker lock(&mtx_);
        auto it = cache_.constFind(path);
        if (it != cache_.constEnd()) {
            // LRU：把命中项挪到队尾
            lru_.removeOne(path);
            lru_.append(path);
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
        // 真正的 LRU：超容时从 lru_ 头部驱逐最久未访问项
        while (cache_.size() >= kMaxCache && !lru_.isEmpty()) {
            const QString oldest = lru_.takeFirst();
            cache_.remove(oldest);
        }
        cache_.insert(path, out);
        lru_.append(path);
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
    lru_.clear();
}

} // namespace apx::ui
