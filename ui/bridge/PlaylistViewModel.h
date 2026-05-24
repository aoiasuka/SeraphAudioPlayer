// =============================================================================
//  ui/bridge/PlaylistViewModel.h
//
//  QAbstractListModel 子类。把 PlayerViewModel 的播放队列以 ListModel 形式
//  暴露给 QML,替代原来的 QVariantList 全量重发。
//
//  使用方式:
//    QML:
//      ListView { model: playerVM.playlistModel
//                 delegate: Row { Text { text: title } ... } }
//    C++:
//      m_playlistModel->setItems(itemsFromPaths(m_queue, m_currentIndex));
//      // 内部走 beginResetModel/endResetModel,QML 端 ListView 会刷新
//
//  Role 与 itemsFromPaths 的 QVariantMap 字段同名,以便 QML 端写法相同:
//    title / artist / album / suffix / dir / fileName / duration
//    isCurrent / liked / hasCover / coverUrl / path
//
//  线程模型:UI 线程独占。
// =============================================================================
#pragma once

#include <QAbstractListModel>
#include <QVariantList>
#include <QVariantMap>
#include <QHash>
#include <QByteArray>
#include <QString>

namespace apx::ui {

class PlaylistViewModel : public QAbstractListModel {
    Q_OBJECT
    Q_PROPERTY(int count READ rowCount NOTIFY countChanged)
    Q_PROPERTY(int currentIndex READ currentIndex NOTIFY currentIndexChanged)

public:
    enum Roles {
        PathRole = Qt::UserRole + 1,
        TitleRole,
        ArtistRole,
        AlbumRole,
        SuffixRole,
        DirRole,
        FileNameRole,
        DurationRole,
        IsCurrentRole,
        LikedRole,
        HasCoverRole,
        CoverUrlRole,
    };

    explicit PlaylistViewModel(QObject* parent = nullptr);

    int rowCount(const QModelIndex& parent = QModelIndex()) const override;
    QVariant data(const QModelIndex& index, int role) const override;
    QHash<int, QByteArray> roleNames() const override;

    int currentIndex() const { return m_currentIndex; }

    // PlayerViewModel 在队列/当前项变化时调用。一次性 reset。
    void setItems(const QVariantList& items, int currentIndex);

    // 取一行 (QML 端可用 listView.itemAtIndex 直接拿 delegate,
    //         但若需要在 JS 端拿到 raw map,这里也提供)
    Q_INVOKABLE QVariantMap get(int row) const;

signals:
    void countChanged();
    void currentIndexChanged();

private:
    QVariantList m_items;        // 每项是 itemsFromPaths 产出的 QVariantMap
    int m_currentIndex = -1;
};

} // namespace apx::ui
