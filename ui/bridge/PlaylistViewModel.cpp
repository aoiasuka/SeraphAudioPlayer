// =============================================================================
//  ui/bridge/PlaylistViewModel.cpp
// =============================================================================
#include "PlaylistViewModel.h"

namespace apx::ui {

PlaylistViewModel::PlaylistViewModel(QObject* parent)
    : QAbstractListModel(parent) {}

int PlaylistViewModel::rowCount(const QModelIndex& parent) const
{
    if (parent.isValid()) return 0;
    return static_cast<int>(m_items.size());
}

QVariant PlaylistViewModel::data(const QModelIndex& index, int role) const
{
    if (!index.isValid()) return {};
    const int row = index.row();
    if (row < 0 || row >= m_items.size()) return {};
    const QVariantMap m = m_items.at(row).toMap();
    switch (role) {
    case PathRole:      return m.value("path");
    case TitleRole:     return m.value("title");
    case ArtistRole:    return m.value("artist");
    case AlbumRole:     return m.value("album");
    case SuffixRole:    return m.value("suffix");
    case DirRole:       return m.value("dir");
    case FileNameRole:  return m.value("fileName");
    case DurationRole:  return m.value("duration");
    case IsCurrentRole: return m.value("isCurrent");
    case LikedRole:     return m.value("liked");
    case HasCoverRole:  return m.value("hasCover");
    case CoverUrlRole:  return m.value("coverUrl");
    default: return {};
    }
}

QHash<int, QByteArray> PlaylistViewModel::roleNames() const
{
    return {
        {PathRole,      "path"},
        {TitleRole,     "title"},
        {ArtistRole,    "artist"},
        {AlbumRole,     "album"},
        {SuffixRole,    "suffix"},
        {DirRole,       "dir"},
        {FileNameRole,  "fileName"},
        {DurationRole,  "duration"},
        {IsCurrentRole, "isCurrent"},
        {LikedRole,     "liked"},
        {HasCoverRole,  "hasCover"},
        {CoverUrlRole,  "coverUrl"},
    };
}

void PlaylistViewModel::setItems(const QVariantList& items, int currentIndex)
{
    const int oldCount = static_cast<int>(m_items.size());
    const int oldCur   = m_currentIndex;
    beginResetModel();
    m_items = items;
    m_currentIndex = currentIndex;
    endResetModel();
    if (oldCount != static_cast<int>(m_items.size())) emit countChanged();
    if (oldCur != m_currentIndex) emit currentIndexChanged();
}

QVariantMap PlaylistViewModel::get(int row) const
{
    if (row < 0 || row >= m_items.size()) return {};
    return m_items.at(row).toMap();
}

} // namespace apx::ui
