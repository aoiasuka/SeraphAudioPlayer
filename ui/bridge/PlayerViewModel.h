// =============================================================================
//  ui/bridge/PlayerViewModel.h
// =============================================================================
#pragma once

#include "app/controller/PlayerController.h"
#include "DeviceBridge.h"
#include "PlaylistViewModel.h"
#include "core/metadata/MetadataReader.h"
#include "core/lyrics/LyricsLoader.h"
#include "core/dsp/Visualizer.h"
#include "core/dsp/Equalizer.h"
#include "platform/smtc/SmtcController.h"
#include "platform/taskbar/TaskbarButtons.h"
#include <QObject>
#include <QString>
#include <QStringList>
#include <QList>
#include <QMap>
#include <QVariantList>
#include <QColor>
#include <QTimer>
#include <memory>

namespace apx::ui {

class PlayerViewModel : public QObject {
    Q_OBJECT
    Q_PROPERTY(int state READ state NOTIFY stateChanged)
    Q_PROPERTY(double position READ position NOTIFY positionChanged)
    Q_PROPERTY(double duration READ duration NOTIFY durationChanged)
    Q_PROPERTY(QString title READ title NOTIFY titleChanged)
    Q_PROPERTY(QString formatInfo READ formatInfo NOTIFY formatInfoChanged)
    Q_PROPERTY(QString coverImage READ coverImage NOTIFY coverImageChanged)
    Q_PROPERTY(QString lastError READ lastError NOTIFY errorOccurred)

    // 音量 0..100
    Q_PROPERTY(int volume READ volume WRITE setVolume NOTIFY volumeChanged)
    Q_PROPERTY(bool muted READ muted WRITE setMuted NOTIFY mutedChanged)

    // 0 = off, 1 = repeat-all, 2 = repeat-one
    Q_PROPERTY(int repeatMode READ repeatMode WRITE setRepeatMode NOTIFY repeatModeChanged)
    Q_PROPERTY(bool shuffle READ shuffle WRITE setShuffle NOTIFY shuffleChanged)

    // 当前播放队列(原始顺序)与最近播放
    Q_PROPERTY(QVariantList queue READ queue NOTIFY queueChanged)
    // QAbstractListModel 形式的同一队列;新 PlaylistView 通过它绑定 ListView,避免 QVariantList 全量重发
    Q_PROPERTY(apx::ui::PlaylistViewModel* playlistModel READ playlistModel CONSTANT)
    Q_PROPERTY(int currentIndex READ currentIndex NOTIFY currentIndexChanged)
    Q_PROPERTY(QVariantList recent READ recent NOTIFY recentChanged)
    // 喜欢的曲目
    Q_PROPERTY(QVariantList liked READ liked NOTIFY likedChanged)
    // 当前是否被喜欢
    Q_PROPERTY(bool currentLiked READ currentLiked NOTIFY currentLikedChanged)
    // 全部本机已知曲目(去重并合并 queue/recent/liked)
    Q_PROPERTY(QVariantList library READ library NOTIFY libraryChanged)

    // 用户歌单 [{id, name, count}]
    Q_PROPERTY(QVariantList playlists READ playlists NOTIFY playlistsChanged)

    // 派生集合 — 跟 library 同步变更
    Q_PROPERTY(QVariantList artists READ artists NOTIFY libraryChanged)
    Q_PROPERTY(QVariantList albums  READ albums  NOTIFY libraryChanged)

    // 当前曲目封面 URL (image://covers/<encoded path>);未播放时为空
    Q_PROPERTY(QString currentCoverUrl READ currentCoverUrl NOTIFY currentCoverUrlChanged)
    // 当前曲目封面主色 (HSV 调整后);未播放时返回品牌默认色
    Q_PROPERTY(QColor currentDominantColor READ currentDominantColor NOTIFY currentCoverUrlChanged)

    // 当前曲目的歌词 ([{time,text,translation}, ...]) 与高亮索引
    Q_PROPERTY(QVariantList currentLyrics READ currentLyrics NOTIFY lyricsChanged)
    Q_PROPERTY(int currentLyricIndex READ currentLyricIndex NOTIFY currentLyricIndexChanged)
    Q_PROPERTY(bool hasLyrics READ hasLyrics NOTIFY lyricsChanged)
    // 歌词元数据 {title, artist, album, by, offset_ms, length_sec, source}
    // source: "" 无, "external" 同目录 lrc, "manual" 用户手动加载, "embedded" 内嵌
    Q_PROPERTY(QVariantMap lyricsMeta READ lyricsMeta NOTIFY lyricsChanged)

