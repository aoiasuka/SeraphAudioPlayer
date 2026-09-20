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
