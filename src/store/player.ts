import { create } from "zustand";
import { persist } from "zustand/middleware";
import { mockDevices, mockPlaylist } from "@/data/mock-playlist";
import { createBilibiliActions } from "./player/bilibiliActions";
import { createLibraryActions } from "./player/libraryActions";
import { createLyricsActions } from "./player/lyricsActions";
import { createOutputActions } from "./player/outputActions";
import { createPlaybackActions } from "./player/playbackActions";
import { createPlayerPersistStorage } from "./player/persistStorage";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./player/types";
import { createUiActions } from "./player/uiActions";

export type {
  BilibiliBatchImportResult,
  BilibiliImportFailure,
  BilibiliImportOptions,
  PlayerStore,
} from "./player/types";

export const usePlayerStore = create<PlayerStore>()(
  persist(
    (set, get) => {
      const storeSet = set as PlayerStoreSet;
      const storeGet = get as PlayerStoreGet;

      return {
        playlist: mockPlaylist,
        currentTrackIndex: 0,
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
      storage: createPlayerPersistStorage(),
      skipHydration: true,
      partialize: (state) => ({
        currentTrackIndex: state.currentTrackIndex,
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
