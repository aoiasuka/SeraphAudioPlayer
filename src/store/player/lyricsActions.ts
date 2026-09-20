import { invoke, normalizeIpcError } from "@/lib/tauri";
import { sanitizeExcludeRules } from "@/lib/lyrics/exclude";
import { isValidAmllTtmlDbUrl, normalizeAmllTtmlDbUrl } from "@/lib/lyrics/settings";
import type {
  LrcExportFormat,
  LyricDocument,
  LyricsDisplayOptions,
  OnlineLyricsCandidate,
  Track,
} from "@/types/track";
import { sendCommand } from "./commands";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

/** `find_local_lyrics` 的返回。 */
interface LocalLyricsMatch {
  path: string;
  lyrics: LyricDocument;
  lookupKeys: string[];
}

const MAX_LYRIC_FILE_BYTES = 2 * 1024 * 1024;

const LRC_EXPORT_FORMAT_LABEL: Record<LrcExportFormat, string> = {
  enhanced: "增强型 LRC",
  verbatim: "逐字 LRC",
  line: "逐行 LRC",
};

/** store 里两个显示选项 → 后端 `set_lyrics_display_options` 的参数。 */
export function lyricsDisplayOptionsOf(
  state: Pick<PlayerStore, "showLyricsCredits" | "ignoreLyricsFileOffset">
): LyricsDisplayOptions {
  return {
    ignoreFileOffset: state.ignoreLyricsFileOffset,
    showCredits: state.showLyricsCredits,
  };
}

