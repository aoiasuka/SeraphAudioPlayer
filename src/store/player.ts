import { create } from "zustand";
import { persist } from "zustand/middleware";
import { mockDevices } from "@/data/mock-playlist";
import { createBilibiliActions } from "./player/bilibiliActions";
import { createLibraryActions } from "./player/libraryActions";
import { createLyricsActions } from "./player/lyricsActions";
import { createOutputActions } from "./player/outputActions";
import { createPlaybackActions } from "./player/playbackActions";
import { createPlayerPersistStorage } from "./player/persistStorage";
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
const PERSIST_VERSION = 2;
const validDrivers = new Set<DriverKind>(["wasapi", "direct", "asio"]);
const validViews = new Set<LibraryView>([
  "local",
  "streaming",
  "recent",
  "liked",
  "playlists",
  "artists",
  "albums",
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
    : "wasapi";
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
        driverKind: "wasapi",
        activeView: "local",
        deviceMenuOpen: false,
        settingsOpen: false,
        notification: null,

        currentTrack: () => storeGet().playlist[storeGet().currentTrackIndex] ?? null,

        ...createPlaybackActions(storeSet, storeGet),
        ...createUiActions(storeSet, storeGet),
        ...createLibraryActions(storeSet, storeGet),
        ...createBilibiliActions(storeSet, storeGet),
        ...createLyricsActions(storeSet, storeGet),
        ...createOutputActions(storeSet, storeGet),
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
        // 发现1：持久化当前曲目 id；playlist 未加载（为空）时回退到已持久化的 id
        persistedCurrentTrackId:
          state.playlist[state.currentTrackIndex]?.id ?? state.persistedCurrentTrackId,
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
      }),
    }
  )
);