    // 可视化
    Q_PROPERTY(double vuLeft  READ vuLeft  NOTIFY visualUpdated)
    Q_PROPERTY(double vuRight READ vuRight NOTIFY visualUpdated)
    Q_PROPERTY(double peakLeft  READ peakLeft  NOTIFY visualUpdated)
    Q_PROPERTY(double peakRight READ peakRight NOTIFY visualUpdated)
    Q_PROPERTY(QVariantList spectrum READ spectrum NOTIFY visualUpdated)
    Q_PROPERTY(int visualizerType READ visualizerType WRITE setVisualizerType NOTIFY visualizerTypeChanged)

    // EQ
    Q_PROPERTY(bool eqEnabled READ eqEnabled WRITE setEqEnabled NOTIFY eqChanged)
    Q_PROPERTY(QVariantList eqGains READ eqGains NOTIFY eqChanged)

    // 设备列表
    Q_PROPERTY(QVariantList devices READ devices NOTIFY devicesListChanged)
    Q_PROPERTY(QString currentDeviceId READ currentDeviceId NOTIFY currentDeviceChanged)
    Q_PROPERTY(QString currentDeviceName READ currentDeviceNameProp NOTIFY currentDeviceChanged)

    // 渲染统计 (1Hz 刷新);UI 在诊断面板里显示
    Q_PROPERTY(quint64 statsUnderruns     READ statsUnderruns     NOTIFY statsUpdated)
    Q_PROPERTY(quint64 statsGlitchFrames  READ statsGlitchFrames  NOTIFY statsUpdated)
    Q_PROPERTY(quint64 statsRecoveryCount READ statsRecoveryCount NOTIFY statsUpdated)
    Q_PROPERTY(quint64 statsPeriodsTotal  READ statsPeriodsTotal  NOTIFY statsUpdated)
    Q_PROPERTY(quint64 statsFramesTotal   READ statsFramesTotal   NOTIFY statsUpdated)

    // ---- 高级 Hi-Fi 设置 ----
    // ReplayGain: 0=Off / 1=Track / 2=Album, preamp 区间 -12..+12 dB
    Q_PROPERTY(int    replayGainMode      READ replayGainMode      WRITE setReplayGainMode      NOTIFY replayGainChanged)
    Q_PROPERTY(double replayGainPreampDb  READ replayGainPreampDb  WRITE setReplayGainPreampDb  NOTIFY replayGainChanged)
    // 独占模式失败时是否允许回退到共享模式 (开 = "至少能听见", 关 = "Hi-Fi 严格")
    Q_PROPERTY(bool   allowSharedFallback READ allowSharedFallback WRITE setAllowSharedFallback NOTIFY outputPolicyChanged)
    // 重采样运行时实际选中的 SIMD 路径 ("avx2" / "sse2" / "scalar"). 只读, 启动后不变.
    Q_PROPERTY(QString simdPath            READ simdPath           CONSTANT)
    // Int16 输出量化 dither (TPDF + noise shaping). 持久化, 影响 WasapiSharedOutput
    // 下一次格式协商后生效.
    Q_PROPERTY(bool   dither              READ dither              WRITE setDither              NOTIFY ditherChanged)
    // DSD over PCM marker 模式: 0=PerFrame (DoP 标准, 0x05/0xFA), 1=PerSample
    Q_PROPERTY(int    dopMarkerMode       READ dopMarkerMode       WRITE setDopMarkerMode       NOTIFY dopMarkerModeChanged)
    // DSD 输出模式: 0=ForceDoP (默认) / 1=ForceNative / 2=Auto
    // ForceNative/Auto 时 decoder 输出 raw LSB8, WASAPI 协商 SUBTYPE_DSD;
    // 实际可用度取决于 DAC 在 WASAPI 端点是否暴露 DSD format.
    Q_PROPERTY(int    dsdMode             READ dsdMode             WRITE setDsdMode             NOTIFY dsdModeChanged)

public:
    explicit PlayerViewModel(QObject* parent = nullptr);
    ~PlayerViewModel() override;

    int state() const { return m_state; }
    double position() const { return m_position; }
    double duration() const { return m_duration; }
    QString title() const { return m_title; }
    QString formatInfo() const { return m_formatInfo; }
    QString coverImage() const { return m_coverImage; }
    QString lastError() const { return m_lastError; }

    int volume() const { return m_volume; }
    void setVolume(int v);
    bool muted() const { return m_muted; }
    void setMuted(bool b);

    int repeatMode() const { return m_repeatMode; }
    void setRepeatMode(int m);
    bool shuffle() const { return m_shuffle; }
    void setShuffle(bool s);

