import type { Track } from "@/types/track";

/** 同一个 playlist 数组引用只建一次索引（WeakMap 随数组回收）。 */
const indexCache = new WeakMap<readonly Track[], Map<string, number>>();

/**
 * trackId → 在曲库（播放队列）中的下标。PERF-05：曲目列表、歌单详情、最近播放
 * 与「下一首」预览此前各自建一份或线性查找；按数组引用缓存后共享一份 O(N) 索引，
 * 后续查找 O(1)。playlist 是不可变更新（每次变更换新数组），引用即版本。
 */
export function trackIndexById(playlist: readonly Track[]): Map<string, number> {
  let index = indexCache.get(playlist);
  if (!index) {
    index = new Map();
    for (let i = 0; i < playlist.length; i += 1) {
      // 与 find 语义一致：重复 id 取第一个
      if (!index.has(playlist[i].id)) index.set(playlist[i].id, i);
    }
    indexCache.set(playlist, index);
  }
  return index;
}

/** 按 id 取曲目；不存在返回 undefined。 */
export function trackById(playlist: readonly Track[], id: string | null | undefined): Track | undefined {
  if (!id) return undefined;
  const index = trackIndexById(playlist).get(id);
  return index === undefined ? undefined : playlist[index];
}
