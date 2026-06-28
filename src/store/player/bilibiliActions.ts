import { invoke } from "@/lib/tauri";
import type { Track } from "@/types/track";
import { mergeIncomingTrack, mergeTracksByPathWithStats } from "./libraryActions";
import { streamingSourceInput, trackMergeKey } from "./trackIdentity";
import type { BilibiliBatchImportResult, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

export async function ensurePlayableTrack(
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

export function bilibiliImportErrorMessage(err: unknown) {
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

export function createBilibiliActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "importBilibiliAudio" | "importBilibiliFavorites"> {
  return {
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
  };
}

