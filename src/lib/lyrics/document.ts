import type { LyricDocument, LyricLine, LyricSource, Track } from "@/types/track";

/** 当前歌词文档格式版本（与 Rust `LYRIC_DOCUMENT_SCHEMA` 一致）。 */
export const LYRIC_DOCUMENT_SCHEMA = 2;

/** 空歌词文档（曲目没有歌词 / 曲库摘要）。每次新建，避免共享可变对象。 */
export function emptyLyricDocument(): LyricDocument {
  return { schema: LYRIC_DOCUMENT_SCHEMA, source: { kind: "unknown" }, sync: "none", offsetMs: 0, lines: [] };
}

/** 由行构造文档（前端只在测试与本地占位时用；同步粒度按是否有逐字判定）。 */
export function lyricDocument(lines: LyricLine[], source: Partial<LyricSource> = {}): LyricDocument {
  const sync = lines.some((line) => line.words && line.words.length > 0) ? "word" : lines.length ? "line" : "none";
  return { schema: LYRIC_DOCUMENT_SCHEMA, source: { kind: "unknown", ...source }, sync, offsetMs: 0, lines };
}

/** 曲目的歌词行（缺省空数组，兼容旧摘要里 lyrics 缺失的情况）。 */
export function lyricLines(track: Pick<Track, "lyrics"> | null | undefined): LyricLine[] {
  return track?.lyrics?.lines ?? [];
}

/** 曲目是否已有歌词。 */
export function hasLyrics(track: Pick<Track, "lyrics"> | null | undefined): boolean {
  return lyricLines(track).length > 0;
}

const SOURCE_KIND_LABEL: Record<LyricSource["kind"], string> = {
  unknown: "来源未知",
  embedded: "音频内嵌",
  sidecar: "同名歌词文件",
  folder: "本地歌词目录",
  manual: "手动导入",
  online: "在线匹配",
  ttml: "AMLL TTML",
  legacy: "旧版曲库",
};

const PROVIDER_LABEL: Record<string, string> = {
  netease: "网易云音乐",
  kugou: "酷狗音乐",
  qq: "QQ 音乐",
  amll: "AMLL",
};

/** 来源的人类可读说明，如「在线匹配（网易云音乐）」；曲目信息弹窗与歌词稿菜单用。 */
export function describeLyricSource(source: LyricSource | null | undefined): string {
  if (!source) return SOURCE_KIND_LABEL.unknown;
  const base = SOURCE_KIND_LABEL[source.kind] ?? SOURCE_KIND_LABEL.unknown;
  const provider = source.provider ? PROVIDER_LABEL[source.provider] ?? source.provider : "";
  return provider && source.kind !== "ttml" ? `${base}（${provider}）` : base;
}
