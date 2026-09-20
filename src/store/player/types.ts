import type {
  DeleteTracksResult,
  DriverKind,
  LibraryView,
  LrcExportFormat,
  LyricDocument,
  LyricsExcludeRule,
  LyricsSourcePriority,
  OnlineLyricsCandidate,
  OutputDevice,
  Track,
  UserPlaylist,
} from "@/types/track";

export interface NotificationPayload {
  id: number;
  text: string;
}

export interface BackendDevice {
  id: string;
  name: string;
  is_default?: boolean;
  isDefault?: boolean;
  legacyIds?: string[];
  legacy_ids?: string[];
}

export interface BilibiliImportOptions {
  preferFlac: boolean;
  preferDolbyAtmos: boolean;
  remuxWithFfmpeg: boolean;
  /** 重缓存只更新仍存在的曲目，不能把下载期间删除的记录重新插入。 */
  existingTrackId?: string;
}

export interface BilibiliImportFailure {
  input: string;
  reason: string;
}

export interface BilibiliBatchImportResult {
  tracks: Track[];
  failed: BilibiliImportFailure[];
  /** 用户中途取消（已导入部分照常返回） */
  cancelled?: boolean;
}

/** 收藏夹批量导入进度（`seraph://bilibili-batch` 事件 payload） */
export interface BilibiliBatchProgress {
  taskId?: string;
  current: number;
  total: number;
  title: string;
  ok: boolean;
}

// 审2-R5：以下流媒体页状态从 StreamingPage 组件提升到 store（非持久化，不进 partialize），
// MainPages 用 key={activeView} 强制卸载页面，组件局部状态切页即丢：
// ffmpeg 下载进度会被重复触发、B 站扫码登录轮询会静默中断。
export interface BilibiliLoginStatus {
  loggedIn: boolean;
  username?: string | null;
  mid?: number | null;
  face?: string | null;
}

export interface BilibiliLoginQrCode {
  url: string;
  qrcodeKey: string;
}

export interface BilibiliLoginPollResult {
  code: number;
  message: string;
  loggedIn: boolean;
  profile?: BilibiliLoginStatus | null;
}

export interface BilibiliFfmpegStatus {
  available: boolean;
  path?: string | null;
}

// 后端 "seraph://ffmpeg-download" 事件载荷
export interface FfmpegDownloadProgress {
  stage: "download" | "extract" | "done" | "error";
  downloaded: number;
  total: number;
  percent: number;
  message?: string | null;
}

// store 内的下载状态机
export interface FfmpegDownloadState {
  stage: "idle" | "downloading" | "done" | "error";
  percent: number;
  message?: string;
}

// B 站扫码登录二维码状态；轮询 interval 本身是不可序列化对象，存 streamingActions 模块级变量
export interface BilibiliLoginQrState {
  qrcodeKey: string;
  url: string;
  dataUrl: string;
  message: string;
}

export interface PersistedPlayerState {
  currentTrackIndex: number;
  persistedCurrentTrackId: string | null;
  persistedCurrentTime: number;
  recentTrackIds: string[];
  volume: number;
  isMuted: boolean;
  previousVolume: number;
  shuffleMode: boolean;
  loopMode: boolean;
  liked: Record<string, boolean>;
  userPlaylists: UserPlaylist[];
  currentDeviceId: string;
  driverKind: DriverKind;
  activeView: LibraryView;
  smtcEnabled: boolean;
  // v0.4.2：记忆播放。关闭时启动不恢复上次曲目/位置，且不持久化播放进度。
  rememberPlayback: boolean;
  // v0.5.4：任务栏集成（缩略图播控按钮 / 图标播放进度条）
  taskbarButtonsEnabled: boolean;
  taskbarProgressEnabled: boolean;
  // v0.5.4：任务栏歌词条（第二窗口，默认关闭）
  taskbarLyricsEnabled: boolean;
  // v0.5.4：歌词条仅歌词模式（鼠标穿透，默认关闭）
  taskbarLyricsClickThrough: boolean;
  // v0.5.5：歌词条沿任务栏长边的位置比例（0 = 最靠起点，1 = 最靠终点）
  taskbarLyricsPosition: number;
  // v0.6.0：歌词设置
  lyricsSourcePriority: LyricsSourcePriority;
  preferTraditionalLyrics: boolean;
  ttmlLyricsEnabled: boolean;
  amllTtmlDbUrl: string;
  /** true = 自定义地址模式（任意公网 https，模板占位）；false = 预设镜像白名单 */
  amllTtmlDbCustom: boolean;
  lyricsExcludeRules: LyricsExcludeRule[];
  showLyricsTranslation: boolean;
  showLyricsRoman: boolean;
  /** 本地歌词目录（绝对路径，空串 = 未设置）；切歌时按「艺术家 - 曲名」自动匹配 */
  lyricsFolder: string;
  /** B2：显示制作信息行（作词 / 作曲…）；关闭后由后端按 hidden 打标，主窗口与任务栏一致 */
  showLyricsCredits: boolean;
  /** B2：忽略歌词文件里的 `[offset:]` 标签（后端回传前还原解析时折进去的偏移） */
  ignoreLyricsFileOffset: boolean;
}

