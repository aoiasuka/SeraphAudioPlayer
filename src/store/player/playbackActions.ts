import { sendCommand, sendCommandAsync } from "./commands";
import { ensurePlayableTrack, bilibiliImportErrorMessage } from "./bilibiliActions";
import { sendPlayCommand } from "./outputActions";
import { bumpPlayEpoch, currentPlayEpoch } from "./playEpoch";
import { syncPlaybackModes, syncPlaybackQueue } from "./queueSync";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";
import { isTauriRuntime } from "@/lib/tauri";
import type { Track } from "@/types/track";

// 审2-R2：代际计数迁至 ./playEpoch（避免与 outputActions 循环导入），此处重新导出保持既有导入兼容
export { bumpPlayEpoch, currentPlayEpoch } from "./playEpoch";

interface VolumeDebounceState {
  timer: number | null;
  pending: number | null;
}

const volumeDebounce: VolumeDebounceState = { timer: null, pending: null };

// 发现7：seek 后 400ms 内忽略明显偏离目标位置的旧 Progress 事件，避免进度条回跳闪烁。
export const seekGuard = { until: 0, target: 0 };

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
      bumpPlayEpoch();
      sendCommand("pause");
      set({ isPlaying: false });
      return;
    }

    const epoch = bumpPlayEpoch();
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
        // 发现2：期间用户已有更新的播放意图，丢弃过期续体
        if (epoch !== currentPlayEpoch()) return;
        if (get().currentTrack()?.id !== playableTrack.id) return;
        // 审2-R2：把复查回调下沉进 sendPlayCommand，其内部两个 await 之后、
        // 真正发 "play" 之前再核对一次代际与当前曲目。
        void sendPlayCommand(playableTrack, get, set, get().currentTime, () =>
          epoch === currentPlayEpoch() &&
          get().currentTrack()?.id === playableTrack.id
        )
          .then(() => {
            if (epoch !== currentPlayEpoch()) return;
            if (get().currentTrack()?.id !== playableTrack.id) return;
            // 发现15：Tauri 下 isPlaying 由 playback_started 事件驱动，
            // 不在此乐观置位，避免短暂覆盖用户刚按下的暂停；stub 模式无事件，保留置位。
            if (!isTauriRuntime()) set({ isPlaying: true });
            get().showNotification(`正在播放: ${playableTrack.title}`);
          })
          .catch((err) => {
            reportPlaybackCommandError(get, "Failed to start playback", err);
            if (epoch === currentPlayEpoch()) set({ isPlaying: false });
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
    // 审2-R6：切歌使上一次 seek 的抑制窗口失效，避免误吞新曲目开头的 Progress 事件
    seekGuard.until = 0;
    resetNextIndexCache();
    void syncPlaybackQueue(get)
      .then(() => sendCommandAsync("next_track"))
      .catch((err) =>
        reportPlaybackCommandError(get, "Failed to advance to next track", err)
      );
  },

  prevTrack: () => {
    if (get().playlist.length === 0) return;
    // 审2-R6：切歌使上一次 seek 的抑制窗口失效
    seekGuard.until = 0;
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
    // 审2-R6：切歌使上一次 seek 的抑制窗口失效
    seekGuard.until = 0;
    // 发现2：任何新的选曲都会使先前挂起的播放续体过期
    const epoch = bumpPlayEpoch();
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
          // 发现2：期间用户已切到别的曲目，丢弃过期续体
          if (epoch !== currentPlayEpoch()) return;
          if (get().currentTrack()?.id !== playableTrack.id) return;
          // 审2-R2：复查回调下沉进 sendPlayCommand，内部 await 之后再核对一次
          void sendPlayCommand(playableTrack, get, set, 0, () =>
            epoch === currentPlayEpoch() &&
            get().currentTrack()?.id === playableTrack.id
          ).catch((err) => {
            reportPlaybackCommandError(get, "Failed to start playback", err);
            if (epoch === currentPlayEpoch()) set({ isPlaying: false });
          });
        })
        .catch((err) => {
          // eslint-disable-next-line no-console
          console.warn("Failed to prepare streaming track", err);
          get().showNotification(bilibiliImportErrorMessage(err));
          // 审2-R4：重缓存失败时复位播放态，避免 UI 停留在“播放中”而实际无声；
          // 仅当播放意图仍指向本曲目时才复位，不影响用户随后切走的新播放。
          if (
            epoch === currentPlayEpoch() &&
            get().currentTrack()?.id === track.id
          ) {
            set({ isPlaying: false });
          }
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
    // 发现7：记录抑制窗口，忽略随后在途的旧位置 Progress 事件
    seekGuard.until = Date.now() + 400;
    seekGuard.target = seconds;
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
    // 审2-R8：滑到 0 时同步记录滑动前的音量，toggleMute 恢复时才不会回到过期的 previousVolume
    set((state) => {
      const next: Partial<PlayerStore> = { volume, isMuted: volume === 0 };
      if (volume === 0 && state.volume > 0) next.previousVolume = state.volume;
      return next;
    });
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
