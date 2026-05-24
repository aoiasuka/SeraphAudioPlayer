// =============================================================================
//  ui/bridge/PlayerViewModel.cpp
// =============================================================================
#include "PlayerViewModel.h"
#include "platform/taskbar/JumpList.h"
#include "app/playlist/Playlist.h"
#include "app/playlist/PlaylistIO.h"
#include "app/playlist/CueSheet.h"
#include <QFileInfo>
#include <QUrl>
#include <QVariantMap>
#include <QSettings>
#include <QSet>
#include <QHash>
#include <QUuid>
#include <QImage>
#include <algorithm>
#include <random>

namespace apx::ui {

namespace {
// 把 QML 传来的 file:/// URI 转为本地路径;若已是本地路径则原样返回
QString toLocalPath(const QString& s)
{
    if (s.isEmpty()) return s;
    if (s.startsWith("file:/", Qt::CaseInsensitive)) {
        QUrl url(s);
        if (url.isLocalFile()) return url.toLocalFile();
    }
    return s;
}
} // namespace

PlayerViewModel::PlayerViewModel(QObject* parent)
    : QObject(parent)
{
    player_ = std::make_unique<PlayerController>();

    // 跨线程信号连接
    connect(this, &PlayerViewModel::_coreStateChanged, this, &PlayerViewModel::onCoreStateChanged, Qt::QueuedConnection);
    connect(this, &PlayerViewModel::_corePositionChanged, this, &PlayerViewModel::onCorePositionChanged, Qt::QueuedConnection);
    connect(this, &PlayerViewModel::_coreEnded, this, &PlayerViewModel::onCoreEnded, Qt::QueuedConnection);
    connect(this, &PlayerViewModel::_coreError, this, &PlayerViewModel::onCoreError, Qt::QueuedConnection);

    player_->setOnStateChanged([this](PlayerState s) { emit _coreStateChanged(static_cast<int>(s)); });
    player_->setOnPositionChanged([this](double sec) { emit _corePositionChanged(sec); });
    player_->setOnEnded([this]() { emit _coreEnded(); });
    player_->setOnError([this](const std::wstring& msg) { emit _coreError(QString::fromStdWString(msg)); });
    // PCM tap → 可视化(运行在 producer 线程,Visualizer 内部有 mutex)
    player_->setOnPcmTap([this](const std::uint8_t* data, std::size_t bytes, const apx::AudioFormat& fmt) {
        m_visualizer.push(data, bytes, fmt);
    });
    // DSP processor → EQ (in-place 修改 PCM)
    player_->setPcmProcessor([this](std::uint8_t* data, std::size_t bytes, const apx::AudioFormat& fmt) {
        m_eq.process(data, bytes, fmt);
    });

    // 可视化 30Hz 刷新
    m_visTimer = new QTimer(this);
    m_visTimer->setInterval(33);
    connect(m_visTimer, &QTimer::timeout, this, &PlayerViewModel::onVisTick);
    m_visTimer->start();

    // 渲染统计 1Hz 刷新 (诊断面板用,频率低不耗 CPU)
    m_statsTimer = new QTimer(this);
    m_statsTimer->setInterval(1000);
    connect(m_statsTimer, &QTimer::timeout, this, &PlayerViewModel::onStatsTick);
    m_statsTimer->start();

    // 设备热插拔
    device_bridge_ = std::make_unique<DeviceBridge>();
    connect(device_bridge_.get(), &DeviceBridge::devicesChanged, this, &PlayerViewModel::onDevicesChanged, Qt::QueuedConnection);
    connect(device_bridge_.get(), &DeviceBridge::devicesChanged, this, &PlayerViewModel::devicesChanged, Qt::QueuedConnection);
    device_bridge_->start();

    // 加载持久化设置(在设备列表拉取前)
    loadSettings();

    // 初始拉一次设备列表
    onDevicesChanged();

    // 如果有偏好设备,尝试应用
    if (!m_preferredDeviceId.isEmpty() && m_preferredDeviceId != m_currentDeviceId) {
        // 仅当该设备真实存在
        for (const auto& dv : m_devicesCache) {
            auto map = dv.toMap();
            if (map.value("id").toString() == m_preferredDeviceId) {
                setDevice(m_preferredDeviceId);
                break;
            }
        }
    }

    // 应用默认音量
    applyVolumeToPlayer();

    // 通知 QML 持久化加载完成后的初始状态
    emit volumeChanged();
    emit mutedChanged();
    emit repeatModeChanged();
    emit shuffleChanged();
    emit recentChanged();
    emit likedChanged();
    emit libraryChanged();
    emit playlistsChanged();
}

PlayerViewModel::~PlayerViewModel()
{
    saveSettings();
    if (taskbar_) { taskbar_->shutdown(); taskbar_.reset(); }
    if (smtc_) { smtc_->shutdown(); smtc_.reset(); }
    if (device_bridge_) device_bridge_->stop();
    if (player_) player_->unloadFile();
}

void PlayerViewModel::attachWindow(void* hwnd)
{
    m_hwnd = hwnd;

    // ---- SMTC ----
    if (!smtc_) smtc_ = std::make_unique<apx::SmtcController>();
    if (smtc_->initialize(hwnd)) {
        smtc_->setOnButton([this](apx::SmtcButton b) {
            QMetaObject::invokeMethod(this, [this, b]() {
                switch (b) {
                case apx::SmtcButton::Play:     play();      break;
                case apx::SmtcButton::Pause:    pause();     break;
                case apx::SmtcButton::Stop:     stop();      break;
                case apx::SmtcButton::Next:     next();      break;
                case apx::SmtcButton::Previous: previous();  break;
                }
            }, Qt::QueuedConnection);
        });
        syncSmtcMetadata();
        syncSmtcStatus();
        syncSmtcTimeline();
    } else {
        smtc_.reset();
    }

    // ---- 任务栏缩略图按钮 ----
    if (!taskbar_) taskbar_ = std::make_unique<apx::TaskbarButtons>();
    if (taskbar_->initialize(hwnd)) {
        taskbar_->setOnButton([this](apx::TaskbarButton b) {
            QMetaObject::invokeMethod(this, [this, b]() {
                switch (b) {
                case apx::TaskbarButton::Previous:  previous();  break;
                case apx::TaskbarButton::Next:      next();      break;
                case apx::TaskbarButton::PlayPause:
                    if (m_state == 2) pause(); else play();
                    break;
                }
            }, Qt::QueuedConnection);
        });
        syncTaskbar();
    } else {
        taskbar_.reset();
    }
}

void PlayerViewModel::syncSmtcMetadata()
{
    if (!smtc_) return;
    QString cur = currentPath();
    if (cur.isEmpty()) {
        smtc_->setMetadata(L"", L"", L"");
        smtc_->setThumbnail(nullptr, 0);
        return;
    }
    apx::TrackMetadata md;
    QString title = m_title;
    QString artist;
    QString album;
    if (fetchMeta(cur, md)) {
        if (!md.artist.empty()) artist = QString::fromStdWString(md.artist);
        if (!md.album.empty())  album  = QString::fromStdWString(md.album);
    }
    smtc_->setMetadata(title.toStdWString(),
                       artist.toStdWString(),
                       album.toStdWString());
    // 封面
    if (md.has_cover) {
        auto cov = apx::MetadataReader::readCover(cur.toStdWString());
        if (cov && !cov->data.empty()) {
            smtc_->setThumbnail(cov->data.data(), cov->data.size());
        } else {
            smtc_->setThumbnail(nullptr, 0);
        }
    } else {
        smtc_->setThumbnail(nullptr, 0);
    }
}

void PlayerViewModel::syncSmtcStatus()
{
    if (!smtc_) return;
    apx::SmtcStatus s;
    switch (m_state) {
    case 1: s = apx::SmtcStatus::Stopped; break;   // PlayerState::Stopped
    case 2: s = apx::SmtcStatus::Playing; break;   // Playing
    case 3: s = apx::SmtcStatus::Paused;  break;   // Paused
    case 4: s = apx::SmtcStatus::Stopped; break;   // Ended
    default: s = apx::SmtcStatus::Closed; break;   // Idle
    }
    smtc_->setStatus(s);
}

void PlayerViewModel::syncSmtcTimeline()
{
    if (!smtc_) return;
    smtc_->setTimeline(m_position, m_duration);
}

void PlayerViewModel::syncTaskbar()
{
    if (!taskbar_) return;
    taskbar_->setPlaying(m_state == 2);
    // canPrev / canNext 必须与 repeatMode / shuffle 一致:
    //   - LoopList (repeatMode == 1) 或 Shuffle:总是允许两边
    //   - LoopOne (repeatMode == 2):允许两边(重新加载当前)
    //   - Sequential (0):头/尾时禁用
    bool canPrev = false, canNext = false;
    if (m_queue.isEmpty()) {
        canPrev = canNext = false;
    } else if (m_repeatMode == 1 || m_shuffle || m_repeatMode == 2) {
        canPrev = canNext = true;
    } else {
        canPrev = m_currentIndex > 0;
        canNext = m_currentIndex >= 0 && m_currentIndex < m_queue.size() - 1;
    }
    taskbar_->setNavEnabled(canPrev, canNext);
}

QVariantList PlayerViewModel::spectrum() const
{
    return m_spectrum;
}

void PlayerViewModel::onVisTick()
{
    auto snap = m_visualizer.snapshot();
    m_vu_l   = snap.vu_left;
    m_vu_r   = snap.vu_right;
    m_peak_l = snap.peak_left;
    m_peak_r = snap.peak_right;
    QVariantList bands;
    bands.reserve(static_cast<int>(snap.bands.size()));
    for (float b : snap.bands) bands.append(static_cast<double>(b));
    m_spectrum = bands;
    emit visualUpdated();
}

// ---- EQ ----

QVariantList PlayerViewModel::eqGains() const
{
    QVariantList out;
    out.reserve(apx::Equalizer::kNumBands);
    for (int i = 0; i < apx::Equalizer::kNumBands; ++i) {
        out.append(m_eq.gain(i));
    }
    return out;
}

void PlayerViewModel::setEqEnabled(bool on)
{
    if (m_eq.enabled() == on) return;
    m_eq.setEnabled(on);
    if (!m_loadingSettings) saveSettings();
    emit eqChanged();
}

void PlayerViewModel::setEqGain(int band, double db)
{
    m_eq.setGain(band, db);
    if (!m_loadingSettings) saveSettings();
    emit eqChanged();
}

void PlayerViewModel::resetEq()
{
    for (int i = 0; i < apx::Equalizer::kNumBands; ++i) m_eq.setGain(i, 0.0);
    if (!m_loadingSettings) saveSettings();
    emit eqChanged();
}

// ---- 播放控制 ----

void PlayerViewModel::play()
{
    if (m_currentIndex < 0 && !m_queue.isEmpty()) {
        loadAndPlay(0);
        return;
    }
    if (!player_->play()) {
        m_lastError = QString::fromStdWString(player_->lastError());
        emit errorOccurred(m_lastError);
    }
}

void PlayerViewModel::pause() { player_->pause(); }
void PlayerViewModel::stop()  { player_->stop(); }
void PlayerViewModel::seek(double sec) { player_->seek(sec); }

// ---- 队列 ----

void PlayerViewModel::openFile(const QString& path)
{
    QString local = toLocalPath(path);
    if (local.isEmpty()) return;

    // 已在队列: 直接跳过去播, 不动队列其他歌曲
    int idx = m_queue.indexOf(local);
    if (idx >= 0) {
        playIndex(idx);
        return;
    }

    // 未在队列: 追加到末尾, 然后从该位置开始播
    // (历史上这里是 clear + append, 会把整队列重置成只有当前一首,
    //  导致 library/recent/liked 等依赖 m_queue 的派生集合一并缩水.
    //  现在改为 append-and-play, 保留队列其他歌曲)
    m_queue.append(local);
    touchLibrary(local);
    emit queueChanged();
    emit libraryChanged();

    int newIdx = m_queue.size() - 1;
    if (m_shuffle) rebuildShuffleOrder(newIdx);

    loadAndPlay(newIdx);
}

void PlayerViewModel::enqueue(const QString& path)
{
    QString local = toLocalPath(path);
    if (local.isEmpty()) return;
    m_queue.append(local);
    touchLibrary(local);
    emit queueChanged();
    emit libraryChanged();

    if (m_shuffle) rebuildShuffleOrder(std::max(0, m_currentIndex));

    if (m_currentIndex < 0) {
        // 队列原本为空,加进来直接开始播
        loadAndPlay(m_queue.size() - 1);
    }
}

void PlayerViewModel::enqueueMany(const QStringList& paths)
{
    bool wasEmpty = m_queue.isEmpty();
    QStringList added;
    added.reserve(paths.size());
    for (const auto& p : paths) {
        QString local = toLocalPath(p);
        if (!local.isEmpty()) {
            m_queue.append(local);
            added.append(local);
        }
    }
    touchLibraryMany(added);
    emit queueChanged();
    emit libraryChanged();
    if (m_shuffle) rebuildShuffleOrder(std::max(0, m_currentIndex));
    if (wasEmpty && !m_queue.isEmpty()) loadAndPlay(0);
}

void PlayerViewModel::playIndex(int index)
{
    if (index < 0 || index >= m_queue.size()) return;
    if (m_shuffle) {
        // 重建 shuffle 顺序,使当前播放从 index 开始
        rebuildShuffleOrder(index);
    }
    loadAndPlay(index);
}

void PlayerViewModel::next()
{
    if (m_queue.isEmpty()) return;
    int idx = nextIndexAfter(m_currentIndex);
    if (idx < 0) {
        // 没有下一首:停在末尾
        player_->stop();
        return;
    }
    loadAndPlay(idx);
}

void PlayerViewModel::previous()
{
    if (m_queue.isEmpty()) return;
    // 已播放超过 3 秒:回到当前曲目开头
    if (m_position > 3.0 && m_currentIndex >= 0) {
        player_->seek(0.0);
        return;
    }

    int idx;
    if (m_shuffle) {
        if (m_shufflePos <= 0) idx = m_shuffleOrder.isEmpty() ? -1 : m_shuffleOrder.first();
        else idx = m_shuffleOrder.value(--m_shufflePos, -1);
    } else {
        idx = m_currentIndex - 1;
        if (idx < 0) idx = (m_repeatMode == 1) ? m_queue.size() - 1 : 0;
    }
    if (idx >= 0 && idx < m_queue.size()) loadAndPlay(idx);
}

void PlayerViewModel::clearQueue()
{
    player_->unloadFile();
    m_queue.clear();
    m_shuffleOrder.clear();
    m_shufflePos = -1;
    m_currentIndex = -1;
    m_title = "未播放";
    m_formatInfo = "";
    m_duration = 0.0;
    m_position = 0.0;
    m_visualizer.reset();
    emit queueChanged();
    emit currentIndexChanged();
    emit titleChanged();
    emit formatInfoChanged();
    emit durationChanged();
    emit positionChanged();
    emit currentLikedChanged();
    emit currentCoverUrlChanged();
    emit libraryChanged();
    reloadLyricsForCurrent();
    syncTaskbar();
    syncSmtcMetadata();
}

void PlayerViewModel::removeAt(int index)
{
    if (index < 0 || index >= m_queue.size()) return;
    bool wasCurrent = (index == m_currentIndex);
    m_queue.removeAt(index);
    if (m_currentIndex > index) {
        --m_currentIndex;
        emit currentIndexChanged();
    } else if (wasCurrent) {
        // 当前的被删了:加载新位置或停止
        player_->unloadFile();
        if (index < m_queue.size()) {
            loadAndPlay(index);
        } else if (!m_queue.isEmpty() && m_repeatMode == 1) {
            loadAndPlay(0);
        } else {
            m_currentIndex = -1;
            emit currentIndexChanged();
        }
    }
    if (m_shuffle) rebuildShuffleOrder(std::max(0, m_currentIndex));
    emit queueChanged();
    emit libraryChanged();
    syncTaskbar();
}

void PlayerViewModel::moveQueueItem(int from, int to)
{
    if (from < 0 || from >= m_queue.size()) return;
    if (to < 0) to = 0;
    if (to >= m_queue.size()) to = m_queue.size() - 1;
    if (from == to) return;

    // 保留当前 index 指向的实际路径
    QString curPath = (m_currentIndex >= 0 && m_currentIndex < m_queue.size())
                        ? m_queue.at(m_currentIndex) : QString();

    m_queue.move(from, to);

    // 重新定位 m_currentIndex
    if (!curPath.isEmpty()) {
        int newIdx = m_queue.indexOf(curPath);
        if (newIdx >= 0 && newIdx != m_currentIndex) {
            m_currentIndex = newIdx;
            emit currentIndexChanged();
        }
    }

    if (m_shuffle) rebuildShuffleOrder(std::max(0, m_currentIndex));
    emit queueChanged();
    syncTaskbar();
}

// ---- 喜欢 ----

bool PlayerViewModel::isLiked(const QString& path) const
{
    if (path.isEmpty()) return false;
    return m_liked.contains(toLocalPath(path));
}

void PlayerViewModel::toggleLike(const QString& path)
{
    QString local = toLocalPath(path);
    if (local.isEmpty()) return;
    if (m_liked.contains(local)) m_liked.removeAll(local);
    else {
        m_liked.prepend(local);
        touchLibrary(local);
    }
    saveSettings();
    emit likedChanged();
    emit libraryChanged();
    if (local == currentPath()) emit currentLikedChanged();
}

void PlayerViewModel::toggleLikeCurrent()
{
    QString cur = currentPath();
    if (!cur.isEmpty()) toggleLike(cur);
}

void PlayerViewModel::removeFromLiked(const QString& path)
{
    QString local = toLocalPath(path);
    if (local.isEmpty()) return;
    if (m_liked.removeAll(local) > 0) {
        saveSettings();
        emit likedChanged();
        emit libraryChanged();
        if (local == currentPath()) emit currentLikedChanged();
    }
}

bool PlayerViewModel::currentLiked() const
{
    QString cur = currentPath();
    if (cur.isEmpty()) return false;
    return m_liked.contains(cur);
}

QString PlayerViewModel::currentCoverUrl() const
{
    QString cur = currentPath();
    if (cur.isEmpty()) return {};
    // 检查是否有封面
    apx::TrackMetadata md;
    if (!fetchMeta(cur, md) || !md.has_cover) return {};
    return QStringLiteral("image://covers/") + QString::fromUtf8(QUrl::toPercentEncoding(cur));
}

QColor PlayerViewModel::currentDominantColor() const
{
    static const QColor kDefault(0x1E, 0x40, 0xAF);   // 品牌深蓝 fallback
    QString cur = currentPath();
    if (cur.isEmpty()) return kDefault;

    auto it = m_colorCache.constFind(cur);
    if (it != m_colorCache.constEnd()) return it.value();

    apx::TrackMetadata md;
    if (!fetchMeta(cur, md) || !md.has_cover) {
        m_colorCache.insert(cur, kDefault);
        return kDefault;
    }
    auto cov = apx::MetadataReader::readCover(cur.toStdWString());
    if (!cov || cov->data.empty()) {
        m_colorCache.insert(cur, kDefault);
        return kDefault;
    }
    QImage img;
    img.loadFromData(reinterpret_cast<const uchar*>(cov->data.data()),
                     static_cast<int>(cov->data.size()));
    if (img.isNull()) {
        m_colorCache.insert(cur, kDefault);
        return kDefault;
    }
    QImage small = img.scaled(24, 24, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)
                      .convertToFormat(QImage::Format_RGB32);
    long long r = 0, g = 0, b = 0;
    int n = 0;
    for (int y = 0; y < small.height(); ++y) {
        for (int x = 0; x < small.width(); ++x) {
            QRgb px = small.pixel(x, y);
            int rr = qRed(px), gg = qGreen(px), bb = qBlue(px);
            // 跳过过亮 / 过暗的像素,避免被背景白边或黑边主导
            int mx = std::max(rr, std::max(gg, bb));
            int mn = std::min(rr, std::min(gg, bb));
            if (mx > 245 && mn > 230) continue;   // 接近纯白
            if (mx < 16) continue;                // 接近纯黑
            r += rr; g += gg; b += bb;
            ++n;
        }
    }
    QColor out;
    if (n == 0) {
        out = kDefault;
    } else {
        QColor avg(static_cast<int>(r / n),
                   static_cast<int>(g / n),
                   static_cast<int>(b / n));
        int h, s, v;
        avg.getHsv(&h, &s, &v);
        if (h < 0) h = 220;
        s = std::min(255, s + 60);
        v = std::clamp(v, 90, 180);
        out = QColor::fromHsv(h, s, v);
    }
    m_colorCache.insert(cur, out);
    return out;
}

void PlayerViewModel::setVisualizerType(int type)
{
    if (m_visualizerType == type) return;
    m_visualizerType = type;
    emit visualizerTypeChanged();
    saveSettings();
}

QVariantList PlayerViewModel::currentLyrics() const
{
    QVariantList out;
    out.reserve(static_cast<int>(m_lyrics.size()));
    for (const auto& ln : m_lyrics) {
        QVariantMap m;
        m["time"] = ln.time_sec;
        m["text"] = QString::fromStdWString(ln.text);
        out.append(m);
    }
    return out;
}

void PlayerViewModel::reloadLyricsForCurrent()
{
    QString cur = currentPath();
    if (cur == m_lyricsForPath) return;
    m_lyricsForPath = cur;
    m_lyrics.clear();
    if (!cur.isEmpty()) {
        m_lyrics = apx::LyricsLoader::loadFor(cur.toStdWString());
    }
    m_lyricIndex = -1;
    emit lyricsChanged();
    emit currentLyricIndexChanged();
}

void PlayerViewModel::updateLyricIndex(double pos)
{
    if (m_lyrics.empty()) {
        if (m_lyricIndex != -1) {
            m_lyricIndex = -1;
            emit currentLyricIndexChanged();
        }
        return;
    }
    // 二分查找最后一个 time <= pos
    int lo = 0, hi = static_cast<int>(m_lyrics.size()) - 1, ans = -1;
    while (lo <= hi) {
        int mid = (lo + hi) / 2;
        if (m_lyrics[mid].time_sec <= pos) { ans = mid; lo = mid + 1; }
        else hi = mid - 1;
    }
    if (ans != m_lyricIndex) {
        m_lyricIndex = ans;
        emit currentLyricIndexChanged();
    }
}

QString PlayerViewModel::currentPath() const
{
    if (m_currentIndex >= 0 && m_currentIndex < m_queue.size()) {
        return m_queue.at(m_currentIndex);
    }
    return {};
}

// ---- 最近播放管理 ----

void PlayerViewModel::removeFromRecent(const QString& path)
{
    QString local = toLocalPath(path);
    if (m_recent.removeAll(local) > 0) {
        saveSettings();
        emit recentChanged();
        emit libraryChanged();
    }
}

void PlayerViewModel::clearRecent()
{
    if (m_recent.isEmpty()) return;
    m_recent.clear();
    saveSettings();
    emit recentChanged();
    emit libraryChanged();
}

// ---- 音量/模式 ----

void PlayerViewModel::setVolume(int v)
{
    v = std::clamp(v, 0, 100);
    if (m_volume == v) return;
    m_volume = v;
    applyVolumeToPlayer();
    emit volumeChanged();
    if (!m_loadingSettings) saveSettings();
}

void PlayerViewModel::setMuted(bool b)
{
    if (m_muted == b) return;
    m_muted = b;
    applyVolumeToPlayer();
    emit mutedChanged();
    if (!m_loadingSettings) saveSettings();
}

void PlayerViewModel::setRepeatMode(int m)
{
    m = std::clamp(m, 0, 2);
    if (m_repeatMode == m) return;
    m_repeatMode = m;
    emit repeatModeChanged();
    syncTaskbar();
    if (!m_loadingSettings) saveSettings();
}

void PlayerViewModel::setShuffle(bool s)
{
    if (m_shuffle == s) return;
    m_shuffle = s;
    if (m_shuffle) rebuildShuffleOrder(std::max(0, m_currentIndex));
    else { m_shuffleOrder.clear(); m_shufflePos = -1; }
    emit shuffleChanged();
    syncTaskbar();
    if (!m_loadingSettings) saveSettings();
}

void PlayerViewModel::applyVolumeToPlayer()
{
    double v = m_muted ? 0.0 : (m_volume / 100.0);
    player_->setVolume(v);
}

// ---- 设备 ----

void PlayerViewModel::setDevice(const QString& deviceId)
{
    if (!player_->setDevice(deviceId.toStdWString())) {
        m_lastError = QString::fromStdWString(player_->lastError());
        emit errorOccurred(m_lastError);
        return;
    }
    QString newId = QString::fromStdWString(player_->currentDeviceId());
    if (newId != m_currentDeviceId) {
        m_currentDeviceId = newId;
        emit currentDeviceChanged();
    }
    m_preferredDeviceId = m_currentDeviceId;
    if (!m_loadingSettings) saveSettings();
}

void PlayerViewModel::refreshDevices()
{
    onDevicesChanged();
}

QString PlayerViewModel::currentDeviceNameProp() const
{
    return QString::fromStdWString(player_->currentDeviceName());
}

QVariantList PlayerViewModel::devices() const
{
    return m_devicesCache;
}

void PlayerViewModel::onDevicesChanged()
{
    if (!device_bridge_) return;
    auto items = device_bridge_->snapshotActive();
    QVariantList out;
    out.reserve(items.size());
    for (const auto& it : items) {
        QVariantMap m;
        m["id"] = it.id;
        m["name"] = it.friendly_name;
        m["isDefault"] = it.is_default_console;
        out.append(m);
    }
    m_devicesCache = std::move(out);
    emit devicesListChanged();

    QString cur = QString::fromStdWString(player_->currentDeviceId());
    if (cur != m_currentDeviceId) {
        m_currentDeviceId = cur;
        emit currentDeviceChanged();
    }
}

// ---- 内部 ----

bool PlayerViewModel::loadAndPlay(int index)
{
    if (index < 0 || index >= m_queue.size()) return false;
    const QString& path = m_queue[index];

    if (!player_->loadFile(path.toStdWString())) {
        m_lastError = QString::fromStdWString(player_->lastError());
        emit errorOccurred(m_lastError);
        return false;
    }
    if (m_currentIndex != index) {
        m_currentIndex = index;
        emit currentIndexChanged();
    }
    updateFileInfo();
    pushRecent(path);

    if (m_shuffle) {
        // 同步 shuffle 游标
        int p = m_shuffleOrder.indexOf(index);
        if (p >= 0) m_shufflePos = p;
    }

    // 应用当前音量
    applyVolumeToPlayer();

    if (!player_->play()) {
        m_lastError = QString::fromStdWString(player_->lastError());
        emit errorOccurred(m_lastError);
        return false;
    }
    emit currentLikedChanged();
    emit currentCoverUrlChanged();
    reloadLyricsForCurrent();
    syncSmtcMetadata();
    syncSmtcStatus();
    syncSmtcTimeline();
    syncTaskbar();
    // current 字段会嵌入到各派生集合,通知 QML 重读
    emit queueChanged();
    emit recentChanged();
    emit likedChanged();
    emit libraryChanged();
    emit playlistsChanged();
    return true;
}

void PlayerViewModel::rebuildShuffleOrder(int startIndex)
{
    m_shuffleOrder.clear();
    if (m_queue.isEmpty()) { m_shufflePos = -1; return; }

    QList<int> rest;
    for (int i = 0; i < m_queue.size(); ++i) {
        if (i != startIndex) rest.append(i);
    }
    std::random_device rd;
    std::mt19937 g(rd());
    std::shuffle(rest.begin(), rest.end(), g);

    if (startIndex >= 0 && startIndex < m_queue.size()) {
        m_shuffleOrder.append(startIndex);
    }
    for (int i : rest) m_shuffleOrder.append(i);
    m_shufflePos = (startIndex >= 0 && startIndex < m_queue.size()) ? 0 : -1;
}

int PlayerViewModel::nextIndexAfter(int currentIndex) const
{
    if (m_queue.isEmpty()) return -1;

    if (m_repeatMode == 2) {
        // 单曲循环:仍由 onCoreEnded 处理,这里走"下一首"的语义=往后跳一首
    }

    if (m_shuffle) {
        if (m_shuffleOrder.isEmpty()) return -1;
        int p = m_shufflePos + 1;
        if (p >= m_shuffleOrder.size()) {
            return (m_repeatMode == 1) ? m_shuffleOrder.first() : -1;
        }
        return m_shuffleOrder.value(p, -1);
    }

    int idx = currentIndex + 1;
    if (idx >= m_queue.size()) {
        return (m_repeatMode == 1) ? 0 : -1;
    }
    return idx;
}

void PlayerViewModel::pushRecent(const QString& path)
{
    m_recent.removeAll(path);
    m_recent.prepend(path);
    while (m_recent.size() > kMaxRecent) m_recent.removeLast();
    touchLibrary(path);
    if (!m_loadingSettings) saveSettings();
    apx::JumpList::addRecent(path.toStdWString());
    emit recentChanged();
    emit libraryChanged();
}

void PlayerViewModel::touchLibrary(const QString& path)
{
    if (path.isEmpty()) return;
    if (m_libraryOrder.contains(path)) return;
    m_libraryOrder.append(path);
}

void PlayerViewModel::touchLibraryMany(const QStringList& paths)
{
    for (const auto& p : paths) touchLibrary(p);
}

void PlayerViewModel::updateFileInfo()
{
    auto fmt = player_->format();
    m_formatInfo = QString::fromStdWString(fmt.to_wstring());
    emit formatInfoChanged();

    double dur = player_->duration();
    if (m_duration != dur) {
        m_duration = dur;
        emit durationChanged();
    }

    QString newTitle = QFileInfo(QString::fromStdWString(player_->currentFile())).completeBaseName();
    if (newTitle.isEmpty()) newTitle = "未知曲目";
    if (m_title != newTitle) {
        m_title = newTitle;
        emit titleChanged();
    }
}

void PlayerViewModel::onCoreStateChanged(int s)
{
    if (m_state != s) {
        m_state = s;
        emit stateChanged();
        syncSmtcStatus();
        syncTaskbar();
    }
}

void PlayerViewModel::onCorePositionChanged(double sec)
{
    m_position = sec;
    emit positionChanged();
    updateLyricIndex(sec);
    // 频率不要太高:每 2 秒同步一次时间线给 SMTC
    static thread_local double lastSent = -10.0;
    if (smtc_ && std::abs(sec - lastSent) >= 2.0) {
        lastSent = sec;
        syncSmtcTimeline();
    }
}

void PlayerViewModel::onCoreEnded()
{
    // 单曲循环:重头开始播
    if (m_repeatMode == 2 && m_currentIndex >= 0) {
        player_->seek(0.0);
        player_->play();
        m_position = 0.0;
        emit positionChanged();
        return;
    }

    int idx = nextIndexAfter(m_currentIndex);
    if (idx >= 0) {
        loadAndPlay(idx);
    } else {
        // 列表已结束:位置归零,等 onCoreStateChanged 更新到 Ended
        m_position = 0.0;
        emit positionChanged();
    }
}

void PlayerViewModel::onCoreError(const QString& msg)
{
    m_lastError = msg;
    emit errorOccurred(msg);
}

QVariantList PlayerViewModel::itemsFromPaths(const QStringList& paths, int currentIdx) const
{
    QVariantList out;
    out.reserve(paths.size());
    for (int i = 0; i < paths.size(); ++i) {
        const QString& p = paths[i];
        QFileInfo fi(p);
        QVariantMap m;
        m["path"] = p;

        QString title = fi.completeBaseName();
        QString artist;
        QString album;
        double duration_sec = 0.0;

        apx::TrackMetadata md;
        if (fetchMeta(p, md)) {
            if (!md.title.empty())  title  = QString::fromStdWString(md.title);
            if (!md.artist.empty()) artist = QString::fromStdWString(md.artist);
            if (!md.album.empty())  album  = QString::fromStdWString(md.album);
            duration_sec = md.duration_sec;
        }

        // 没有 ARTIST/ALBUM 时,用文件类型 + 目录作为兜底显示(保持之前的视觉)
        if (artist.isEmpty()) artist = fi.suffix().toUpper();
        if (album.isEmpty())  album  = fi.absolutePath();

        QString durationStr;
        if (duration_sec > 0.5) {
            int total = static_cast<int>(duration_sec + 0.5);
            int mm = total / 60;
            int ss = total % 60;
            durationStr = QString("%1:%2").arg(mm, 2, 10, QLatin1Char('0')).arg(ss, 2, 10, QLatin1Char('0'));
        }

        // 当前曲目同步: 路径相等 OR queue 中索引匹配
        bool isCur = (i == currentIdx);
        if (!isCur && m_currentIndex >= 0 && m_currentIndex < m_queue.size()) {
            isCur = (p == m_queue.at(m_currentIndex));
        }

        m["title"]      = title;
        m["artist"]     = artist;
        m["album"]      = album;
        m["suffix"]     = fi.suffix().toUpper();
        m["dir"]        = fi.absolutePath();
        m["fileName"]   = fi.fileName();
        m["duration"]   = durationStr;
        m["isCurrent"]  = isCur;
        m["liked"]      = m_liked.contains(p);
        m["hasCover"]   = md.has_cover;
        m["coverUrl"]   = md.has_cover
            ? (QStringLiteral("image://covers/") + QString::fromUtf8(QUrl::toPercentEncoding(p)))
            : QString();
        out.append(m);
    }
    return out;
}

bool PlayerViewModel::fetchMeta(const QString& path, apx::TrackMetadata& out) const
{
    auto it = m_metaCache.constFind(path);
    if (it != m_metaCache.constEnd()) { out = it.value(); return true; }
    if (m_metaMissed.contains(path))   return false;

    auto opt = apx::MetadataReader::read(path.toStdWString());
    if (!opt) {
        m_metaMissed.append(path);
        if (m_metaMissed.size() > 256) m_metaMissed.removeFirst();
        return false;
    }
    m_metaCache.insert(path, *opt);
    out = *opt;
    return true;
}

// queue/recent getters 暴露 QVariantList(由 QML 直接用)

QVariantList PlayerViewModel::queue() const
{
    return itemsFromPaths(m_queue, m_currentIndex);
}

QVariantList PlayerViewModel::recent() const
{
    return itemsFromPaths(m_recent, -1);
}

QVariantList PlayerViewModel::liked() const
{
    return itemsFromPaths(m_liked, -1);
}

QVariantList PlayerViewModel::library() const
{
    // 顺序: 严格按 m_libraryOrder (首次见到顺序)
    // 内容: 仅保留当前仍存活于 queue/recent/liked/playlists 中的 path
    // 这样切歌或重排队列不会引起列表抖动 -- 视觉上只有 isCurrent 高亮在移动
    QSet<QString> alive;
    for (const auto& p : m_queue)  alive.insert(p);
    for (const auto& p : m_recent) alive.insert(p);
    for (const auto& p : m_liked)  alive.insert(p);
    for (const auto& pl : m_playlists)
        for (const auto& p : pl.paths) alive.insert(p);

    QStringList ordered;
    ordered.reserve(alive.size());
    for (const auto& p : m_libraryOrder) {
        if (alive.contains(p)) ordered.append(p);
    }
    return itemsFromPaths(ordered, -1);
}

// ---- 歌单 ----

QVariantList PlayerViewModel::playlists() const
{
    QVariantList out;
    out.reserve(m_playlists.size());
    for (const auto& pl : m_playlists) {
        QVariantMap m;
        m["id"]    = pl.id;
        m["name"]  = pl.name;
        m["count"] = pl.paths.size();
        out.append(m);
    }
    return out;
}

QString PlayerViewModel::createPlaylist(const QString& name)
{
    QString trimmed = name.trimmed();
    if (trimmed.isEmpty()) trimmed = "新建歌单";
    Playlist pl;
    pl.id   = QUuid::createUuid().toString(QUuid::WithoutBraces);
    pl.name = trimmed;
    m_playlists.append(pl);
    saveSettings();
    emit playlistsChanged();
    return pl.id;
}

void PlayerViewModel::renamePlaylist(const QString& id, const QString& name)
{
    QString trimmed = name.trimmed();
    if (trimmed.isEmpty()) return;
    for (auto& pl : m_playlists) {
        if (pl.id == id) {
            if (pl.name == trimmed) return;
            pl.name = trimmed;
            saveSettings();
            emit playlistsChanged();
            return;
        }
    }
}

void PlayerViewModel::deletePlaylist(const QString& id)
{
    for (int i = 0; i < m_playlists.size(); ++i) {
        if (m_playlists[i].id == id) {
            m_playlists.removeAt(i);
            saveSettings();
            emit playlistsChanged();
            emit libraryChanged();
            return;
        }
    }
}

void PlayerViewModel::addToPlaylist(const QString& id, const QString& path)
{
    QString local = toLocalPath(path);
    if (local.isEmpty()) return;
    for (auto& pl : m_playlists) {
        if (pl.id == id) {
            if (!pl.paths.contains(local)) {
                pl.paths.append(local);
                touchLibrary(local);
                saveSettings();
                emit playlistsChanged();
                emit libraryChanged();
            }
            return;
        }
    }
}

void PlayerViewModel::addManyToPlaylist(const QString& id, const QStringList& paths)
{
    for (auto& pl : m_playlists) {
        if (pl.id == id) {
            bool changed = false;
            QStringList added;
            for (const auto& p : paths) {
                QString local = toLocalPath(p);
                if (!local.isEmpty() && !pl.paths.contains(local)) {
                    pl.paths.append(local);
                    added.append(local);
                    changed = true;
                }
            }
            if (changed) {
                touchLibraryMany(added);
                saveSettings();
                emit playlistsChanged();
                emit libraryChanged();
            }
            return;
        }
    }
}

void PlayerViewModel::removeFromPlaylist(const QString& id, const QString& path)
{
    QString local = toLocalPath(path);
    for (auto& pl : m_playlists) {
        if (pl.id == id) {
            if (pl.paths.removeAll(local) > 0) {
                saveSettings();
                emit playlistsChanged();
                emit libraryChanged();
            }
            return;
        }
    }
}

void PlayerViewModel::movePlaylistItem(const QString& id, int from, int to)
{
    for (auto& pl : m_playlists) {
        if (pl.id == id) {
            if (from < 0 || from >= pl.paths.size()) return;
            if (to < 0) to = 0;
            if (to >= pl.paths.size()) to = pl.paths.size() - 1;
            if (from == to) return;
            pl.paths.move(from, to);
            saveSettings();
            emit playlistsChanged();
            return;
        }
    }
}

void PlayerViewModel::playPlaylist(const QString& id)
{
    for (const auto& pl : m_playlists) {
        if (pl.id == id) {
            if (pl.paths.isEmpty()) return;
            clearQueue();
            enqueueMany(pl.paths);
            return;
        }
    }
}

void PlayerViewModel::enqueuePlaylist(const QString& id)
{
    for (const auto& pl : m_playlists) {
        if (pl.id == id) {
            enqueueMany(pl.paths);
            return;
        }
    }
}

QVariantList PlayerViewModel::playlistTracks(const QString& id) const
{
    for (const auto& pl : m_playlists) {
        if (pl.id == id) return itemsFromPaths(pl.paths, -1);
    }
    return {};
}

QVariantMap PlayerViewModel::playlistById(const QString& id) const
{
    for (const auto& pl : m_playlists) {
        if (pl.id == id) {
            QVariantMap m;
            m["id"]    = pl.id;
            m["name"]  = pl.name;
            m["count"] = pl.paths.size();
            return m;
        }
    }
    return {};
}

// ---- 歌手 / 专辑聚合 ----

namespace {
struct PathMeta {
    QString path;
    QString artist;
    QString album;
};
}  // namespace

QVariantList PlayerViewModel::artists() const
{
    // 收集所有已知路径
    QStringList all;
    QSet<QString> seen;
    auto push = [&](const QStringList& src) {
        for (const auto& p : src) {
            if (!seen.contains(p)) { seen.insert(p); all.append(p); }
        }
    };
    push(m_queue); push(m_recent); push(m_liked);
    for (const auto& pl : m_playlists) push(pl.paths);

    // 按 artist 聚合: count + 第一个带封面的 path
    struct ArtistAgg { int count = 0; QString coverPath; };
    QMap<QString, ArtistAgg> counter;
    for (const auto& p : all) {
        apx::TrackMetadata md;
        QString artist = QStringLiteral("未知歌手");
        bool hasCover = false;
        if (fetchMeta(p, md)) {
            if (!md.artist.empty()) {
                artist = QString::fromStdWString(md.artist).trimmed();
                if (artist.isEmpty()) artist = QStringLiteral("未知歌手");
            }
            hasCover = md.has_cover;
        }
        auto& a = counter[artist];
        a.count += 1;
        if (a.coverPath.isEmpty() && hasCover) a.coverPath = p;
    }

    QVariantList out;
    out.reserve(counter.size());
    for (auto it = counter.constBegin(); it != counter.constEnd(); ++it) {
        QVariantMap m;
        m["name"]  = it.key();
        m["count"] = it.value().count;
        m["coverUrl"] = it.value().coverPath.isEmpty() ? QString()
            : (QStringLiteral("image://covers/") + QString::fromUtf8(QUrl::toPercentEncoding(it.value().coverPath)));
        out.append(m);
    }
    // 按 count 倒序
    std::sort(out.begin(), out.end(), [](const QVariant& a, const QVariant& b) {
        return a.toMap().value("count").toInt() > b.toMap().value("count").toInt();
    });
    return out;
}

QVariantList PlayerViewModel::albums() const
{
    QStringList all;
    QSet<QString> seen;
    auto push = [&](const QStringList& src) {
        for (const auto& p : src) {
            if (!seen.contains(p)) { seen.insert(p); all.append(p); }
        }
    };
    push(m_queue); push(m_recent); push(m_liked);
    for (const auto& pl : m_playlists) push(pl.paths);

    // 按 album+artist 聚合
    struct AlbumKey { QString album; QString artist; };
    QMap<QString, QVariantMap> agg;  // 用 "albumartist" 做 key
    for (const auto& p : all) {
        apx::TrackMetadata md;
        QString album = QStringLiteral("未知专辑");
        QString artist;
        bool hasCover = false;
        if (fetchMeta(p, md)) {
            if (!md.album.empty())  album  = QString::fromStdWString(md.album).trimmed();
            if (!md.artist.empty()) artist = QString::fromStdWString(md.artist).trimmed();
            hasCover = md.has_cover;
        }
        if (album.isEmpty())  album  = QStringLiteral("未知专辑");
        QString key = album + QChar(0x01) + artist;
        auto it = agg.find(key);
        if (it == agg.end()) {
            QVariantMap m;
            m["album"]  = album;
            m["artist"] = artist;
            m["count"]  = 1;
            m["coverUrl"] = hasCover
                ? (QStringLiteral("image://covers/") + QString::fromUtf8(QUrl::toPercentEncoding(p)))
                : QString();
            agg.insert(key, m);
        } else {
            it->insert("count", it->value("count").toInt() + 1);
            if (it->value("coverUrl").toString().isEmpty() && hasCover) {
                it->insert("coverUrl",
                    QStringLiteral("image://covers/") + QString::fromUtf8(QUrl::toPercentEncoding(p)));
            }
        }
    }

    QVariantList out;
    out.reserve(agg.size());
    for (auto it = agg.constBegin(); it != agg.constEnd(); ++it) {
        out.append(it.value());
    }
    // 按 count 倒序
    std::sort(out.begin(), out.end(), [](const QVariant& a, const QVariant& b) {
        return a.toMap().value("count").toInt() > b.toMap().value("count").toInt();
    });
    return out;
}

QVariantList PlayerViewModel::tracksByArtist(const QString& artist) const
{
    QStringList all;
    QSet<QString> seen;
    auto push = [&](const QStringList& src) {
        for (const auto& p : src) {
            if (!seen.contains(p)) { seen.insert(p); all.append(p); }
        }
    };
    push(m_queue); push(m_recent); push(m_liked);
    for (const auto& pl : m_playlists) push(pl.paths);

    QStringList match;
    for (const auto& p : all) {
        apx::TrackMetadata md;
        QString a = QStringLiteral("未知歌手");
        if (fetchMeta(p, md) && !md.artist.empty()) {
            a = QString::fromStdWString(md.artist).trimmed();
            if (a.isEmpty()) a = QStringLiteral("未知歌手");
        }
        if (a == artist) match.append(p);
    }
    return itemsFromPaths(match, -1);
}

QVariantList PlayerViewModel::tracksByAlbum(const QString& album, const QString& artist) const
{
    QStringList all;
    QSet<QString> seen;
    auto push = [&](const QStringList& src) {
        for (const auto& p : src) {
            if (!seen.contains(p)) { seen.insert(p); all.append(p); }
        }
    };
    push(m_queue); push(m_recent); push(m_liked);
    for (const auto& pl : m_playlists) push(pl.paths);

    QStringList match;
    for (const auto& p : all) {
        apx::TrackMetadata md;
        QString a = QStringLiteral("未知专辑");
        QString ar;
        if (fetchMeta(p, md)) {
            if (!md.album.empty())  a  = QString::fromStdWString(md.album).trimmed();
            if (!md.artist.empty()) ar = QString::fromStdWString(md.artist).trimmed();
        }
        if (a.isEmpty()) a = QStringLiteral("未知专辑");
        if (a != album) continue;
        if (!artist.isEmpty() && ar != artist) continue;
        match.append(p);
    }
    return itemsFromPaths(match, -1);
}

void PlayerViewModel::playArtist(const QString& artist)
{
    auto list = tracksByArtist(artist);
    if (list.isEmpty()) return;
    QStringList paths;
    for (const auto& v : list) paths.append(v.toMap().value("path").toString());
    clearQueue();
    enqueueMany(paths);
}

void PlayerViewModel::playAlbum(const QString& album, const QString& artist)
{
    auto list = tracksByAlbum(album, artist);
    if (list.isEmpty()) return;
    QStringList paths;
    for (const auto& v : list) paths.append(v.toMap().value("path").toString());
    clearQueue();
    enqueueMany(paths);
}

// ---- 全库搜索 ----
namespace {

enum class SearchField { Any, Title, Artist, Album };

struct SearchToken {
    SearchField field = SearchField::Any;
    QString     text;   // 已 lower-case (供 QString::indexOf 配 Qt::CaseInsensitive 用; 此处直接保留原文亦可)
};

// 字段权重 — 与 QML 端 SearchUtil.js 保持一致
constexpr int kWeightTitle  = 10;
constexpr int kWeightArtist = 5;
constexpr int kWeightAlbum  = 3;
constexpr int kPrefixBonus  = 2;

// 解析字段前缀别名 (英文 + 简写 + 中文)
SearchField parseFieldAlias(const QString& key) {
    static const QHash<QString, SearchField> kAlias = {
        {"title",  SearchField::Title },  {"t",  SearchField::Title },
        {"artist", SearchField::Artist},  {"ar", SearchField::Artist},
        {QString::fromUtf8("歌手"), SearchField::Artist},
        {"album",  SearchField::Album },  {"al", SearchField::Album },
        {QString::fromUtf8("专辑"), SearchField::Album },
    };
    auto it = kAlias.constFind(key.toLower());
    return it == kAlias.constEnd() ? SearchField::Any : it.value();
}

QList<SearchToken> parseTokens(const QString& query) {
    QList<SearchToken> out;
    QString q = query;
    // 全角空格 (U+3000) 归一为半角
    q.replace(QChar(0x3000), QLatin1Char(' '));
    q = q.trimmed();
    if (q.isEmpty()) return out;

    const QStringList parts = q.split(QChar(' '), Qt::SkipEmptyParts);
    for (const QString& raw : parts) {
        SearchToken tok;
        int colon = raw.indexOf(QLatin1Char(':'));
        if (colon > 0 && colon < raw.size() - 1) {
            SearchField f = parseFieldAlias(raw.left(colon));
            if (f != SearchField::Any) {
                tok.field = f;
                tok.text  = raw.mid(colon + 1);
                out.append(std::move(tok));
                continue;
            }
        }
        tok.text = raw;
        out.append(std::move(tok));
    }
    return out;
}

// 在单个字段值上计算分数; 不匹配返回 0
int scoreField(const QString& value, const QString& needle, int weight) {
    if (value.isEmpty() || needle.isEmpty()) return 0;
    int idx = value.indexOf(needle, 0, Qt::CaseInsensitive);
    if (idx < 0) return 0;
    return weight * (idx == 0 ? kPrefixBonus : 1);
}

}  // namespace

QVariantList PlayerViewModel::searchTracks(const QString& query, int limit) const
{
    const auto tokens = parseTokens(query);
    if (tokens.isEmpty()) return {};

    // 候选集合: 与 library() 同源, 保留稳定顺序
    QSet<QString> alive;
    for (const auto& p : m_queue)  alive.insert(p);
    for (const auto& p : m_recent) alive.insert(p);
    for (const auto& p : m_liked)  alive.insert(p);
    for (const auto& pl : m_playlists)
        for (const auto& p : pl.paths) alive.insert(p);

    struct Hit { QString path; int score; int origIdx; };
    QList<Hit> hits;
    hits.reserve(alive.size());

    int origIdx = 0;
    for (const QString& path : m_libraryOrder) {
        if (!alive.contains(path)) continue;
        const int curIdx = origIdx++;

        // 提取与 itemsFromPaths 一致的可见字段, 保证用户搜的就是看到的
        QFileInfo fi(path);
        QString title = fi.completeBaseName();
        QString artist;
        QString album;
        apx::TrackMetadata md;
        if (fetchMeta(path, md)) {
            if (!md.title.empty())  title  = QString::fromStdWString(md.title);
            if (!md.artist.empty()) artist = QString::fromStdWString(md.artist);
            if (!md.album.empty())  album  = QString::fromStdWString(md.album);
        }

        int total = 0;
        bool allMatched = true;
        for (const SearchToken& tok : tokens) {
            int tokenScore = 0;
            switch (tok.field) {
            case SearchField::Title:
                tokenScore = scoreField(title, tok.text, kWeightTitle);
                break;
            case SearchField::Artist:
                tokenScore = scoreField(artist, tok.text, kWeightArtist);
                break;
            case SearchField::Album:
                tokenScore = scoreField(album, tok.text, kWeightAlbum);
                break;
            case SearchField::Any: {
                int best = 0;
                best = std::max(best, scoreField(title,  tok.text, kWeightTitle));
                best = std::max(best, scoreField(artist, tok.text, kWeightArtist));
                best = std::max(best, scoreField(album,  tok.text, kWeightAlbum));
                tokenScore = best;
                break;
            }
            }
            if (tokenScore == 0) { allMatched = false; break; }
            total += tokenScore;
        }
        if (!allMatched) continue;
        hits.append(Hit{ path, total, curIdx });
    }

    // 高分在前; 同分按 m_libraryOrder 中原序稳定
    std::stable_sort(hits.begin(), hits.end(), [](const Hit& a, const Hit& b) {
        if (a.score != b.score) return a.score > b.score;
        return a.origIdx < b.origIdx;
    });

    if (limit > 0 && hits.size() > limit) hits = hits.mid(0, limit);

    QStringList ordered;
    ordered.reserve(hits.size());
    for (const auto& h : hits) ordered.append(h.path);
    return itemsFromPaths(ordered, -1);
}

// ---- 持久化 ----

void PlayerViewModel::loadSettings()
{
    m_loadingSettings = true;

    QSettings s;
    s.beginGroup("player");
    int vol = s.value("volume", m_volume).toInt();
    bool mut = s.value("muted", m_muted).toBool();
    int  rm  = s.value("repeatMode", m_repeatMode).toInt();
    bool sh  = s.value("shuffle", m_shuffle).toBool();
    m_preferredDeviceId = s.value("deviceId").toString();
    m_visualizerType = s.value("visualizerType", m_visualizerType).toInt();
    s.endGroup();

    setVolume(vol);
    setMuted(mut);
    setRepeatMode(rm);
    setShuffle(sh);
    emit visualizerTypeChanged();

    s.beginGroup("history");
    m_recent = s.value("recent").toStringList();
    if (m_recent.size() > kMaxRecent) m_recent = m_recent.mid(0, kMaxRecent);
    s.endGroup();

    s.beginGroup("library");
    m_liked = s.value("liked").toStringList();
    s.endGroup();

    // EQ
    s.beginGroup("eq");
    m_eq.setEnabled(s.value("enabled", false).toBool());
    for (int i = 0; i < apx::Equalizer::kNumBands; ++i) {
        QString key = QStringLiteral("gain%1").arg(i);
        double v = s.value(key, 0.0).toDouble();
        m_eq.setGain(i, v);
    }
    s.endGroup();

    // 歌单
    m_playlists.clear();
    int n = s.beginReadArray("playlists");
    for (int i = 0; i < n; ++i) {
        s.setArrayIndex(i);
        Playlist pl;
        pl.id    = s.value("id").toString();
        pl.name  = s.value("name").toString();
        pl.paths = s.value("tracks").toStringList();
        if (pl.id.isEmpty()) pl.id = QUuid::createUuid().toString(QUuid::WithoutBraces);
        if (pl.name.isEmpty()) pl.name = QStringLiteral("歌单 %1").arg(i + 1);
        m_playlists.append(pl);
    }
    s.endArray();

    // 重建 library 稳定顺序: recent (最新在前) -> liked -> playlists
    // 持久化层不单独存 library 顺序, 每次启动按此规则一次性重建,
    // 之后运行期由 touchLibrary 维护
    m_libraryOrder.clear();
    for (const auto& p : m_recent) touchLibrary(p);
    for (const auto& p : m_liked)  touchLibrary(p);
    for (const auto& pl : m_playlists)
        for (const auto& p : pl.paths) touchLibrary(p);

    m_loadingSettings = false;
}

void PlayerViewModel::saveSettings() const
{
    if (m_loadingSettings) return;
    QSettings s;
    s.beginGroup("player");
    s.setValue("volume", m_volume);
    s.setValue("muted", m_muted);
    s.setValue("repeatMode", m_repeatMode);
    s.setValue("shuffle", m_shuffle);
    s.setValue("deviceId", m_preferredDeviceId);
    s.setValue("visualizerType", m_visualizerType);
    s.endGroup();

    s.beginGroup("history");
    s.setValue("recent", m_recent);
    s.endGroup();

    s.beginGroup("library");
    s.setValue("liked", m_liked);
    s.endGroup();

    // EQ
    s.beginGroup("eq");
    s.setValue("enabled", m_eq.enabled());
    for (int i = 0; i < apx::Equalizer::kNumBands; ++i) {
        QString key = QStringLiteral("gain%1").arg(i);
        s.setValue(key, m_eq.gain(i));
    }
    s.endGroup();

    // 歌单
    s.remove("playlists");
    s.beginWriteArray("playlists", m_playlists.size());
    for (int i = 0; i < m_playlists.size(); ++i) {
        s.setArrayIndex(i);
        s.setValue("id", m_playlists[i].id);
        s.setValue("name", m_playlists[i].name);
        s.setValue("tracks", m_playlists[i].paths);
    }
    s.endArray();
}

// =============================================================================
// 渲染统计与 Playlist 文件 IO
// =============================================================================

void PlayerViewModel::onStatsTick()
{
    if (!player_) return;
    const auto s = player_->stats();
    bool changed = false;
    if (s.underruns      != m_stats_underruns) { m_stats_underruns = s.underruns;      changed = true; }
    if (s.glitch_frames  != m_stats_glitch)    { m_stats_glitch    = s.glitch_frames;  changed = true; }
    if (s.recovery_count != m_stats_recovery)  { m_stats_recovery  = s.recovery_count; changed = true; }
    if (s.periods_total  != m_stats_periods)   { m_stats_periods   = s.periods_total;  changed = true; }
    if (s.frames_total   != m_stats_frames)    { m_stats_frames    = s.frames_total;   changed = true; }
    if (changed) emit statsUpdated();
}

namespace {

// 把 VM 的 QStringList m_queue 转成 apx::Playlist (附上能在缓存里找到的元数据)
apx::Playlist makePlaylistFromQueue(
    const QStringList& queue, int current,
    const QMap<QString, apx::TrackMetadata>& meta)
{
    apx::Playlist pl;
    for (const QString& p : queue) {
        apx::PlaylistItem it;
        it.path = p.toStdWString();
        auto m = meta.find(p);
        if (m != meta.end()) {
            it.title  = m->title;
            it.artist = m->artist;
            it.album  = m->album;
            it.track_index = static_cast<std::uint32_t>(m->track_no);
            it.duration_sec = m->duration_sec;
        }
        pl.append(std::move(it));
    }
    pl.setCurrentIndex(current);
    return pl;
}

} // namespace

QString PlayerViewModel::exportPlaylistM3U(const QString& path) const
{
    const auto pl = makePlaylistFromQueue(m_queue, m_currentIndex, m_metaCache);
    std::wstring err;
    if (!apx::PlaylistIO::saveM3U(pl, path.toStdWString(), &err)) {
        return QString::fromStdWString(err);
    }
    return {};
}

QString PlayerViewModel::exportPlaylistJson(const QString& path) const
{
    const auto pl = makePlaylistFromQueue(m_queue, m_currentIndex, m_metaCache);
    std::wstring err;
    if (!apx::PlaylistIO::saveJson(pl, path.toStdWString(), &err)) {
        return QString::fromStdWString(err);
    }
    return {};
}

QString PlayerViewModel::importPlaylistM3U(const QString& path)
{
    apx::Playlist pl;
    std::wstring err;
    if (!apx::PlaylistIO::loadM3U(path.toStdWString(), pl, &err)) {
        return QString::fromStdWString(err);
    }
    QStringList paths;
    for (const auto& it : pl.items()) paths << QString::fromStdWString(it.path);
    enqueueMany(paths);
    return {};
}

QString PlayerViewModel::importPlaylistJson(const QString& path)
{
    apx::Playlist pl;
    std::wstring err;
    if (!apx::PlaylistIO::loadJson(path.toStdWString(), pl, &err)) {
        return QString::fromStdWString(err);
    }
    QStringList paths;
    for (const auto& it : pl.items()) paths << QString::fromStdWString(it.path);
    enqueueMany(paths);
    return {};
}

int PlayerViewModel::importCueSheet(const QString& cuePath)
{
    std::wstring err;
    auto tracks = apx::CueSheet::parse(cuePath.toStdWString(), &err);
    if (tracks.empty()) return 0;
    // 把 Cue 元数据塞进 m_metaCache,UI 就能展示标题/艺人
    QStringList paths;
    for (auto& it : tracks) {
        const QString p = QString::fromStdWString(it.path);
        apx::TrackMetadata md;
        md.title  = it.title;
        md.artist = it.artist;
        md.album  = it.album;
        md.track_no = static_cast<int>(it.track_index);
        if (it.cue_end_sec > it.cue_start_sec) {
            md.duration_sec = it.cue_end_sec - it.cue_start_sec;
        }
        m_metaCache[p] = std::move(md);
        paths << p;
    }
    enqueueMany(paths);
    return static_cast<int>(tracks.size());
}

} // namespace apx::ui
