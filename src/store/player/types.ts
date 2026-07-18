import type {
  DriverKind,
  LibraryView,
  LyricLine,
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
}

export interface BilibiliImportFailure {
  input: string;
  reason: string;
}

export interface BilibiliBatchImportResult {
  tracks: Track[];
  failed: BilibiliImportFailure[];
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
}

export interface PlayerStore {
  playlist: Track[];
  currentTrackIndex: number;
  persistedCurrentTrackId: string | null;
  persistedCurrentTime: number;
  recentTrackIds: string[];
  isPlaying: boolean;
  currentTime: number;
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
  deviceMenuOpen: boolean;
  settingsOpen: boolean;
  notification: NotificationPayload | null;
  // 审2-R5：流媒体页提升到 store 的状态（非持久化）
  bilibiliLoginStatus: BilibiliLoginStatus;
  bilibiliFfmpegStatus: BilibiliFfmpegStatus;
  ffmpegDownload: FfmpegDownloadState;
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
  loadBackendLibrary: () => Promise<void>;
  importLocalTracks: (paths: string[]) => Promise<void>;
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
  applyOnlineLyricsForCurrentTrack: (lyrics: LyricLine[]) => Promise<boolean>;
  loadDevices: () => void;
  selectDevice: (id: string) => void;
  setDriver: (k: DriverKind) => void;
  setSmtcEnabled: (enabled: boolean) => void;
  setRememberPlayback: (enabled: boolean) => void;
  toggleDeviceMenu: () => void;
  closeDeviceMenu: () => void;
  toggleSettings: () => void;
  showNotification: (text: string) => void;
  dismissNotification: () => void;
}

export type PlayerStoreSet = (
  partial:
    | PlayerStore
    | Partial<PlayerStore>
    | ((state: PlayerStore) => PlayerStore | Partial<PlayerStore>),
  replace?: false
) => void;

export type PlayerStoreGet = () => PlayerStore;

