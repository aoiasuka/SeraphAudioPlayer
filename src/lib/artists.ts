import type { Track } from "@/types/track";

/**
 * 艺术家页的分组、排序与筛选。
 *
 * 曲目的 `artist` 是标签里的原始字符串，合作曲目常写成「A / B」「A、B」
 * 「A feat. B」。按完整字符串分组会把同一位艺术家拆成很多条，这里拆开后各自归组，
 * 合作曲目同时出现在每位参与者名下。
 *
 * 只按不会出现在艺术家本名里的分隔符拆：`/`、`、`、`;` 与 feat. 标记。
 * **不拆 `&` 和逗号**——「Simon & Garfunkel」「Earth, Wind & Fire」这类名字很常见，
 * 拆错比不拆更糟。`AC/DC` 这种斜杠两侧都是短大写词的写法也保持完整。
 */

export interface ArtistGroup {
  /** 归一化后的分组键（小写、空白压缩） */
  key: string;
  /** 展示名：曲库里第一次出现的写法 */
  name: string;
  tracks: Track[];
}

// 私用区字符作临时标记：不会出现在标签文本里，也不是控制字符
const SLASH_GUARD = String.fromCharCode(0xe001);
const SPLIT_MARK = String.fromCharCode(0xe000);
const SEPARATORS = new RegExp(`[${SPLIT_MARK}、;；/／]`);
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

function artistKey(name: string) {
  return name.trim().replace(/\s+/g, " ").toLocaleLowerCase();
}

/** 把标签里的艺术家字符串拆成各位艺术家的名字（去重、保序）。 */
export function splitArtistNames(raw: string): string[] {
  const marked = raw
    // AC/DC：斜杠两侧都是 1–3 位大写字母或数字、且紧贴斜杠，视为名字的一部分
    .replace(/\b([A-Z0-9]{1,3})\/([A-Z0-9]{1,3})\b/g, `$1${SLASH_GUARD}$2`)
    // 「曲名风格」的括号合作标记：A (feat. B) / A（ft. B）
    .replace(/\s*[(（[]\s*(?:feat\.?|ft\.?|featuring)\s+([^)）\]]*)[)）\]]/gi, `${SPLIT_MARK}$1`)
    .replace(/\s+(?:feat\.?|ft\.?|featuring)\s+/gi, SPLIT_MARK);
  const names: string[] = [];
  const seen = new Set<string>();
  for (const part of marked.split(SEPARATORS)) {
    const name = part.split(SLASH_GUARD).join("/").trim().replace(/\s+/g, " ");
    const key = artistKey(name);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    names.push(name);
  }
  if (names.length > 0) return names;
  const fallback = raw.trim();
  return [fallback];
}

/** 按艺术家分组并按名称排序（本地化排序，数字按数值）。 */
export function groupTracksByArtist(tracks: Track[]): ArtistGroup[] {
  const groups = new Map<string, ArtistGroup>();
  for (const track of tracks) {
    for (const name of splitArtistNames(track.artist ?? "")) {
      const key = artistKey(name);
      const group = groups.get(key);
      if (group) group.tracks.push(track);
      else groups.set(key, { key, name, tracks: [track] });
    }
  }
  return Array.from(groups.values()).sort((a, b) => collator.compare(a.name, b.name));
}

/** 按名称筛选（大小写与首尾空格不敏感）；空词返回原数组引用。 */
export function filterArtistGroups(groups: ArtistGroup[], query: string): ArtistGroup[] {
  const needle = query.trim().toLocaleLowerCase();
  return needle ? groups.filter((group) => group.key.includes(needle)) : groups;
}
