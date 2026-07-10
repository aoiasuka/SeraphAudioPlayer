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

export interface PersistedPlayerState {
  currentTrackIndex: number;
  persistedCurrentTrackId: string | null;
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
}

export interface PlayerStore {
  playlist: Track[];
  currentTrackIndex: number;
  persistedCurrentTrackId: string | null;
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
  deviceMenuOpen: boolean;
  settingsOpen: boolean;
  notification: NotificationPayload | null;
  currentTrack: () => Track | null;
  nextTrackPreview: () => Track | null;
  togglePlayback: () => void;
  nextTrack: () => void;
  prevTrack: () => void;
  loadTrack: (index: number) => void;
  setActiveView: (view: LibraryView) => void;
  seek: (sec: number) => void;
  tick: () => void;
  setVolume: (v: number) => void;
  toggleMute: () => void;
  toggleShuffle: () => void;
  toggleLoop: () => void;
  toggleLike: (trackId: string) => void;
  createUserPlaylist: (name: string) => void;
  deleteUserPlaylist: (playlistId: string) => void;
  deleteTrack: (trackId: string) => Promise<void>;
  loadBackendLibrary: () => Promise<void>;
  importLocalTracks: (paths: string[]) => Promise<void>;
  importBilibiliAudio: (
    input: string,
    options?: BilibiliImportOptions
  ) => Promise<void>;
  importBilibiliFavorites: (
    input: string,
    options?: BilibiliImportOptions
  ) => Promise<BilibiliBatchImportResult | null>;
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

