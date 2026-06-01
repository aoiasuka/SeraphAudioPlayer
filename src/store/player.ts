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

function streamingSourceInput(track: Track) {
  const bvid = bvidFromTrack(track);
  return (
    track.sourceUrl?.trim() ||
    track.sourceId?.trim() ||
    (bvid ? `https://www.bilibili.com/video/${bvid}` : "")
  );
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

function normalizeText(value: string | undefined | null) {
  return (value ?? "").trim().replace(/\s+/g, " ").toLowerCase();
}

function bvidFromTrack(track: Track) {
  const sourceId = track.sourceId?.trim();
  if (sourceId?.toLowerCase().startsWith("bv")) return sourceId.toLowerCase();

  const sourceUrl = track.sourceUrl ?? "";
  const sourceMatch = sourceUrl.match(/BV[a-zA-Z0-9]+/);
  if (sourceMatch) return sourceMatch[0].toLowerCase();

  const idMatch = track.id.match(/bilibili-(bv[a-zA-Z0-9]+)/i);
  if (idMatch) return idMatch[1].toLowerCase();

  const pathMatch = track.path.match(/(BV[a-zA-Z0-9]+)-\d+/i);
  if (pathMatch) return pathMatch[1].toLowerCase();

  return "";
}

function isBilibiliTrack(track: Track) {
  return track.id.startsWith("bilibili-") || track.album === "Bilibili";
}

function trackMergeKey(track: Track) {
  const bvid = bvidFromTrack(track);
  if (bvid) return `bvid:${bvid}`;
  const sourceId = track.sourceId?.trim().toLowerCase();
  if (sourceId) return `source-id:${sourceId}`;
  const sourceUrl = track.sourceUrl?.trim().toLowerCase();
  if (sourceUrl) return `source-url:${sourceUrl}`;
  if (isBilibiliTrack(track)) {
    return [
      "bilibili-meta",
      normalizeText(track.title),
      normalizeText(track.artist),
      Math.round(track.duration || 0),
    ].join(":");
  }
  return `path:${normalizePath(track.path)}`;
}

function dedupeTracks(tracks: Track[]) {
  const byKey = new Map<string, Track>();
  const orderedKeys: string[] = [];

  for (const track of tracks) {
    const key = trackMergeKey(track);
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, track);
      orderedKeys.push(key);
      continue;
    }

    const preferred =
      existing.cacheMissing && !track.cacheMissing
        ? mergeIncomingTrack(existing, track)
        : mergeIncomingTrack(track, existing);
    byKey.set(key, preferred);
  }

  return orderedKeys.map((key) => byKey.get(key)).filter((track): track is Track => !!track);
}

function dedupeTracksWithLiked(tracks: Track[], liked: Record<string, boolean>) {
  const likedByKey = new Set<string>();
  for (const track of tracks) {
    if (liked[track.id]) likedByKey.add(trackMergeKey(track));
  }

  const playlist = dedupeTracks(tracks);
  const nextLiked = { ...liked };
  for (const track of playlist) {
    if (likedByKey.has(trackMergeKey(track))) {
      nextLiked[track.id] = true;
    }
  }

  return { playlist, liked: nextLiked };
}

function mergeTracksByPath(existing: Track[], incoming: Track[]) {
  const remaining = new Map<string, Track>();
  for (const track of incoming) {
    const key = trackMergeKey(track);
    if (key) remaining.set(key, track);
  }

  const playlist = existing.map((track) => {
    const key = trackMergeKey(track);
    const updated = remaining.get(key);
    if (!updated) return track;
    remaining.delete(key);
    return mergeIncomingTrack(track, updated);
  });

  return dedupeTracks([...playlist, ...Array.from(remaining.values())]);
}

function mergeTracksByPathWithStats(existing: Track[], incoming: Track[]) {
  const remaining = new Map<string, Track>();
  for (const track of incoming) {
    const key = trackMergeKey(track);
    if (key) remaining.set(key, track);
  }

  let updatedCount = 0;
  const playlist = existing.map((track) => {
    const key = trackMergeKey(track);
    const updated = remaining.get(key);
    if (!updated) return track;
    remaining.delete(key);
    updatedCount += 1;
    return mergeIncomingTrack(track, updated);
  });
  const addedTracks = Array.from(remaining.values());

  return {
    playlist: dedupeTracks([...playlist, ...addedTracks]),
    addedCount: addedTracks.length,
    updatedCount,
  };
}

