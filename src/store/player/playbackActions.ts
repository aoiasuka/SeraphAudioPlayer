import { sendCommand, sendCommandAsync } from "./commands";
import { ensurePlayableTrack, bilibiliImportErrorMessage } from "./bilibiliActions";
import { sendPlayCommand } from "./outputActions";
import { syncPlaybackModes, syncPlaybackQueue } from "./queueSync";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";
import type { Track } from "@/types/track";

interface VolumeDebounceState {
  timer: number | null;
  pending: number | null;
}

const volumeDebounce: VolumeDebounceState = { timer: null, pending: null };

export function withRecentTrack(ids: string[], trackId: string) {
  return [trackId, ...ids.filter((id) => id !== trackId)].slice(0, 12);
}

function nextSequentialTrackIndex(playlist: Track[], currentTrackIndex: number) {
  if (playlist.length === 0) return -1;
  return (currentTrackIndex + 1) % playlist.length;
}

function nextShuffleTrackIndex(
  playlist: Track[],
  currentTrackIndex: number,
  recentTrackIds: string[]
) {
  if (playlist.length === 0) return -1;
  if (playlist.length === 1) return 0;

  const candidates = playlist
    .map((_, index) => index)
    .filter((index) => index !== currentTrackIndex);
  const freshCandidates = candidates.filter(
    (index) => !recentTrackIds.includes(playlist[index].id)
  );
  const pool = freshCandidates.length > 0 ? freshCandidates : candidates;
  return pool[Math.floor(Math.random() * pool.length)];
}

function nextTrackIndex(
  playlist: Track[],
  currentTrackIndex: number,
  shuffleMode: boolean,
  recentTrackIds: string[]
) {
  return shuffleMode
    ? nextShuffleTrackIndex(playlist, currentTrackIndex, recentTrackIds)
    : nextSequentialTrackIndex(playlist, currentTrackIndex);
}

let nextIndexCache: { key: string; index: number } | null = null;

function nextIndexCacheKey(
  playlist: Track[],
  currentTrackIndex: number,
  shuffleMode: boolean,
  recentTrackIds: string[]
) {
  return [
    playlist.length,
    currentTrackIndex,
    playlist[currentTrackIndex]?.id ?? "",
    shuffleMode ? "s" : "o",
    recentTrackIds.join(","),
  ].join("|");
}

function resolveNextIndex(
  playlist: Track[],
  currentTrackIndex: number,
  shuffleMode: boolean,
  recentTrackIds: string[]
) {
  if (playlist.length === 0) return -1;
  const key = nextIndexCacheKey(
    playlist,
    currentTrackIndex,
    shuffleMode,
    recentTrackIds
  );
  if (nextIndexCache && nextIndexCache.key === key) return nextIndexCache.index;
  const index = nextTrackIndex(
    playlist,
    currentTrackIndex,
    shuffleMode,
    recentTrackIds
  );
  nextIndexCache = { key, index };
  return index;
}

export function resetNextIndexCache() {
  nextIndexCache = null;
}

export function playbackErrorMessage(err: unknown) {
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : "";

  return message ? `播放失败：${message}` : "播放失败";
}

function clampVolume(volume: number) {
  if (!Number.isFinite(volume)) return 0;
  return Math.max(0, Math.min(1, volume));
}

function cancelQueuedVolumeCommand() {
  if (volumeDebounce.timer !== null) {
    window.clearTimeout(volumeDebounce.timer);
    volumeDebounce.timer = null;
  }
  volumeDebounce.pending = null;
}

function sendVolumeCommandNow(volume: number) {
  cancelQueuedVolumeCommand();
  sendCommand("set_volume", { volume });
}

function queueVolumeCommand(volume: number) {
  volumeDebounce.pending = volume;
  if (volumeDebounce.timer !== null) return;

  sendCommand("set_volume", { volume });
  volumeDebounce.pending = null;
  volumeDebounce.timer = window.setTimeout(() => {
    volumeDebounce.timer = null;
    if (volumeDebounce.pending !== null) {
      const nextVolume = volumeDebounce.pending;
      volumeDebounce.pending = null;
      queueVolumeCommand(nextVolume);
    }
  }, 80);
}

function reportPlaybackCommandError(
  get: PlayerStoreGet,
  context: string,
  err: unknown
) {
  // eslint-disable-next-line no-console
  console.warn(context, err);
  get().showNotification(playbackErrorMessage(err));
}

export function createPlaybackActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "nextTrackPreview" | "togglePlayback" | "nextTrack" | "prevTrack" | "loadTrack" | "seek" | "tick" | "setVolume" | "toggleMute" | "toggleShuffle" | "toggleLoop" | "toggleLike"> {
  return {
  nextTrackPreview: () => {
    const { playlist, currentTrackIndex, recentTrackIds, shuffleMode } = get();
    const next = resolveNextIndex(
      playlist,
      currentTrackIndex,
      shuffleMode,
      recentTrackIds
    );
    return next >= 0 ? playlist[next] ?? null : null;
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
        void sendPlayCommand(playableTrack, get, set, get().currentTime)
          .then(() => {
            set({ isPlaying: true });
            get().showNotification(`正在播放: ${playableTrack.title}`);
          })
          .catch((err) => {
            reportPlaybackCommandError(get, "Failed to start playback", err);
            set({ isPlaying: false });
          });
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Failed to prepare streaming track", err);
        get().showNotification(bilibiliImportErrorMessage(err));
      });
  },

  nextTrack: () => {
    if (get().playlist.length === 0) return;
    resetNextIndexCache();
    void syncPlaybackQueue(get)
      .then(() => sendCommandAsync("next_track"))
      .catch((err) =>
        reportPlaybackCommandError(get, "Failed to advance to next track", err)
      );
  },

  prevTrack: () => {
    if (get().playlist.length === 0) return;
    resetNextIndexCache();
    void syncPlaybackQueue(get)
      .then(() => sendCommandAsync("prev_track"))
      .catch((err) =>
        reportPlaybackCommandError(get, "Failed to return to previous track", err)
      );
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
    resetNextIndexCache();
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
        .then((playableTrack) => {
          void sendPlayCommand(playableTrack, get, set, 0).catch((err) => {
            reportPlaybackCommandError(get, "Failed to start playback", err);
            set({ isPlaying: false });
          });
        })
        .catch((err) => {
          // eslint-disable-next-line no-console
          console.warn("Failed to prepare streaming track", err);
          get().showNotification(bilibiliImportErrorMessage(err));
        });
    } else {
      void syncPlaybackQueue(get).catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Failed to sync selected track", err);
      });
    }
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
    resetNextIndexCache();
    void syncPlaybackModes(get).catch((err) => {
      // eslint-disable-next-line no-console
      console.warn("Failed to sync shuffle mode", err);
    });
    get().showNotification(next ? "随机播放已启用" : "顺序播放已启用");
  },

  toggleLoop: () => {
    const next = !get().loopMode;
    set({ loopMode: next });
    void syncPlaybackModes(get).catch((err) => {
      // eslint-disable-next-line no-console
      console.warn("Failed to sync loop mode", err);
    });
    get().showNotification(next ? "单曲循环已开启" : "单曲循环已关闭");
  },

  toggleLike: (trackId) => {
    const current = get().liked[trackId] ?? false;
    set({ liked: { ...get().liked, [trackId]: !current } });
    get().showNotification(current ? "已取消收藏" : "已加入我喜欢");
  },
  };
}