/** 导出文件名：`艺术家 - 曲名.lrc`，去掉文件系统不允许的字符。 */
function lyricsExportFileName(track: Track) {
  const stem = [track.artist, track.title]
    .map((part) => part?.trim() ?? "")
    .filter(Boolean)
    .join(" - ")
    .replace(/[\\/:*?"<>|\r\n]+/g, " ")
    .trim();
  return `${stem || "lyrics"}.lrc`;
}

function replaceTrackLyrics(
  playlist: Track[],
  trackId: string,
  lyrics: LyricDocument
) {
  return playlist.map((track) =>
    track.id === trackId ? { ...track, lyrics, lyricsLoaded: true } : track
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
): Pick<
  PlayerStore,
  | "importLyricsForCurrentTrack"
  | "fetchOnlineLyricsForCurrentTrack"
  | "applyOnlineLyricsForCurrentTrack"
  | "exportLyricsForCurrentTrack"
  | "setLyricsSourcePriority"
  | "setPreferTraditionalLyrics"
  | "setTtmlLyricsEnabled"
  | "setAmllTtmlDbUrl"
  | "setLyricsExcludeRules"
  | "setShowLyricsTranslation"
  | "setShowLyricsRoman"
  | "setLyricsFolder"
  | "setShowLyricsCredits"
  | "setIgnoreLyricsFileOffset"
  | "setCurrentTrackLyricsPinned"
  | "findLocalLyricsForTrack"
> {
  return {
  setLyricsFolder: (folder) => {
    const normalized = folder.trim();
    if (get().lyricsFolder === normalized) return;
    set({ lyricsFolder: normalized });
    get().showNotification(
      normalized ? "已设置本地歌词目录，切歌时自动匹配" : "已清除本地歌词目录"
    );
  },

  findLocalLyricsForTrack: async (track) => {
    const folder = get().lyricsFolder;
    if (!folder) return false;
    try {
      const result = await invoke<LocalLyricsMatch | null>("find_local_lyrics", {
        trackId: track.id,
        trackPath: track.path,
        title: track.title,
        artist: track.artist,
        folder,
        preferTraditional: get().preferTraditionalLyrics,
      });
      if (!result || (result.lyrics?.lines.length ?? 0) === 0) return false;
      // 后端返回的文档已带并入的查找键
      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, result.lyrics),
      }));
      return true;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: find_local_lyrics", err);
      return false;
    }
  },

  setLyricsSourcePriority: (priority) => {
    if (get().lyricsSourcePriority === priority) return;
    set({ lyricsSourcePriority: priority });
  },

  setPreferTraditionalLyrics: (enabled) => {
    if (get().preferTraditionalLyrics === enabled) return;
    set({ preferTraditionalLyrics: enabled });
    get().showNotification(
      enabled ? "新获取的歌词将转换为繁体中文" : "已关闭繁体中文转换"
    );
  },

  setTtmlLyricsEnabled: (enabled) => {
    if (get().ttmlLyricsEnabled === enabled) return;
    set({ ttmlLyricsEnabled: enabled });
  },

  setAmllTtmlDbUrl: (url, custom) => {
    const normalized = normalizeAmllTtmlDbUrl(url);
    if (!isValidAmllTtmlDbUrl(normalized, custom)) {
      get().showNotification(
        custom
          ? "地址无效：需为公网 HTTPS 域名（不接受 IP 直连、localhost 或内网主机）"
          : "地址不在预设镜像列表内（仅 amlldb.bikonoo.com）；如需其它域名请切换到自定义模式"
      );
      return false;
    }
    if (get().amllTtmlDbUrl !== normalized || get().amllTtmlDbCustom !== custom) {
      set({ amllTtmlDbUrl: normalized, amllTtmlDbCustom: custom });
    }
    return true;
  },

  setLyricsExcludeRules: (rules) => {
    const sanitized = sanitizeExcludeRules(rules);
    set({ lyricsExcludeRules: sanitized });
    // 匹配在 Rust 侧：同步规则，后端会广播 seraph://lyrics-rules-updated 让各窗口重拉歌词
    sendCommand("set_lyrics_exclude_rules", { rules: sanitized });
  },

  setShowLyricsTranslation: (enabled) => {
    if (get().showLyricsTranslation === enabled) return;
    set({ showLyricsTranslation: enabled });
  },

  setShowLyricsRoman: (enabled) => {
    if (get().showLyricsRoman === enabled) return;
    set({ showLyricsRoman: enabled });
  },

  setShowLyricsCredits: (enabled) => {
    if (get().showLyricsCredits === enabled) return;
    set({ showLyricsCredits: enabled });
    // 投影在 Rust 侧（回传前按 hidden 打标），后端变更后广播让主窗口与任务栏重拉
    sendCommand("set_lyrics_display_options", { options: lyricsDisplayOptionsOf(get()) });
  },

  setIgnoreLyricsFileOffset: (enabled) => {
    if (get().ignoreLyricsFileOffset === enabled) return;
    set({ ignoreLyricsFileOffset: enabled });
    sendCommand("set_lyrics_display_options", { options: lyricsDisplayOptionsOf(get()) });
  },

  setCurrentTrackLyricsPinned: async (pinned) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return false;
    }
    if ((track.lyrics?.lines.length ?? 0) === 0) {
      get().showNotification("当前曲目没有歌词");
      return false;
    }
    try {
      const lyrics = await invoke<LyricDocument>("set_track_lyrics_pinned", {
        trackId: track.id,
        pinned,
      });
      if (!lyrics || !Array.isArray(lyrics.lines)) return false;
      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, lyrics),
      }));
      get().showNotification(
        pinned ? "已固定这份歌词，自动匹配不会替换它" : "已取消固定，重新导入或目录匹配可替换这份歌词"
      );
      return true;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: set_track_lyrics_pinned", err);
      get().showNotification(`修改歌词固定状态失败：${normalizeIpcError(err).message}`);
      return false;
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
      const lyrics = await invoke<LyricDocument>("save_track_lyrics", {
        trackId: track.id,
        trackPath: track.path,
        lyricsBytes,
        preferTraditional: get().preferTraditionalLyrics,
      });

      if (!lyrics || !Array.isArray(lyrics.lines) || lyrics.lines.length === 0) {
        get().showNotification("歌词文件没有可用内容");
        return;
      }

      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, lyrics),
      }));
      get().showNotification(`已导入 ${lyrics.lines.length} 行歌词`);
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
    const {
      lyricsSourcePriority,
      preferTraditionalLyrics,
      ttmlLyricsEnabled,
      amllTtmlDbUrl,
      amllTtmlDbCustom,
    } = get();

    try {
      const candidates = await invoke<OnlineLyricsCandidate[]>(
        "fetch_online_lyrics",
        {
          trackId: track.id,
          title: manualQuery || track.title,
          artist: manualQuery ? "" : track.artist,
          duration: track.duration,
          options: {
            sourcePriority: lyricsSourcePriority,
            preferTraditional: preferTraditionalLyrics,
            ttmlEnabled: ttmlLyricsEnabled,
            ttmlDbUrl: amllTtmlDbUrl,
            ttmlDbCustom: amllTtmlDbCustom,
            lookupKeys: track.lyrics?.source?.lookupKeys ?? [],
          },
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

  applyOnlineLyricsForCurrentTrack: async (lyrics, lookupKeys) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return false;
    }

    if (!lyrics || lyrics.lines.length === 0) {
      get().showNotification("歌词内容为空");
      return false;
    }

    const keys = (lookupKeys ?? []).filter((key) => typeof key === "string" && key);
    try {
      const savedLyrics = await invoke<LyricDocument>("apply_online_lyrics", {
        trackId: track.id,
        trackPath: track.path,
        lyrics,
        lookupKeys: keys,
      });

      if (!savedLyrics || !Array.isArray(savedLyrics.lines) || savedLyrics.lines.length === 0) {
        get().showNotification("歌词内容为空");
        return false;
      }

      // 后端返回的文档已把查找键并进来源，下次在线匹配直接带上
      set((state) => ({
        playlist: replaceTrackLyrics(state.playlist, track.id, savedLyrics),
      }));
      get().showNotification(`已应用 ${savedLyrics.lines.length} 行在线歌词`);
      return true;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: apply_online_lyrics", err);
      get().showNotification(onlineLyricsErrorMessage(err));
      return false;
    }
  },

  exportLyricsForCurrentTrack: async (format) => {
    const track = get().currentTrack();
    if (!track) {
      get().showNotification("请先选择曲目");
      return false;
    }
    if ((track.lyrics?.lines.length ?? 0) === 0) {
      get().showNotification("当前曲目没有歌词可导出");
      return false;
    }

    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        defaultPath: lyricsExportFileName(track),
        filters: [{ name: "LRC 歌词", extensions: ["lrc"] }],
      });
      if (!target) return false;

      // 导出的是曲库原始歌词（后端读取），不受排除规则与显示选项影响
      const written = await invoke<number>("export_track_lyrics", {
        trackId: track.id,
        path: target,
        format,
        options: {
          msDigits: 3,
          includeTranslation: get().showLyricsTranslation,
          includeRoman: get().showLyricsRoman,
        },
      });
      get().showNotification(
        `已导出 ${LRC_EXPORT_FORMAT_LABEL[format]}（${written} 行）`
      );
      return true;
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: export_track_lyrics", err);
      get().showNotification(`导出歌词失败：${normalizeIpcError(err).message}`);
      return false;
    }
  },
  };
}

