import { invoke, normalizeIpcError } from "@/lib/tauri";
import type { Track } from "@/types/track";
import { mergeIncomingTrack, mergeTracksByPathWithStats } from "./libraryActions";
import { streamingSourceInput, trackMergeKey } from "./trackIdentity";
import type {
  BilibiliBatchImportResult,
  BilibiliBatchProgress,
  BilibiliImportOptions,
  BilibiliVideoInfo,
  PlayerStore,
  PlayerStoreGet,
  PlayerStoreSet,
} from "./types";

// 审2-R3：同一曲目在途的重缓存请求去重——并发触发（如快速重复点击）复用同一 Promise，
// 避免对同一曲目发起第二次下载。
const ensureInFlight = new Map<string, Promise<Track>>();

/**
 * 缓存缺失后自动重新下载时用的选项：沿用曲目原来的音质。
 *
 * 此前写死为「FLAC + 杜比全景声」全开——用户按普通音质导入的曲目，缓存被清理后
 * 一点播放就按无损或杜比重新下载：文件大得多、更耗流量，还会更快触发配额清理。
 * 导入时的选项没有落盘，这里按缓存文件的格式标注反推（后端 `format_from_codec`
 * 的取值：FLAC / EAC3 / M4A / OPUS / AUDIO）。混流只影响容器、不影响音质，保持开启。
 */
export function recacheOptionsForTrack(track: Track): BilibiliImportOptions {
  const format = (track.format ?? "").toUpperCase();
  return {
    preferFlac: format.includes("FLAC"),
    preferDolbyAtmos: format.includes("EAC3"),
    remuxWithFfmpeg: true,
    existingTrackId: track.id,
  };
}

async function recachePlayableTrack(
  track: Track,
  sourceInput: string,
  replaceTrack: (track: Track) => void,
  notify: (text: string) => void
) {
  notify(`正在重新缓存: ${track.title}`);
  const imported = await invoke<Track>("import_bilibili_audio_with_options", {
    input: sourceInput,
    options: recacheOptionsForTrack(track),
  });

  const merged = mergeIncomingTrack(track, imported);
  replaceTrack(merged);
  notify(`已重新缓存: ${merged.title}`);
  return merged;
}

export async function ensurePlayableTrack(
  track: Track,
  replaceTrack: (track: Track) => void,
  notify: (text: string) => void
) {
  const sourceInput = streamingSourceInput(track);
  if (!track.cacheMissing || !sourceInput) {
    return track;
  }

  // 审2-R3：已有同曲目请求在途时直接返回既有 Promise
  const inFlight = ensureInFlight.get(track.id);
  if (inFlight) return inFlight;

  const request = recachePlayableTrack(track, sourceInput, replaceTrack, notify);
  ensureInFlight.set(track.id, request);
  try {
    return await request;
  } finally {
    ensureInFlight.delete(track.id);
  }
}

export function bilibiliImportErrorMessage(err: unknown) {
  if (err == null) return "导入 B 站音频失败";
  // FE3-01：按后端给的错误码分支，不再对消息做子串猜测
  // （文件路径里带 404、标题里带 BV 的失败都曾被误分类）。
  const { code, message } = normalizeIpcError(err);
  switch (code) {
    case "invalid_input":
      return message;
    case "no_audio_stream":
      return "这个视频没有可用的 DASH 音频流";
    case "auth_required":
      return "B 站拒绝了音频下载，可能需要登录或该内容受限";
    case "link_expired":
      return "B 站音频链接已失效，请重新导入";
    case "timeout":
      return "连接 B 站超时，请稍后重试";
    default:
      return message && message !== "[object Object]" ? `导入 B 站音频失败：${message}` : "导入 B 站音频失败";
  }
}

function newTaskId() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

/**
 * 把后端的批量进度事件并入 store 里的进度。只在导入中（`current` 非 null）且任务一致时
 * 更新——结果落定清空后，滞后到达的事件不能把进度复活（返回 null 表示忽略）。
 * 事件里没有 `kind`（后端不区分收藏夹与分 P），沿用发起时记下的。
 */
