import { create } from "zustand";
import { persist, type PersistStorage } from "zustand/middleware";
import { mockDevices, mockPlaylist } from "@/data/mock-playlist";
import { invoke } from "@/lib/tauri";
import type { LibraryView, LyricLine, OutputDevice, Track } from "@/types/track";

interface NotificationPayload {
  id: number;
  text: string;
}

interface BackendDevice {
  id: string;
  name: string;
  is_default?: boolean;
  isDefault?: boolean;
}

interface PersistedPlayerState {
  currentTrackIndex: number;
  recentTrackIds: string[];
  volume: number;
  isMuted: boolean;
  previousVolume: number;
  shuffleMode: boolean;
  loopMode: boolean;
  liked: Record<string, boolean>;
  playlist: Track[];
  currentDeviceId: string;
  driverKind: "wasapi" | "asio" | "direct";
  activeView: LibraryView;
}

interface PlayerStore {
  playlist: Track[];
  currentTrackIndex: number;
  recentTrackIds: string[];
  isPlaying: boolean;
  currentTime: number;
  volume: number;
  isMuted: boolean;
  previousVolume: number;
  shuffleMode: boolean;
  loopMode: boolean;
  liked: Record<string, boolean>;
  devices: OutputDevice[];
  currentDeviceId: string;
  driverKind: "wasapi" | "asio" | "direct";
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
  loadBackendLibrary: () => Promise<void>;
  importLocalTracks: (paths: string[]) => Promise<void>;
  importLyricsForCurrentTrack: (file: File) => Promise<void>;
  loadDevices: () => void;
  selectDevice: (id: string) => void;
  setDriver: (k: "wasapi" | "asio" | "direct") => void;
  toggleDeviceMenu: () => void;
  closeDeviceMenu: () => void;
  toggleSettings: () => void;
  showNotification: (text: string) => void;
  dismissNotification: () => void;
}

let notificationCounter = 0;
let volumeCommandTimer: number | null = null;
let pendingVolumeCommand: number | null = null;
const MAX_LYRIC_FILE_BYTES = 2 * 1024 * 1024;

function withRecentTrack(ids: string[], trackId: string) {
  return [trackId, ...ids.filter((id) => id !== trackId)].slice(0, 12);
}

function sendCommand(cmd: string, args?: Record<string, unknown>) {
  void invoke(cmd, args).catch((err) => {
    // eslint-disable-next-line no-console
    console.warn(`Tauri command failed: ${cmd}`, err);
  });
}

function sendPlayCommand(track: Track, startSeconds = 0) {
  sendCommand("play", {
    path: track.path,
    trackId: track.id,
    startSeconds,
  });
}

function normalizeDevice(device: BackendDevice): OutputDevice {
  return {
    id: device.id,
    name: device.name,
    isDefault: device.isDefault ?? device.is_default ?? false,
  };
}

function normalizePath(path: string) {
  return path.trim().toLowerCase();
}

function mergeTracksByPath(existing: Track[], incoming: Track[]) {
  const remaining = new Map<string, Track>();
  for (const track of incoming) {
    const key = normalizePath(track.path);
    if (key) remaining.set(key, track);
  }

  const playlist = existing.map((track) => {
    const key = normalizePath(track.path);
    const updated = remaining.get(key);
    if (!updated) return track;
    remaining.delete(key);
    return mergeIncomingTrack(track, updated);
  });

  return [...playlist, ...Array.from(remaining.values())];
}

function mergeIncomingTrack(existing: Track, incoming: Track) {
  const incomingLyrics = incoming.lyrics ?? [];
  const existingLyrics = existing.lyrics ?? [];
  if (incomingLyrics.length === 0 && existingLyrics.length > 0) {
    return { ...incoming, lyrics: existingLyrics };
  }
  return { ...incoming, lyrics: incomingLyrics };
}

function replaceTrackLyrics(
  playlist: Track[],
  trackId: string,
  lyrics: LyricLine[]
) {
  return playlist.map((track) =>
    track.id === trackId ? { ...track, lyrics } : track
  );
}

