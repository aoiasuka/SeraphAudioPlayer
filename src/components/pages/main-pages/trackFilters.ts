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

// 曲目与列表按 store 的不可变引用失效；旧曲库被释放后缓存也可回收。
const searchFields = new WeakMap<Track, string[]>();
const sortedLists = new WeakMap<Track[], Map<TrackSortKey, Track[]>>();
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function matchesQuery(track: Track, needle: string) {
  let fields = searchFields.get(track);
  if (!fields) {
    fields = [track.title.toLowerCase(), track.artist.toLowerCase(), track.album.toLowerCase()];
    searchFields.set(track, fields);
  }
  return fields.some((field) => field.includes(needle));
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
  let ordered = tracks;
  if (sortKey !== "default") {
    let cached = sortedLists.get(tracks);
    if (!cached) {
      cached = new Map();
      sortedLists.set(tracks, cached);
    }
    const existing = cached.get(sortKey);
    if (existing) ordered = existing;
    else {
      ordered = [...tracks].sort((a, b) => sortKey === "duration"
        ? a.duration - b.duration : collator.compare(a[sortKey], b[sortKey]));
      cached.set(sortKey, ordered);
    }
  }
  return needle ? ordered.filter((track) => matchesQuery(track, needle)) : ordered;
}