export function mergeBatchProgress(
  current: BilibiliBatchProgress | null,
  incoming: BilibiliBatchProgress
): BilibiliBatchProgress | null {
  if (!current || (current.taskId && incoming.taskId !== current.taskId)) return null;
  return { ...incoming, kind: current.kind };
}

export function createBilibiliActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<
  PlayerStore,
  | "probeBilibiliVideo"
  | "importBilibiliAudio"
  | "importBilibiliFavorites"
  | "cancelBilibiliFavoritesImport"
  | "reloadStreamingTrack"
> {
  return {
  // BILI-05：导入前先看视频有几个分 P——多 P 又没指定分 P 时，页面要先问导入范围。
  probeBilibiliVideo: async (input) => {
    const cleanInput = input.trim();
    if (!cleanInput) {
      get().showNotification("请输入 B 站视频链接或 BV 号");
      return null;
    }
    try {
      const info = await invoke<BilibiliVideoInfo>("bilibili_video_info", { input: cleanInput });
      if (!info || !Array.isArray(info.pages) || info.pages.length === 0) {
        get().showNotification("没有解析到可用的 B 站音频");
        return null;
      }
      return info;
    } catch (err) {
      console.warn("Tauri command failed: bilibili_video_info", err);
      get().showNotification(bilibiliImportErrorMessage(err), "error");
      return null;
    }
  },

  importBilibiliAudio: async (input, options) => {
    // 审2-R10：返回是否导入成功，调用方（StreamingPage）据此决定是否清空输入框（L-7）
    const cleanInput = input.trim();
    if (!cleanInput) {
      get().showNotification("请输入 B 站视频链接或 BV 号");
      return false;
    }

    // BILI-05：全量导入多个分 P 是一次批量——后端占批量槽位、逐 P 推进度、可取消。
    // 只出一首的导入（单 P、指定了分 P、只要首 P）不占槽位，也没有进度事件。
    const batch = options?.multiPage === "all";
    if (batch && get().bilibiliBatchProgress) {
      get().showNotification("已有批量导入正在进行，请等待完成或取消");
      return false;
    }
    const taskId = newTaskId();
    if (batch) {
      set({
        bilibiliBatchProgress: { taskId, kind: "video", current: 0, total: 0, title: "", ok: true },
      });
    }

    try {
      const result = await invoke<BilibiliBatchImportResult>("import_bilibili_video", {
        taskId,
        input: cleanInput,
        options,
      });
      const tracks = (Array.isArray(result?.tracks) ? result.tracks : []).filter(
        (track) => Boolean(track?.path)
      );
      const failed = Array.isArray(result?.failed) ? result.failed : [];
      const cancelled = result?.cancelled === true;

      if (tracks.length === 0) {
        if (cancelled) {
          get().showNotification("导入已取消");
        } else if (failed.length > 0) {
          get().showNotification(`导入 B 站音频失败：${failed[0].reason}`, "error");
        } else {
          get().showNotification("没有解析到可用的 B 站音频");
        }
        return false;
      }

      // 按来源身份并入：已有同一来源的记录原位更新（保留 id / 收藏 / 歌单归属），其余追加。
      // 各分 P 的来源 ID 互不相同（第 1 P 是 BV 号，其余是 BV…-pN），不会并成一首。
      const previousLength = get().playlist.length;
      const stats = mergeTracksByPathWithStats(get().playlist, tracks);
      set({
        playlist: stats.playlist,
        currentTrackIndex: previousLength === 0 ? 0 : get().currentTrackIndex,
        activeView: "streaming",
      });

      if (tracks.length === 1 && failed.length === 0 && !cancelled) {
        get().showNotification(
          stats.addedCount > 0
            ? `已添加 B 站音频: ${tracks[0].title}`
            : `已更新 B 站音频: ${tracks[0].title}`
        );
      } else {
        const summary = `新增 ${stats.addedCount} 首，更新 ${stats.updatedCount} 首，失败 ${failed.length} 首`;
        get().showNotification(
          cancelled ? `分 P 导入已取消：${summary}` : `分 P 导入完成：${summary}`
        );
      }
      return true;
    } catch (err) {

      console.warn("Tauri command failed: import_bilibili_video", err);
      get().showNotification(bilibiliImportErrorMessage(err), "error");
      return false;
    } finally {
      if (batch) set({ bilibiliBatchProgress: null });
    }
  },

  importBilibiliFavorites: async (input, options) => {
    if (get().bilibiliBatchProgress) return null;
    const cleanInput = input.trim();
    if (!cleanInput) {
      get().showNotification("请输入 B 站收藏夹链接、media_id 或 fid");
      return null;
    }

    // M-15：立刻进入“导入中”态（total 未知先置 0），真实进度由
    // seraph://bilibili-batch 事件写入；结束/异常都在 finally 清理。
    const taskId = newTaskId();
    set({
      bilibiliBatchProgress: { taskId, kind: "favorites", current: 0, total: 0, title: "", ok: true },
    });
    try {
      const result = await invoke<BilibiliBatchImportResult>("import_bilibili_favorites", {
        taskId,
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
        const summary = `新增 ${stats.addedCount} 首，更新 ${stats.updatedCount} 首，失败 ${failed.length} 首`;
        get().showNotification(
          result.cancelled
            ? `收藏夹导入已取消：${summary}`
            : `收藏夹导入完成：${summary}`
        );
      } else if (result.cancelled) {
        get().showNotification("收藏夹导入已取消");
      } else {
        get().showNotification(
          failed.length > 0
            ? `收藏夹导入失败：${failed[0].reason}`
            : "收藏夹里没有可导入的音频"
        );
      }

      return { tracks, failed, cancelled: result.cancelled };
    } catch (err) {

      console.warn("Tauri command failed: import_bilibili_favorites", err);
      get().showNotification(bilibiliImportErrorMessage(err), "error");
      return null;
    } finally {
      set({ bilibiliBatchProgress: null });
    }
  },

  cancelBilibiliFavoritesImport: async () => {
    const progress = get().bilibiliBatchProgress;
    if (!progress) return;
    try {
      await invoke("cancel_bilibili_favorites_import", { taskId: progress.taskId });
      get().showNotification(
        progress.kind === "video" ? "正在取消分 P 导入…" : "正在取消收藏夹导入…"
      );
    } catch (err) {

      console.warn("Tauri command failed: cancel_bilibili_favorites_import", err);
    }
  },

  // v0.4.4：B 站流媒体曲目「重新加载」——用用户当次勾选的音质选项
  // （杜比 / FLAC / 混流，默认全不勾选）重新解析下载，原位替换曲库中的记录。
  reloadStreamingTrack: async (trackId, options) => {
    const track = get().playlist.find((item) => item.id === trackId);
    if (!track) {
      get().showNotification("找不到要重新加载的曲目");
      return false;
    }

    const sourceInput = streamingSourceInput(track);
    if (!sourceInput) {
      get().showNotification("这首曲目缺少 B 站来源信息，无法重新加载");
      return false;
    }

    const reloadOptions: BilibiliImportOptions = {
      preferFlac: options?.preferFlac ?? false,
      preferDolbyAtmos: options?.preferDolbyAtmos ?? false,
      remuxWithFfmpeg: options?.remuxWithFfmpeg ?? false,
      existingTrackId: trackId,
    };

    get().showNotification(`正在重新加载: ${track.title}`);
    try {
      const imported = await invoke<Track>("import_bilibili_audio_with_options", {
        input: sourceInput,
        options: reloadOptions,
      });

      if (!imported?.path) {
        get().showNotification("没有解析到可用的 B 站音频");
        return false;
      }

      // 用来源身份（mergeKey）定位并原位替换，保留原 id / 收藏 / 歌单归属。
      const incomingKey = trackMergeKey(imported);
      set((state) => ({
        playlist: state.playlist.map((item) =>
          item.id === trackId || trackMergeKey(item) === incomingKey
            ? mergeIncomingTrack(item, imported)
            : item
        ),
      }));

      get().showNotification(`已重新加载: ${imported.title}`);
      return true;
    } catch (err) {

      console.warn("Tauri command failed: reload streaming track", err);
      get().showNotification(bilibiliImportErrorMessage(err), "error");
      return false;
    }
  },
  };
}