function lyricImportErrorMessage(err: unknown) {
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : "";

  if (!message) return "导入歌词失败";
  if (message.includes("missing track id")) return "当前曲目缺少 ID";
  if (message.includes("lyrics file is empty")) return "歌词文件为空";
  if (message.includes("no usable text")) return "歌词文件没有可用内容";
  if (message.includes("audio file is unavailable")) {
    return "当前曲目未写入曲库缓存，且原音频文件不可用，请重新导入音频";
  }
  if (message.includes("track was not found")) {
    return "当前曲目未写入曲库缓存，请重新导入音频";
  }
  if (message.includes("failed to parse library cache")) {
    return "曲库缓存损坏，无法保存歌词";
  }
  if (message.includes("failed to write library cache")) {
    return "无法写入曲库缓存";
  }

  return `导入歌词失败：${message}`;
}

function clampVolume(volume: number) {
  if (!Number.isFinite(volume)) return 0;
  return Math.max(0, Math.min(1, volume));
}

function cancelQueuedVolumeCommand() {
  if (volumeCommandTimer !== null) {
    window.clearTimeout(volumeCommandTimer);
    volumeCommandTimer = null;
  }
  pendingVolumeCommand = null;
}

function sendVolumeCommandNow(volume: number) {
  cancelQueuedVolumeCommand();
  sendCommand("set_volume", { volume });
}

function queueVolumeCommand(volume: number) {
  pendingVolumeCommand = volume;
  if (volumeCommandTimer !== null) return;

  sendCommand("set_volume", { volume });
  pendingVolumeCommand = null;
  volumeCommandTimer = window.setTimeout(() => {
    volumeCommandTimer = null;
    if (pendingVolumeCommand !== null) {
      const nextVolume = pendingVolumeCommand;
      pendingVolumeCommand = null;
      queueVolumeCommand(nextVolume);
    }
  }, 80);
}

function isSamePersistedState(
  previous: PersistedPlayerState,
  next: PersistedPlayerState
) {
  return (
    previous.currentTrackIndex === next.currentTrackIndex &&
    previous.recentTrackIds === next.recentTrackIds &&
    previous.volume === next.volume &&
    previous.isMuted === next.isMuted &&
    previous.previousVolume === next.previousVolume &&
    previous.shuffleMode === next.shuffleMode &&
    previous.loopMode === next.loopMode &&
    previous.liked === next.liked &&
    previous.playlist === next.playlist &&
    previous.currentDeviceId === next.currentDeviceId &&
    previous.driverKind === next.driverKind &&
    previous.activeView === next.activeView
  );
}

function createPlayerPersistStorage(): PersistStorage<PersistedPlayerState> {
  let lastValue: {
    state: PersistedPlayerState;
    version?: number;
  } | null = null;

  return {
    getItem: (name) => {
      const value = window.localStorage.getItem(name);
      if (value === null) {
        lastValue = null;
        return null;
      }

      const parsed = JSON.parse(value) as {
        state: PersistedPlayerState;
        version?: number;
      };
      lastValue = parsed;
      return parsed;
    },
    setItem: (name, value) => {
      if (
        lastValue &&
        lastValue.version === value.version &&
        isSamePersistedState(lastValue.state, value.state)
      ) {
        return;
      }

      lastValue = value;
      window.localStorage.setItem(name, JSON.stringify(value));
    },
    removeItem: (name) => {
      lastValue = null;
      window.localStorage.removeItem(name);
    },
  };
}

