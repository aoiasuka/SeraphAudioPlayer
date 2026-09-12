import { invoke } from "@/lib/tauri";
import type { PlaybackQueuePreview, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

let previewRequest = 0;

function applyPreview(
  set: PlayerStoreSet,
  requestedState: PlayerStore,
  preview: PlaybackQueuePreview | undefined,
  request: number
) {
  if (!preview || request !== previewRequest) return;
  set((state) => {
    // 同步期间已经切歌或修改队列时，旧请求不能覆盖新曲目的预览。
    if (
      state.playlist !== requestedState.playlist ||
      state.currentTrackIndex !== requestedState.currentTrackIndex ||
      state.recentTrackIds !== requestedState.recentTrackIds ||
      state.shuffleMode !== requestedState.shuffleMode ||
      state.loopMode !== requestedState.loopMode
    ) return {};
    return { playbackQueuePreview: preview };
  });
}

export function playbackQueueArgs(get: PlayerStoreGet) {
  const {
    playlist,
    currentTrackIndex,
    recentTrackIds,
    shuffleMode,
    loopMode,
  } = get();

  return {
    tracks: playlist.map((track) => ({
      id: track.id,
      path: track.path,
      // SMTC 系统媒体浮窗展示用元数据
      title: track.title,
      artist: track.artist,
      album: track.album,
      cover: track.cover,
      duration: track.duration,
    })),
    currentTrackIndex,
    recentTrackIds,
    shuffleMode,
    loopMode,
  };
}

export async function syncPlaybackQueue(get: PlayerStoreGet, set: PlayerStoreSet) {
  const requestedState = get();
  const request = ++previewRequest;
  const preview = await invoke<PlaybackQueuePreview>("sync_playback_queue", playbackQueueArgs(get));
  applyPreview(set, requestedState, preview, request);
}

export async function syncPlaybackModes(get: PlayerStoreGet, set: PlayerStoreSet) {
  const requestedState = get();
  const { shuffleMode, loopMode } = requestedState;
  const request = ++previewRequest;
  const preview = await invoke<PlaybackQueuePreview>("set_playback_modes", { shuffleMode, loopMode });
  applyPreview(set, requestedState, preview, request);
}