    QVariantList queue() const;
    PlaylistViewModel* playlistModel() const { return m_playlistModel.get(); }
    int currentIndex() const { return m_currentIndex; }
    QVariantList recent() const;
    QVariantList liked() const;
    QVariantList library() const;
    QVariantList playlists() const;
    QVariantList artists() const;
    QVariantList albums() const;
    bool currentLiked() const;
    QString currentCoverUrl() const;
    QColor  currentDominantColor() const;

    QVariantList currentLyrics() const;
    int          currentLyricIndex() const { return m_lyricIndex; }
    bool         hasLyrics() const { return !m_lyrics.empty(); }
    QVariantMap  lyricsMeta() const;
    // 用户手动加载外部歌词(覆盖当前曲目)
    Q_INVOKABLE bool loadExternalLyrics(const QString& path);
    // 重新从磁盘扫一遍(用户刚把 .lrc 放进文件夹)
    Q_INVOKABLE void refreshLyrics();
    // 清空当前歌词
    Q_INVOKABLE void clearLyrics();

    double vuLeft()   const { return m_vu_l; }
    double vuRight()  const { return m_vu_r; }
    double peakLeft() const { return m_peak_l; }
    double peakRight()const { return m_peak_r; }
    QVariantList spectrum() const;
    int visualizerType() const { return m_visualizerType; }
    void setVisualizerType(int type);

    bool eqEnabled() const { return m_eq.enabled(); }
    void setEqEnabled(bool on);
    QVariantList eqGains() const;
    Q_INVOKABLE void setEqGain(int band, double db);
    Q_INVOKABLE void resetEq();          // 所有 gain 归零

    QVariantList devices() const;
    QString currentDeviceId() const { return m_currentDeviceId; }
    QString currentDeviceNameProp() const;

    quint64 statsUnderruns()     const { return m_stats_underruns; }
    quint64 statsGlitchFrames()  const { return m_stats_glitch; }
    quint64 statsRecoveryCount() const { return m_stats_recovery; }
    quint64 statsPeriodsTotal()  const { return m_stats_periods; }
    quint64 statsFramesTotal()   const { return m_stats_frames; }

    // ---- 高级 Hi-Fi 设置 ----
    int    replayGainMode()      const { return m_rg_mode; }
    void   setReplayGainMode(int m);
    double replayGainPreampDb()  const { return m_rg_preamp_db; }
    void   setReplayGainPreampDb(double db);
    bool   allowSharedFallback() const { return m_allow_shared_fallback; }
    void   setAllowSharedFallback(bool on);
    QString simdPath() const;
    bool   dither()              const { return m_dither; }
    void   setDither(bool on);
    int    dopMarkerMode()       const { return m_dop_marker_mode; }
    void   setDopMarkerMode(int m);
    int    dsdMode()             const { return m_dsd_mode; }
    void   setDsdMode(int m);

    // ---- 播放控制 ----
    Q_INVOKABLE void play();
    Q_INVOKABLE void pause();
    Q_INVOKABLE void stop();
    Q_INVOKABLE void seek(double sec);

    // ---- 队列 ----
    // openFile = 清空队列后加入并播放
    Q_INVOKABLE void openFile(const QString& path);
    // 追加到队列尾部(不打断当前播放)。如果队列为空则会自动加载并播放。
    Q_INVOKABLE void enqueue(const QString& path);
    Q_INVOKABLE void enqueueMany(const QStringList& paths);
    Q_INVOKABLE void playIndex(int index);
    Q_INVOKABLE void next();
    Q_INVOKABLE void previous();
    Q_INVOKABLE void clearQueue();
    Q_INVOKABLE void removeAt(int index);
    Q_INVOKABLE void moveQueueItem(int from, int to);

    // ---- 喜欢 ----
    Q_INVOKABLE bool isLiked(const QString& path) const;
    Q_INVOKABLE void toggleLike(const QString& path);
    Q_INVOKABLE void toggleLikeCurrent();
    Q_INVOKABLE void removeFromLiked(const QString& path);

    // ---- 歌单 ----
    Q_INVOKABLE QString createPlaylist(const QString& name);
    Q_INVOKABLE void    renamePlaylist(const QString& id, const QString& name);
    Q_INVOKABLE void    deletePlaylist(const QString& id);
    Q_INVOKABLE void    addToPlaylist(const QString& id, const QString& path);
    Q_INVOKABLE void    addManyToPlaylist(const QString& id, const QStringList& paths);
    Q_INVOKABLE void    removeFromPlaylist(const QString& id, const QString& path);
    Q_INVOKABLE void    movePlaylistItem(const QString& id, int from, int to);
    Q_INVOKABLE void    playPlaylist(const QString& id);
    Q_INVOKABLE void    enqueuePlaylist(const QString& id);
    // 返回 [{path,title,artist,album,...}]
    Q_INVOKABLE QVariantList playlistTracks(const QString& id) const;
    // 返回 {id,name,count} 或空 map
    Q_INVOKABLE QVariantMap  playlistById(const QString& id) const;