export interface PlaybackQueuePreview {
  currentTrackId: string | null;
  nextTrackId: string | null;
  shuffleMode: boolean;
}

export interface PlayerStore {
  playlist: Track[];
  /** 后端保留的下一首，仅用于运行时预览，不持久化。 */
  playbackQueuePreview: PlaybackQueuePreview | null;
  currentTrackIndex: number;
  persistedCurrentTrackId: string | null;
  persistedCurrentTime: number;
  recentTrackIds: string[];
  isPlaying: boolean;
  currentTime: number;
  /** 引擎输出延迟（秒，Progress 事件携带）：歌词定位用 currentTime − outputLatency。不持久化。 */
  outputLatency: number;
  volume: number;
  isMuted: boolean;
  previousVolume: number;
  shuffleMode: boolean;
  loopMode: boolean;
  liked: Record<string, boolean>;
  userPlaylists: UserPlaylist[];
  devices: OutputDevice[];
  currentDeviceId: string;
  driverKind: DriverKind;
  activeView: LibraryView;
  smtcEnabled: boolean;
  rememberPlayback: boolean;
  taskbarButtonsEnabled: boolean;
  taskbarProgressEnabled: boolean;
  taskbarLyricsEnabled: boolean;
  taskbarLyricsClickThrough: boolean;
  taskbarLyricsPosition: number;
  lyricsSourcePriority: LyricsSourcePriority;
  preferTraditionalLyrics: boolean;
  ttmlLyricsEnabled: boolean;
  amllTtmlDbUrl: string;
  amllTtmlDbCustom: boolean;
  lyricsExcludeRules: LyricsExcludeRule[];
  showLyricsTranslation: boolean;
  showLyricsRoman: boolean;
  lyricsFolder: string;
  showLyricsCredits: boolean;
  ignoreLyricsFileOffset: boolean;
  deviceMenuOpen: boolean;
  settingsOpen: boolean;
  notification: NotificationPayload | null;
  // 审2-R5：流媒体页提升到 store 的状态（非持久化）
  bilibiliLoginStatus: BilibiliLoginStatus;
  bilibiliFfmpegStatus: BilibiliFfmpegStatus;
  ffmpegDownload: FfmpegDownloadState;
  /** 收藏夹批量导入进度（导入中非 null；App 级事件监听写入，切页不丢） */
  bilibiliBatchProgress: BilibiliBatchProgress | null;
  loginQr: BilibiliLoginQrState | null;
  isLoginBusy: boolean;
  currentTrack: () => Track | null;
  nextTrackPreview: () => Track | null;
  playNextPreview: () => void;
  togglePlayback: () => void;
  nextTrack: () => void;
  prevTrack: () => void;
  loadTrack: (index: number, options?: { forcePlay?: boolean }) => void;
  setActiveView: (view: LibraryView) => void;
  seek: (sec: number) => void;
  tick: () => void;
  setVolume: (v: number) => void;
  toggleMute: () => void;
  toggleShuffle: () => void;
  toggleLoop: () => void;
  toggleLike: (trackId: string) => void;
  createUserPlaylist: (name: string) => string | null;
  renameUserPlaylist: (playlistId: string, name: string) => void;
  deleteUserPlaylist: (playlistId: string) => void;
  deleteTrack: (trackId: string) => Promise<void>;
  deleteTracks: (trackIds: string[]) => Promise<DeleteTracksResult>;
  loadBackendLibrary: () => Promise<void>;
  importLocalTracks: (
    paths: string[],
    options?: { keepView?: boolean }
  ) => Promise<void>;
  fetchOnlineCoverForCurrentTrack: () => Promise<boolean>;
  addTrackToUserPlaylist: (playlistId: string, trackId: string) => void;
  removeTrackFromUserPlaylist: (playlistId: string, trackId: string) => void;
  moveTrackInUserPlaylist: (
    playlistId: string,
    trackId: string,
    direction: "up" | "down"
  ) => void;
  importPlaylistFromM3u8: () => Promise<void>;
  exportUserPlaylistToM3u8: (playlistId: string) => Promise<void>;
  importBilibiliAudio: (
    input: string,
    options?: BilibiliImportOptions
  ) => Promise<boolean>;
  importBilibiliFavorites: (
    input: string,
    options?: BilibiliImportOptions
  ) => Promise<BilibiliBatchImportResult | null>;
  /** 请求中断进行中的收藏夹批量导入（当前一首完成后停止） */
  cancelBilibiliFavoritesImport: () => Promise<void>;
  // v0.4.4：按当次勾选的音质选项重新加载 B 站流媒体曲目，原位替换。
  reloadStreamingTrack: (
    trackId: string,
    options?: BilibiliImportOptions
  ) => Promise<boolean>;
  // 审2-R5：流媒体页 actions（生命周期归 store 管，组件卸载不清理）
  refreshBilibiliState: () => Promise<void>;
  startFfmpegDownload: () => Promise<void>;
  startLoginPolling: () => Promise<void>;
  stopLoginPolling: () => void;
  logoutBilibili: () => Promise<void>;
  markTracksCacheMissingByPaths: (paths: string[]) => void;
  normalizeLibrary: () => void;
  importLyricsForCurrentTrack: (file: File) => Promise<void>;
  fetchOnlineLyricsForCurrentTrack: (
    query?: string
  ) => Promise<OnlineLyricsCandidate[]>;
  applyOnlineLyricsForCurrentTrack: (
    lyrics: LyricDocument,
    lookupKeys?: string[]
  ) => Promise<boolean>;
  /** 弹出保存对话框，把当前曲目歌词导出为 LRC（增强型 / 逐字 / 逐行）；成功返回 true */
  exportLyricsForCurrentTrack: (format: LrcExportFormat) => Promise<boolean>;
  loadDevices: () => Promise<void>;
  selectDevice: (id: string) => void;
  setDriver: (k: DriverKind) => void;
  setSmtcEnabled: (enabled: boolean) => void;
  setRememberPlayback: (enabled: boolean) => void;
  setTaskbarButtonsEnabled: (enabled: boolean) => void;
  setTaskbarProgressEnabled: (enabled: boolean) => void;
  setTaskbarLyricsEnabled: (enabled: boolean) => void;
  setTaskbarLyricsClickThrough: (enabled: boolean) => void;
  /** 歌词条横向（垂直任务栏时为纵向）位置比例，0..=1 */
  setTaskbarLyricsPosition: (ratio: number) => void;
  setLyricsSourcePriority: (priority: LyricsSourcePriority) => void;
  setPreferTraditionalLyrics: (enabled: boolean) => void;
  setTtmlLyricsEnabled: (enabled: boolean) => void;
  /** 按模式校验；非法地址返回 false 且不写入。同时写入模式位。 */
  setAmllTtmlDbUrl: (url: string, custom: boolean) => boolean;
  setLyricsExcludeRules: (rules: LyricsExcludeRule[]) => void;
  setShowLyricsTranslation: (enabled: boolean) => void;
  setShowLyricsRoman: (enabled: boolean) => void;
  setLyricsFolder: (folder: string) => void;
  /** 显示制作信息行；经 set_lyrics_display_options 同步后端（后端广播重拉） */
  setShowLyricsCredits: (enabled: boolean) => void;
  /** 忽略歌词文件里的 [offset:]；同上 */
  setIgnoreLyricsFileOffset: (enabled: boolean) => void;
  /** 切换当前曲目歌词的「固定」标记（固定 = 自动流程不替换）；成功返回 true */
  setCurrentTrackLyricsPinned: (pinned: boolean) => Promise<boolean>;
  /** 把 11 个歌词设置字段恢复默认；排除规则经 setLyricsExcludeRules 清空、显示选项经 IPC 同步后端。 */
  resetLyricsSettings: () => void;
  /** 在本地歌词目录里匹配并写入该曲目的歌词；命中返回 true */
  findLocalLyricsForTrack: (track: Track) => Promise<boolean>;
  toggleDeviceMenu: () => void;
  closeDeviceMenu: () => void;
  toggleSettings: () => void;
  showNotification: (text: string) => void;
  dismissNotification: () => void;
}

/** `test_amll_ttml_db` 命令的结果分类。 */
export type AmllTtmlDbTestKind =
  | "invalid_url"
  | "unreachable"
  | "not_found"
  | "http_error"
  | "html"
  | "invalid_xml"
  | "unsupported_ttml"
  | "ok";

/** `test_amll_ttml_db({ template, custom, sampleKey? })` 的返回值。 */
export interface AmllTtmlDbTestResult {
  ok: boolean;
  kind: AmllTtmlDbTestKind;
  message: string;
  url: string;
  lines: number;
}

export type PlayerStoreSet = (
  partial:
    | PlayerStore
    | Partial<PlayerStore>
    | ((state: PlayerStore) => PlayerStore | Partial<PlayerStore>),
  replace?: false
) => void;

export type PlayerStoreGet = () => PlayerStore;
