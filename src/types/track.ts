/** 逐字歌词的一个音节（秒）。 */
export interface LyricWord {
  start: number;
  end: number;
  text: string;
}

export interface LyricLine {
  time: number;
  text: string;
  /** 行结束时间（秒），仅逐字来源提供。 */
  end?: number;
  /** 逐字时间轴（AMLL TTML）；缺省按整行处理。 */
  words?: LyricWord[];
  /** 译文字段（TTML）。LRC 类来源的译文仍是相邻同时间戳行。 */
  translation?: string;
  /** 音译 / 罗马音（TTML）。 */
  roman?: string;
  /** 被歌词排除规则命中（后端打标，仅显示层使用）。 */
  hidden?: boolean;
}

export type LyricsSourcePriority = "auto" | "netease" | "kugou" | "qq";

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
  lyrics: LyricLine[];
  /** AMLL TTML 候选携带的查找键（如 `ncm-lyrics/123`），应用后回写进曲目 `lyricsLookupKeys` */
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
  lyrics: LyricLine[];
  /** false 表示来自曲库摘要，当前曲目需要再按 ID 读取歌词。 */
  lyricsLoaded?: boolean;
  /** AMLL TTML DB 查找键（如 `ncm-lyrics/65923804`），来自本地歌词文件名等可靠来源。 */
  lyricsLookupKeys?: string[];
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