    // ---- 歌手/专辑聚合 ----
    Q_INVOKABLE QVariantList tracksByArtist(const QString& artist) const;
    Q_INVOKABLE QVariantList tracksByAlbum(const QString& album, const QString& artist = QString()) const;
    Q_INVOKABLE void playArtist(const QString& artist);
    Q_INVOKABLE void playAlbum(const QString& album, const QString& artist = QString());

    // ---- 全库搜索 ----
    // 在 title / artist / album 上做加权打分搜索, 与 QML 端 SearchUtil.js 算法一致:
    //   1. 空格分词 (全部 token 需 AND 命中, 任一字段 OR)
    //   2. 字段前缀: "artist:xxx" "album:xxx" "title:xxx" + 简写 ar:/al:/t: + 中文 歌手:/专辑:
    //   3. 字段权重: title=10, artist=5, album=3
    //   4. 命中字段开头 ×2 加成
    //   5. 大小写不敏感
    // limit ≤ 0 表示不限
    Q_INVOKABLE QVariantList searchTracks(const QString& query, int limit = 200) const;

    // ---- 最近播放管理 ----
    Q_INVOKABLE void removeFromRecent(const QString& path);
    Q_INVOKABLE void clearRecent();

    // 切换模式快捷方法
    Q_INVOKABLE void toggleShuffle() { setShuffle(!m_shuffle); }
    Q_INVOKABLE void cycleRepeatMode() { setRepeatMode((m_repeatMode + 1) % 3); }
    Q_INVOKABLE void toggleMute() { setMuted(!m_muted); }

    Q_INVOKABLE void setDevice(const QString& deviceId);
    Q_INVOKABLE void refreshDevices();

    // ---- Playlist 文件导入导出 (基于 PlaylistIO,使用当前 m_queue) ----
    // 成功返回空字符串;失败返回错误消息
    Q_INVOKABLE QString exportPlaylistM3U(const QString& path) const;
    Q_INVOKABLE QString importPlaylistM3U(const QString& path);
    Q_INVOKABLE QString exportPlaylistJson(const QString& path) const;
    Q_INVOKABLE QString importPlaylistJson(const QString& path);

    // 拖入 .cue 文件时调用,展开为多条目并加入队列。返回拆出的条目数
    Q_INVOKABLE int     importCueSheet(const QString& cuePath);

    // 在窗口创建之后由 main.cpp 调用,把 HWND 绑给 SMTC / 任务栏按钮
    void attachWindow(void* hwnd);

signals:
    void stateChanged();
    void positionChanged();
    void durationChanged();
    void titleChanged();
    void formatInfoChanged();
    void coverImageChanged();
    void errorOccurred(const QString& msg);
    void devicesChanged();    // 设备列表底层变化(由 DeviceBridge 转发)
    void devicesListChanged();
    void currentDeviceChanged();

    void volumeChanged();
    void mutedChanged();
    void repeatModeChanged();
    void shuffleChanged();

    void queueChanged();
    void currentIndexChanged();
    void recentChanged();
    void likedChanged();
    void currentLikedChanged();
    void libraryChanged();
    void playlistsChanged();
    void currentCoverUrlChanged();
    void lyricsChanged();
    void currentLyricIndexChanged();
    void visualUpdated();
    void visualizerTypeChanged();
    void eqChanged();
    void statsUpdated();
    void replayGainChanged();
    void outputPolicyChanged();
    void ditherChanged();
    void dopMarkerModeChanged();
    void dsdModeChanged();

    // 跨线程内部信号
    void _coreStateChanged(int s);
    void _corePositionChanged(double sec);
    void _coreEnded();
    void _coreError(const QString& msg);

private slots:
    void onCoreStateChanged(int s);
    void onCorePositionChanged(double sec);
    void onCoreEnded();
    void onCoreError(const QString& msg);
    void onDevicesChanged();

private:
    std::unique_ptr<PlayerController> player_;
    std::unique_ptr<DeviceBridge> device_bridge_;
    std::unique_ptr<PlaylistViewModel> m_playlistModel;
    std::unique_ptr<apx::SmtcController> smtc_;
    std::unique_ptr<apx::TaskbarButtons> taskbar_;
    void* m_hwnd = nullptr;

