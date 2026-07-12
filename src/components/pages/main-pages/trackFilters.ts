import type { Track } from "@/types/track";

export function isStreamingTrack(track: Track) {
  return (
    track.id.startsWith("bilibili-") ||
    track.sourceId?.trim().toLowerCase().startsWith("bv") ||
    track.sourceUrl?.trim().toLowerCase().includes("bilibili.com") ||
    track.album === "Bilibili"
  );
}

export function isLocalTrack(track: Track) {
  return !isStreamingTrack(track);
}

export type TrackSortKey = "default" | "title" | "artist" | "album" | "duration";

export const TRACK_SORT_OPTIONS: { value: TrackSortKey; label: string }[] = [
  { value: "default", label: "默认顺序" },
  { value: "title", label: "标题" },
  { value: "artist", label: "艺术家" },
  { value: "album", label: "专辑" },
  { value: "duration", label: "时长" },
];

/** 归一化查询词：去空白、转小写，便于大小写/空格不敏感匹配。 */
function normalizeQuery(query: string) {
  return query.trim().toLowerCase();
}

function matchesQuery(track: Track, needle: string) {
  return (
    track.title.toLowerCase().includes(needle) ||
    track.artist.toLowerCase().includes(needle) ||
    track.album.toLowerCase().includes(needle)
  );
}

/**
 * 按搜索词过滤 + 按指定键排序。
 * - 搜索匹配标题/艺术家/专辑，大小写与首尾空格不敏感；空词不过滤。
 * - "default" 保持传入顺序（即播放队列顺序），其它键用 localeCompare
 *   （数字用数值序），保证中文/日文按本地化规则排序。
 * - 排序稳定：非默认键返回新数组，不修改入参。
 */
export function filterAndSortTracks(
  tracks: Track[],
  query: string,
  sortKey: TrackSortKey
): Track[] {
  const needle = normalizeQuery(query);
  const filtered = needle
    ? tracks.filter((track) => matchesQuery(track, needle))
    : tracks;

  if (sortKey === "default") {
    // 过滤后若未变则原样返回，避免无谓复制
    return needle ? filtered : tracks;
  }

  const collator = new Intl.Collator(undefined, {
    numeric: true,
    sensitivity: "base",
  });
  const sorted = [...filtered];
  sorted.sort((a, b) => {
    if (sortKey === "duration") return a.duration - b.duration;
    return collator.compare(a[sortKey], b[sortKey]);
  });
  return sorted;
}
