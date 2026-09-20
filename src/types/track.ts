/** 逐字歌词的一个音节（毫秒整数）。`endMs` 缺省 = 终点未知，渲染时用下一音节起点或行终点兜底。 */
export interface LyricWord {
  startMs: number;
  endMs?: number;
  text: string;
  /** 逐字音译（暂只存不渲染）。 */
  roman?: string;
}

/** 译文 / 音译这类副轨文本，可带自己的逐字时间轴与语言标记。 */
export interface LyricText {
  text: string;
  words?: LyricWord[];
  lang?: string;
}

export type LyricRole = "main" | "background" | "credit";

export interface LyricLine {
  startMs: number;
  /** 行结束时间（毫秒）；缺省 = 来源没给。 */
  endMs?: number;
  /** `endMs` 是解析后处理用下一句起点推导的。 */
  endInferred?: boolean;
  text: string;
  /** 逐字时间轴；缺省按整行处理。 */
  words?: LyricWord[];
  /** 译文（TTML / KRC / 网易云 / QQ 都进这里）。来源不明的双语 LRC 仍是相邻同起点的两行。 */
  translations?: LyricText[];
  /** 音译 / 罗马音。 */
  roman?: LyricText;
  /**
   * 行角色（缺省 main）：和声（TTML `x-bg`，挂在所在主句下渲染）/ 制作信息（自成一组，
   * 「显示制作信息」关闭时由后端按 `hidden` 打标）。
   */
  role?: LyricRole;
  /** TTML 对唱 agent；同起点、agent 不同的两条主句是对唱而不是双语。 */
  agent?: string;
  /** 被歌词排除规则命中或被显示选项隐藏（后端打标，仅显示层使用）。 */
  hidden?: boolean;
}

export type LyricSourceKind =
  | "unknown"
  | "embedded"
  | "sidecar"
  | "folder"
  | "manual"
  | "online"
  | "ttml"
  | "legacy";

export interface LyricSource {
  kind: LyricSourceKind;
  provider?: string;
  providerTrackId?: string;
  /** AMLL TTML DB 查找键（如 `ncm-lyrics/65923804`）。 */
  lookupKeys?: string[];
  /** 用户手动导入 / 明确应用 = 固定选择：重新导入与歌词目录匹配不替换；歌词稿右键菜单可切换。 */
  pinned?: boolean;
  fetchedAt?: number;
}

export type LyricSync = "none" | "line" | "word";

/** 一首歌的歌词文档：行 + 来源 + 同步粒度（与 Rust `LyricDocument` 同形）。 */
export interface LyricDocument {
  schema: number;
  source: LyricSource;
  sync: LyricSync;
  /** 解析时折进各行时间的文件 `[offset:]`（毫秒）；「忽略文件 offset」开启时后端回传前已还原。 */
  offsetMs: number;
  lines: LyricLine[];
}

/** 歌词显示选项（后端回传前投影，主窗口与任务栏一致；与 Rust `LyricsDisplayOptions` 同形）。 */
export interface LyricsDisplayOptions {
  ignoreFileOffset: boolean;
  showCredits: boolean;
}

export type LyricsSourcePriority = "auto" | "netease" | "kugou" | "qq";

/** `export_track_lyrics` 的目标格式：增强型（ESLyric `<t>`）/ 逐字（`[t]`）/ 逐行。 */
export type LrcExportFormat = "enhanced" | "verbatim" | "line";

/** 歌词排除规则：命中的歌词行不显示（仅显示层过滤，不改曲库数据）。 */
export interface LyricsExcludeRule {
  id: string;
  kind: "keyword" | "regex";
  pattern: string;
}

export interface OnlineLyricsCandidate {
  id: string;
  source: string;
  title: string;
  artist: string;
  album?: string | null;
  duration?: number | null;
  lyrics: LyricDocument;
  /** AMLL TTML 候选携带的查找键（如 `ncm-lyrics/123`），应用后并入曲目歌词文档的来源 */
  lookupKeys?: string[];
}

export interface Track {
  id: string;
  title: string;
  artist: string;
  album: string;
  albumYear?: string;
  cover: string;
  format: string;
  bitdepth: string;
  sampleRate?: string;
  bitrate: string;
  channels: string;
  size: string;
  path: string;
  sourceUrl?: string | null;
  sourceId?: string | null;
  cacheMissing?: boolean;
  duration: number;
  glowColor: string;
  glow1?: string;
  glow2?: string;
  /** 歌词文档；曲库摘要里是空文档，当前曲目按 ID 读取后才完整。 */
  lyrics: LyricDocument;
  /** false 表示来自曲库摘要，当前曲目需要再按 ID 读取歌词。 */
  lyricsLoaded?: boolean;
}

export interface DeleteTrackFailure {
  id: string;
  title: string;
  message: string;
}

export interface DeleteTracksResult {
  deletedIds: string[];
  deletedFiles: number;
  failures: DeleteTrackFailure[];
}

export interface OutputDevice {
  id: string;
  name: string;
  isDefault: boolean;
  legacyIds?: string[];
}

export interface UserPlaylist {
  id: string;
  name: string;
  trackIds: string[];
  createdAt: number;
}

export type DriverKind = "wasapi" | "asio" | "direct";

export type LibraryView =
  | "local"
  | "streaming"
  | "recent"
  | "liked"
  | "playlists"
  | "artists"
  | "albums"
  | "eq"
  | "analysis";
