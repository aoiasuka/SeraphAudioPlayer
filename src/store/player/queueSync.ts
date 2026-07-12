import type { PlayerStoreGet } from "./types";
import { sendCommandAsync } from "./commands";

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

export async function syncPlaybackQueue(get: PlayerStoreGet) {
  await sendCommandAsync("sync_playback_queue", playbackQueueArgs(get));
}

export async function syncPlaybackModes(get: PlayerStoreGet) {
  const { shuffleMode, loopMode } = get();
  await sendCommandAsync("set_playback_modes", { shuffleMode, loopMode });
}