    void syncSmtcMetadata();
    void syncSmtcStatus();
    void syncSmtcTimeline();
    void syncTaskbar();
public:
    // 给 native event filter 用
    apx::TaskbarButtons* taskbarButtons() { return taskbar_.get(); }
private:

    int m_state = 0; // PlayerState::Idle
    double m_position = 0.0;
    double m_duration = 0.0;
    QString m_title = "未播放";
    QString m_formatInfo = "";
    QString m_coverImage = "";
    QString m_lastError = "";

    // 音量(0..100) / 静音
    int m_volume = 70;
    bool m_muted = false;

    // 模式
    int m_repeatMode = 0;
    bool m_shuffle = false;

    // 队列
    QStringList m_queue;
    int m_currentIndex = -1;
    QList<int> m_shuffleOrder;      // shuffle 下的播放顺序(对 m_queue 的索引)
    int m_shufflePos = -1;

    // 最近播放(最新在前)
    QStringList m_recent;
    static constexpr int kMaxRecent = 50;

    // 喜欢的曲目
    QStringList m_liked;

    // 用户歌单
    struct Playlist {
        QString id;
        QString name;
        QStringList paths;
    };
    QList<Playlist> m_playlists;

    // 音乐库稳定顺序: 按"首次进入 ViewModel"顺序保留所有已知 path
    // library() 输出顺序基于此, 避免播放当前曲目变化引起列表抖动
    QStringList m_libraryOrder;

    // 设备
    QVariantList m_devicesCache;
    QString m_currentDeviceId;

    // 持久化
    QString m_preferredDeviceId;     // 启动时记住的设备
    bool m_loadingSettings = false;  // 加载阶段抑制写回

    // 元数据缓存:path -> (title/artist/album/date/track_no)
    mutable QMap<QString, apx::TrackMetadata> m_metaCache;
    // 已尝试过但没拿到元数据的 path 黑名单(避免重复磁盘 IO)
    mutable QStringList m_metaMissed;
    // 主色缓存
    mutable QMap<QString, QColor> m_colorCache;

    // 可视化
    apx::Visualizer m_visualizer;
    QTimer*         m_visTimer = nullptr;
    double          m_vu_l = 0, m_vu_r = 0, m_peak_l = 0, m_peak_r = 0;
    QVariantList    m_spectrum;
    int             m_visualizerType = 0;
    void            onVisTick();

    // EQ
    apx::Equalizer  m_eq;

    // 渲染统计 (1Hz 刷新)
    quint64 m_stats_underruns = 0;
    quint64 m_stats_glitch    = 0;
    quint64 m_stats_recovery  = 0;
    quint64 m_stats_periods   = 0;
    quint64 m_stats_frames    = 0;
    QTimer* m_statsTimer      = nullptr;
    void    onStatsTick();

    // 高级 Hi-Fi 设置 (持久化到 QSettings)
    int    m_rg_mode               = 0;   // 0=off, 1=track, 2=album
    double m_rg_preamp_db          = 0.0;
    bool   m_allow_shared_fallback = true;
    bool   m_dither                = true;
    int    m_dop_marker_mode       = 0;   // 0=PerFrame, 1=PerSample
    int    m_dsd_mode              = 0;   // 0=ForceDoP, 1=ForceNative, 2=Auto
    void   applyReplayGainToPlayer();
    void   applyOutputPolicyToPlayer();

    // 歌词
    std::vector<apx::LyricLine> m_lyrics;
    apx::LyricMetadata          m_lyricsMeta;
    QString                     m_lyricsSource;     // "" / "external" / "manual" / "embedded"
    int m_lyricIndex = -1;
    QString m_lyricsForPath;          // 当前歌词来自哪条 path,用于变更检测

    void reloadLyricsForCurrent();
    void updateLyricIndex(double pos);

    // 拿元数据(缓存命中直接返回);返回 true 表示拿到了非空 metadata
    bool fetchMeta(const QString& path, apx::TrackMetadata& out) const;

    void applyVolumeToPlayer();
    void updateFileInfo();
    bool loadAndPlay(int index);
    void rebuildShuffleOrder(int startIndex);
    int  nextIndexAfter(int currentIndex) const; // 返回下一索引,-1 表示无
    void pushRecent(const QString& path);
    QVariantList itemsFromPaths(const QStringList& paths, int currentIdx = -1) const;

    // 维护 m_libraryOrder: 把首次见到的 path 追加到末尾, 已有则不动
    void touchLibrary(const QString& path);
    void touchLibraryMany(const QStringList& paths);

    // settings
    void loadSettings();
    void saveSettings() const;
    QString currentPath() const;
};

} // namespace apx::ui