function mergeIncomingTrack(existing: Track, incoming: Track) {
  const incomingLyrics = incoming.lyrics ?? [];
  const existingLyrics = existing.lyrics ?? [];
  const merged = {
    ...incoming,
    sourceUrl: incoming.sourceUrl ?? existing.sourceUrl,
    sourceId: incoming.sourceId ?? existing.sourceId,
    cacheMissing: incoming.cacheMissing ?? false,
  };
  if (incomingLyrics.length === 0 && existingLyrics.length > 0) {
    return { ...merged, lyrics: existingLyrics };
  }
  return { ...merged, lyrics: incomingLyrics };
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

async function ensurePlayableTrack(
  track: Track,
  replaceTrack: (track: Track) => void,
  notify: (text: string) => void
) {
  const sourceInput = streamingSourceInput(track);
  if (!track.cacheMissing || !sourceInput) {
    return track;
  }

  notify(`正在重新缓存: ${track.title}`);
  const imported = await invoke<Track>("import_bilibili_audio_with_options", {
    input: sourceInput,
    options: {
      preferFlac: true,
      preferDolbyAtmos: true,
      remuxWithFfmpeg: true,
    },
  });

  const merged = mergeIncomingTrack(track, imported);
  replaceTrack(merged);
  notify(`已重新缓存: ${merged.title}`);
  return merged;
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

function bilibiliImportErrorMessage(err: unknown) {
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : "";

  if (!message) return "导入 B 站音频失败";
  if (message.includes("BV") || message.includes("B 站链接")) return message;
  if (message.includes("no dash audio") || message.includes("no usable audio")) {
    return "这个视频没有可用的 DASH 音频流";
  }
  if (message.includes("403") || message.includes("401")) {
    return "B 站拒绝了音频下载，可能需要登录或该内容受限";
  }
  if (message.includes("404")) return "B 站音频链接已失效，请重新导入";
  if (message.includes("timed out") || message.includes("timeout")) {
    return "连接 B 站超时，请稍后重试";
  }

  return `导入 B 站音频失败：${message}`;
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

    void ensurePlayableTrack(
      track,
      (updatedTrack) => {
        set((state) => ({
          playlist: state.playlist.map((item) =>
            item.id === track.id ? updatedTrack : item
          ),
        }));
      },
      get().showNotification
    )
      .then((playableTrack) => {
        sendPlayCommand(playableTrack, get().currentTime);
        set({ isPlaying: true });
        get().showNotification(`正在播放: ${playableTrack.title}`);
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Failed to prepare streaming track", err);
        get().showNotification(bilibiliImportErrorMessage(err));
      });
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
    if (wasPlaying) {
      void ensurePlayableTrack(
        track,
        (updatedTrack) => {
          set((state) => ({
            playlist: state.playlist.map((item, itemIndex) =>
              itemIndex === index ? updatedTrack : item
            ),
          }));
        },
        get().showNotification
      )
        .then((playableTrack) => sendPlayCommand(playableTrack, 0))
        .catch((err) => {
          // eslint-disable-next-line no-console
          console.warn("Failed to prepare streaming track", err);
          get().showNotification(bilibiliImportErrorMessage(err));
        });
    }
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
        ...dedupeTracksWithLiked(
          mergeTracksByPath(state.playlist, cached),
          state.liked
        ),
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

  importBilibiliAudio: async (input, options) => {
    const cleanInput = input.trim();
    if (!cleanInput) {
      get().showNotification("请输入 B 站视频链接或 BV 号");
      return;
    }

    try {
      const imported = await invoke<Track>("import_bilibili_audio_with_options", {
        input: cleanInput,
        options,
      });

      if (!imported?.path) {
        get().showNotification("没有解析到可用的 B 站音频");
        return;
      }

      let added = false;
      let updated = false;
      const previousLength = get().playlist.length;

      set((state) => {
        const incomingKey = trackMergeKey(imported);
        const existingIndex = state.playlist.findIndex(
          (track) => trackMergeKey(track) === incomingKey
        );

        if (existingIndex >= 0) {
          updated = true;
          const playlist = state.playlist.map((track, index) =>
            index === existingIndex ? mergeIncomingTrack(track, imported) : track
          );
          return {
            playlist,
            currentTrackIndex: state.currentTrackIndex,
            activeView: "streaming",
          };
        }

        added = true;
        return {
          playlist: [...state.playlist, imported],
          currentTrackIndex: previousLength === 0 ? 0 : state.currentTrackIndex,
          activeView: "streaming",
        };
      });

      get().showNotification(
        added
          ? `已添加 B 站音频: ${imported.title}`
          : updated
            ? `已更新 B 站音频: ${imported.title}`
            : "B 站音频已在曲库中"
      );
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: import_bilibili_audio", err);
      get().showNotification(bilibiliImportErrorMessage(err));
    }
  },

  importBilibiliFavorites: async (input, options) => {
    const cleanInput = input.trim();
    if (!cleanInput) {
      get().showNotification("请输入 B 站收藏夹链接、media_id 或 fid");
      return null;
    }

    try {
      const result = await invoke<BilibiliBatchImportResult>("import_bilibili_favorites", {
        input: cleanInput,
        options,
      });
      const tracks = Array.isArray(result.tracks) ? result.tracks : [];
      const failed = Array.isArray(result.failed) ? result.failed : [];

      if (tracks.length > 0) {
        const previousLength = get().playlist.length;
        const stats = mergeTracksByPathWithStats(get().playlist, tracks);
        set({
          playlist: stats.playlist,
          currentTrackIndex: previousLength === 0 ? 0 : get().currentTrackIndex,
          activeView: "streaming",
        });
        get().showNotification(
          `收藏夹导入完成：新增 ${stats.addedCount} 首，更新 ${stats.updatedCount} 首，失败 ${failed.length} 首`
        );
      } else {
        get().showNotification(
          failed.length > 0
            ? `收藏夹导入失败：${failed[0].reason}`
            : "收藏夹里没有可导入的音频"
        );
      }

      return { tracks, failed };
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: import_bilibili_favorites", err);
      get().showNotification(bilibiliImportErrorMessage(err));
      return null;
    }
  },

  markTracksCacheMissingByPaths: (paths) => {
    const removed = new Set(paths.map(normalizePath).filter(Boolean));
    if (removed.size === 0) return;
    set((state) => {
      const playlist = state.playlist.map((track) =>
        removed.has(normalizePath(track.path)) && streamingSourceInput(track)
          ? { ...track, cacheMissing: true, size: "0 MB" }
          : track
      );
      return {
        playlist,
        currentTrackIndex: Math.min(state.currentTrackIndex, Math.max(playlist.length - 1, 0)),
      };
    });
  },

  normalizeLibrary: () => {
    set((state) => {
      const deduped = dedupeTracksWithLiked(state.playlist, state.liked);
      return {
        ...deduped,
        currentTrackIndex: Math.min(
          state.currentTrackIndex,
          Math.max(deduped.playlist.length - 1, 0)
        ),
      };
    });
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
