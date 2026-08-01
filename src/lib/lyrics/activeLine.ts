import type { LyricLine } from "@/types/track";

/**
 * 歌词分组与当前行定位纯函数。
 *
 * 从 LyricsPanel 提炼,主窗口歌词稿与任务栏歌词条共用同一口径:
 * 相同时间戳(±epsilon)的多行(原文/译文)归为一组,按播放时间二分
 * 定位当前组。
 */

export interface LyricGroup {
  time: number;
  lines: LyricLine[];
}

export const SAME_TIMESTAMP_EPSILON = 0.01;

/** 相邻且时间戳相同(±epsilon)的歌词行归为一组(双语/多行歌词)。 */
export function groupLyricsByTime(lyrics: LyricLine[]): LyricGroup[] {
  const groups: LyricGroup[] = [];

  for (const line of lyrics) {
    const previous = groups[groups.length - 1];
    if (
      previous &&
      Math.abs(previous.time - line.time) <= SAME_TIMESTAMP_EPSILON
    ) {
      previous.lines.push(line);
      continue;
    }

    groups.push({ time: line.time, lines: [line] });
  }

  return groups;
}

/**
 * 二分定位当前播放时间对应的歌词组下标;尚未到达第一句时返回 -1。
 * 要求 groups 按时间升序(LRC 解析后的自然顺序)。
 */
export function activeGroupIndex(
  groups: LyricGroup[],
  currentTime: number
): number {
  let low = 0;
  let high = groups.length - 1;
  let match = -1;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if (currentTime + SAME_TIMESTAMP_EPSILON >= groups[mid].time) {
      match = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }

  return match;
}
