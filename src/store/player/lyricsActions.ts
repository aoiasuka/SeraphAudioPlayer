import { invoke } from "@/lib/tauri";
import type { LyricLine, OnlineLyricsCandidate, Track } from "@/types/track";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

const MAX_LYRIC_FILE_BYTES = 2 * 1024 * 1024;

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

function onlineLyricsErrorMessage(err: unknown) {
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : "";

  if (!message) return "在线歌词获取失败";
  if (message.includes("missing track title")) return "当前曲目缺少标题";
  if (message.includes("online lyrics not found")) {
    return "没有匹配到在线歌词";
  }
  if (message.includes("track was not found")) {
    return "当前曲目未写入曲库缓存，请重新导入音频";
  }
  if (message.includes("failed to write library cache")) {
    return "无法写入曲库缓存";
  }

  return `在线歌词获取失败：${message}`;
}

export function createLyricsActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "importLyricsForCurrentTrack" | "fetchOnlineLyricsForCurrentTrack" | "applyOnlineLyricsForCurrentTrack"> {
  return {
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

  fetchOnlineLyricsForCurrentTrack: async (query) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return [];
    }

    const manualQuery = query?.trim();

    try {
      const candidates = await invoke<OnlineLyricsCandidate[]>(
        "fetch_online_lyrics",
        {
          trackId: track.id,
          title: manualQuery || track.title,
          artist: manualQuery ? "" : track.artist,
          duration: track.duration,
        }
      );

      if (!Array.isArray(candidates) || candidates.length === 0) {
        get().showNotification("没有匹配到在线歌词");
        return [];
      }

      get().showNotification(`找到 ${candidates.length} 份在线歌词`);
      return candidates;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: fetch_online_lyrics", err);
      get().showNotification(onlineLyricsErrorMessage(err));
      return [];
    }
  },

  applyOnlineLyricsForCurrentTrack: async (lyrics) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return false;
    }

    if (lyrics.length === 0) {
      get().showNotification("歌词内容为空");
      return false;
    }

    try {
      const savedLyrics = await invoke<LyricLine[]>("apply_online_lyrics", {
        trackId: track.id,
        trackPath: track.path,
        lyrics,
      });

      if (!Array.isArray(savedLyrics) || savedLyrics.length === 0) {
        get().showNotification("歌词内容为空");
        return false;
      }

      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, savedLyrics),
      }));
      get().showNotification(`已应用 ${savedLyrics.length} 行在线歌词`);
      return true;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: apply_online_lyrics", err);
      get().showNotification(onlineLyricsErrorMessage(err));
      return false;
    }
  },
  };
}

