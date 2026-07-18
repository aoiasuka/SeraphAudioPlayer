import { create } from "zustand";
import { persist } from "zustand/middleware";
import { mockDevices } from "@/data/mock-playlist";
import { createBilibiliActions } from "./player/bilibiliActions";
import { createLibraryActions } from "./player/libraryActions";
import { createLyricsActions } from "./player/lyricsActions";
import { createOutputActions } from "./player/outputActions";
import { createPlaybackActions } from "./player/playbackActions";
import { createPlayerPersistStorage } from "./player/persistStorage";
import { createStreamingActions } from "./player/streamingActions";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./player/types";
import { createUiActions } from "./player/uiActions";
import type { DriverKind, LibraryView } from "@/types/track";

export type {
  BilibiliBatchImportResult,
  BilibiliImportFailure,
  BilibiliImportOptions,
  PersistedPlayerState,
  PlayerStore,
} from "./player/types";

// v2: 新增 persistedCurrentTrackId，用曲目 id（而非索引）恢复上次播放位置
// v3: 新增 rememberPlayback（记忆播放开关，默认开）
const PERSIST_VERSION = 3;
const validDrivers = new Set<DriverKind>(["wasapi", "direct", "asio"]);
const validViews = new Set<LibraryView>([
  "local",
  "streaming",
  "recent",
  "liked",
  "playlists",
  "artists",
  "albums",
  "eq",
]);

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function stringArray(value: unknown) {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function booleanRecord(value: unknown) {
  const record = asRecord(value);
  return Object.fromEntries(
    Object.entries(record).filter((entry): entry is [string, boolean] => (
      typeof entry[1] === "boolean"
    ))
  );
}

function finiteNumber(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function clampVolume(value: unknown, fallback: number) {
  return Math.max(0, Math.min(1, finiteNumber(value, fallback)));
}

function migrateDriver(value: unknown): DriverKind {
  if (value === "usb") return "wasapi";
  if (value === "asio") return "direct";
  return typeof value === "string" && validDrivers.has(value as DriverKind)
    ? value as DriverKind
    : "direct";
}

function migrateView(value: unknown): LibraryView {
  return typeof value === "string" && validViews.has(value as LibraryView)
    ? value as LibraryView
    : "local";
}

export function migratePersistedPlayerState(persistedState: unknown) {
  const state = asRecord(persistedState);
  const volume = clampVolume(state.volume, 0.7);
  return {
    currentTrackIndex: Math.max(0, Math.trunc(finiteNumber(state.currentTrackIndex, 0))),
    persistedCurrentTrackId:
      typeof state.persistedCurrentTrackId === "string" && state.persistedCurrentTrackId
        ? state.persistedCurrentTrackId
        : null,
    persistedCurrentTime: Math.max(0, finiteNumber(state.persistedCurrentTime, 0)),
    recentTrackIds: stringArray(state.recentTrackIds).slice(0, 12),
    volume,
    isMuted: typeof state.isMuted === "boolean" ? state.isMuted : volume === 0,
    previousVolume: clampVolume(state.previousVolume, 0.7),
    shuffleMode: state.shuffleMode === true,
    loopMode: state.loopMode === true,
    liked: booleanRecord(state.liked),
    userPlaylists: Array.isArray(state.userPlaylists) ? state.userPlaylists : [],
    currentDeviceId:
      typeof state.currentDeviceId === "string" && state.currentDeviceId.trim()
        ? state.currentDeviceId
        : "wasapi:hd-dac1",
    driverKind: migrateDriver(state.driverKind),
    activeView: migrateView(state.activeView),
    // 旧版本无此字段时默认启用（与既有行为一致）
    smtcEnabled: state.smtcEnabled !== false,
    // v3：旧版本无此字段时默认启用记忆播放（保持既有“恢复上次播放”的行为）
    rememberPlayback: state.rememberPlayback !== false,
  };
}

export const usePlayerStore = create<PlayerStore>()(
  persist(
    (set, get) => {
      const storeSet = set as PlayerStoreSet;
      const storeGet = get as PlayerStoreGet;

      return {
        playlist: [],
        currentTrackIndex: 0,
        persistedCurrentTrackId: null,
        persistedCurrentTime: 0,
        recentTrackIds: [],
        isPlaying: false,
        currentTime: 0,
        volume: 0.7,
        isMuted: false,
        previousVolume: 0.7,
        shuffleMode: false,
        loopMode: false,
        liked: {},
        userPlaylists: [],
        devices: mockDevices,
        currentDeviceId: "wasapi:hd-dac1",
        // 默认使用系统共享输出（兼容性最高，适合普通扬声器/蓝牙），
        // 已选过其它 driver 的老用户仍从持久化状态恢复自己的选择。
        driverKind: "direct",
        activeView: "local",
        smtcEnabled: true,
        rememberPlayback: true,
        deviceMenuOpen: false,
        settingsOpen: false,
        notification: null,
        // 审2-R5：流媒体页提升到 store 的状态（非持久化，不进 partialize）
        bilibiliLoginStatus: { loggedIn: false },
        bilibiliFfmpegStatus: { available: false },
        ffmpegDownload: { stage: "idle", percent: 0 },
        loginQr: null,
        isLoginBusy: false,

        currentTrack: () => storeGet().playlist[storeGet().currentTrackIndex] ?? null,

        ...createPlaybackActions(storeSet, storeGet),
        ...createUiActions(storeSet, storeGet),
        ...createLibraryActions(storeSet, storeGet),
        ...createBilibiliActions(storeSet, storeGet),
        ...createLyricsActions(storeSet, storeGet),
        ...createOutputActions(storeSet, storeGet),
        ...createStreamingActions(storeSet, storeGet),
      };
    },
    {
      name: "seraph-player-state",
      version: PERSIST_VERSION,
      storage: createPlayerPersistStorage(),
      skipHydration: true,
      migrate: migratePersistedPlayerState,
      partialize: (state) => ({
        currentTrackIndex: state.currentTrackIndex,
        // 发现1：持久化当前曲目 id；playlist 未加载（为空）时回退到已持久化的 id。
        // v3：记忆播放关闭时不写入曲目/位置，磁盘上不留播放痕迹。
        persistedCurrentTrackId: state.rememberPlayback
          ? state.playlist[state.currentTrackIndex]?.id ?? state.persistedCurrentTrackId
          : null,
        // 播放进度按 5 秒粒度持久化：恢复误差 ≤5s，同时把播放中每秒 tick
        // 触发的 localStorage 写频率压到 1/5Hz
        persistedCurrentTime: state.rememberPlayback
          ? Math.floor(Math.max(0, state.currentTime) / 5) * 5
          : 0,
        recentTrackIds: state.recentTrackIds,
        volume: state.volume,
        isMuted: state.isMuted,
        previousVolume: state.previousVolume,
        shuffleMode: state.shuffleMode,
        loopMode: state.loopMode,
        liked: state.liked,
        userPlaylists: state.userPlaylists,
        currentDeviceId: state.currentDeviceId,
        driverKind: state.driverKind,
        activeView: state.activeView,
        smtcEnabled: state.smtcEnabled,
        rememberPlayback: state.rememberPlayback,
      }),
    }
  )
);