export const usePlayerStore = create<PlayerStore>()(
  persist(
    (set, get) => ({
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
  devices: mockDevices,
  currentDeviceId: "wasapi:hd-dac1",
  driverKind: "wasapi",
  activeView: "local",
  deviceMenuOpen: false,
  settingsOpen: false,
  notification: null,

  currentTrack: () => get().playlist[get().currentTrackIndex] ?? null,

  nextTrackPreview: () => {
    const { playlist, currentTrackIndex } = get();
    if (playlist.length === 0) return null;
    return playlist[(currentTrackIndex + 1) % playlist.length];
  },

  togglePlayback: () => {
    const { isPlaying, currentTrack } = get();
    const track = currentTrack();
    if (!track) {
      get().showNotification("请先添加本地音乐");
      return;
    }

    if (isPlaying) {
      sendCommand("pause");
      set({ isPlaying: false });
      return;
    }

    sendPlayCommand(track, get().currentTime);
    set({ isPlaying: true });
    get().showNotification(`正在播放: ${track.title}`);
  },

  nextTrack: () => {
    const { currentTrackIndex, playlist } = get();
    if (playlist.length === 0) return;
    const next = (currentTrackIndex + 1) % playlist.length;
    sendCommand("next_track");
    get().loadTrack(next);
    get().showNotification(`已切换到: ${playlist[next].title}`);
  },

  prevTrack: () => {
    const { currentTrackIndex, playlist } = get();
    if (playlist.length === 0) return;
    const prev = (currentTrackIndex - 1 + playlist.length) % playlist.length;
    sendCommand("prev_track");
    get().loadTrack(prev);
    get().showNotification(`已切换到: ${playlist[prev].title}`);
  },

  loadTrack: (index) => {
    const track = get().playlist[index];
    if (!track) return;
    const wasPlaying = get().isPlaying;
    set({
      currentTrackIndex: index,
      currentTime: 0,
      recentTrackIds: withRecentTrack(get().recentTrackIds, track.id),
    });
    if (wasPlaying) sendPlayCommand(track, 0);
  },

  setActiveView: (view) => {
    if (get().activeView === view) return;
    set({ activeView: view });
  },

  seek: (sec) => {
    const track = get().currentTrack();
    if (!track) return;
    const seconds = Math.max(0, Math.min(sec, track.duration));
    if (get().currentTime === seconds) return;
    sendCommand("seek", { seconds });
    set({ currentTime: seconds });
  },

  tick: () => {
    const { isPlaying, currentTime, loopMode } = get();
    if (!isPlaying) return;
    const track = get().currentTrack();
    if (!track) return;
    const next = currentTime + 1;
    if (next >= track.duration) {
      if (loopMode) set({ currentTime: 0 });
      else get().nextTrack();
      return;
    }
    set({ currentTime: next });
  },

  setVolume: (v) => {
    const volume = clampVolume(v);
    if (get().volume === volume) return;

    queueVolumeCommand(volume);
    set({ volume, isMuted: volume === 0 });
  },

  toggleMute: () => {
    const { isMuted, volume, previousVolume } = get();
    if (!isMuted) {
      sendVolumeCommandNow(0);
      set({ previousVolume: volume, volume: 0, isMuted: true });
      get().showNotification("已静音");
      return;
    }

    const restoredVolume = clampVolume(previousVolume || 0.7);
    sendVolumeCommandNow(restoredVolume);
    set({ volume: restoredVolume, isMuted: false });
    get().showNotification(`音量恢复到 ${Math.round(restoredVolume * 100)}%`);
  },

  toggleShuffle: () => {
    const next = !get().shuffleMode;
    set({ shuffleMode: next });
    get().showNotification(next ? "随机播放已启用" : "顺序播放已启用");
  },

  toggleLoop: () => {
    const next = !get().loopMode;
    set({ loopMode: next });
    get().showNotification(next ? "单曲循环已开启" : "单曲循环已关闭");
  },

  toggleLike: (trackId) => {
    const current = get().liked[trackId] ?? false;
    set({ liked: { ...get().liked, [trackId]: !current } });
    get().showNotification(current ? "已取消收藏" : "已加入我喜欢");
  },

  loadBackendLibrary: async () => {
    try {
      const cached = await invoke<Track[]>("get_playlist");
      if (!Array.isArray(cached) || cached.length === 0) return;

      set((state) => ({
        playlist: mergeTracksByPath(state.playlist, cached),
        currentTrackIndex:
          state.playlist.length === 0 && cached.length > 0
            ? 0
            : state.currentTrackIndex,
      }));
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: get_playlist", err);
    }
  },

  importLocalTracks: async (paths) => {
    const cleanPaths = paths.filter(Boolean);
    if (cleanPaths.length === 0) return;

    try {
      const imported = await invoke<Track[]>("import_tracks", { paths: cleanPaths });
      const importedByPath = new Map<string, Track>();

      for (const track of imported) {
        const key = normalizePath(track.path);
        if (key) importedByPath.set(key, track);
      }

      if (importedByPath.size === 0) {
        get().showNotification("没有可添加的新音频文件");
        return;
      }

      let updatedCount = 0;
      let addedCount = 0;
      const previousLength = get().playlist.length;

      set((state) => {
        const remaining = new Map(importedByPath);
        const playlist = state.playlist.map((track) => {
          const key = normalizePath(track.path);
          const updatedTrack = remaining.get(key);
          if (!updatedTrack) return track;

          remaining.delete(key);
          updatedCount += 1;
          return updatedTrack;
        });
        const newTracks = Array.from(remaining.values());
        addedCount = newTracks.length;

        return {
          playlist: [...playlist, ...newTracks],
          currentTrackIndex: previousLength === 0 && newTracks.length > 0
            ? 0
            : state.currentTrackIndex,
          activeView: "local",
        };
      });

      if (updatedCount === 0 && addedCount === 0) {
        get().showNotification("没有可添加的新音频文件");
        return;
      }

      if (addedCount > 0 && updatedCount > 0) {
        get().showNotification(`已添加 ${addedCount} 首，更新 ${updatedCount} 首本地音乐`);
      } else if (addedCount > 0) {
        get().showNotification(`已添加 ${addedCount} 首本地音乐`);
      } else {
        get().showNotification(`已更新 ${updatedCount} 首本地音乐`);
      }
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: import_tracks", err);
      get().showNotification("导入本地音乐失败");
    }
  },

  importLyricsForCurrentTrack: async (file) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return;
    }

    if (file.size === 0) {
      get().showNotification("歌词文件为空");
      return;
    }

    if (file.size > MAX_LYRIC_FILE_BYTES) {
      get().showNotification("歌词文件过大");
      return;
    }

    try {
      const lyricsBytes = Array.from(new Uint8Array(await file.arrayBuffer()));
      const lyrics = await invoke<LyricLine[]>("save_track_lyrics", {
        trackId: track.id,
        trackPath: track.path,
        lyricsBytes,
      });

      if (!Array.isArray(lyrics) || lyrics.length === 0) {
        get().showNotification("歌词文件没有可用内容");
        return;
      }

      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, lyrics),
      }));
      get().showNotification(`已导入 ${lyrics.length} 行歌词`);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: save_track_lyrics", err);
      get().showNotification(lyricImportErrorMessage(err));
    }
  },

  loadDevices: () => {
    void invoke<BackendDevice[]>("list_devices")
      .then((devices) => {
        if (!Array.isArray(devices) || devices.length === 0) return;
        const normalized = devices.map(normalizeDevice);
        const currentDeviceId = get().currentDeviceId;
        const selectedDeviceId =
          normalized.find((device) => device.id === currentDeviceId)?.id ??
          normalized.find((device) => device.isDefault)?.id ??
          normalized[0].id;
        set({
          devices: normalized,
          currentDeviceId: selectedDeviceId,
        });
        sendCommand("select_output_device", { deviceId: selectedDeviceId });
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: list_devices", err);
      });
  },

  selectDevice: (id) => {
    const { currentDeviceId, deviceMenuOpen } = get();
    if (currentDeviceId === id) {
      if (deviceMenuOpen) set({ deviceMenuOpen: false });
      return;
    }

    const device = get().devices.find((item) => item.id === id);
    sendCommand("select_output_device", { deviceId: id });
    set({ currentDeviceId: id, deviceMenuOpen: false });
    get().showNotification(`输出设备已切换到: ${device?.name ?? id}`);
  },

  setDriver: (k) => {
    if (get().driverKind === k) return;
    set({ driverKind: k });
  },

  toggleDeviceMenu: () => {
    const next = !get().deviceMenuOpen;
    set({ deviceMenuOpen: next });
    if (next) get().loadDevices();
  },

  closeDeviceMenu: () => {
    if (!get().deviceMenuOpen) return;
    set({ deviceMenuOpen: false });
  },

  toggleSettings: () => set({ settingsOpen: !get().settingsOpen }),

  showNotification: (text) => {
    notificationCounter += 1;
    set({ notification: { id: notificationCounter, text } });
  },

      dismissNotification: () => {
        if (get().notification === null) return;
        set({ notification: null });
      },
    }),
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
        playlist: state.playlist,
        currentDeviceId: state.currentDeviceId,
        driverKind: state.driverKind,
        activeView: state.activeView,
      }),
    }
  )
);
