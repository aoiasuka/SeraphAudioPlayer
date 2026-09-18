import type { LyricLine, LyricWord } from "@/types/track";

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

/**
 * 全部行分组与可见行分组的对照。
 *
 * 排除规则命中的行由后端打 `hidden` 标记。若先过滤再分组，被隐藏句的时间区间
 * 会被并入上一句（上一句"延长"到下一可见句开始），所以这里保留全量分组做定位，
 * 只用可见分组做渲染与点击 seek。
 */
export interface ResolvedLyricGroups {
  /** 按全部行（含 hidden）分组 */
  all: LyricGroup[];
  /** 每组剔除 hidden 行后仍非空的组；组内 lines 只保留可见行 */
  visible: LyricGroup[];
  /** all 的第 i 组在 visible 里的下标；整组隐藏时为 -1 */
  visibleIndexOfAll: number[];
}

export function resolveVisibleGroups(lyrics: LyricLine[]): ResolvedLyricGroups {
  const all = groupLyricsByTime(lyrics);
  const visible: LyricGroup[] = [];
  const visibleIndexOfAll: number[] = [];
  for (const group of all) {
    const lines = group.lines.filter((line) => !line.hidden);
    if (lines.length === 0) {
      visibleIndexOfAll.push(-1);
      continue;
    }
    visibleIndexOfAll.push(visible.length);
    visible.push(lines.length === group.lines.length ? group : { time: group.time, lines });
  }
  return { all, visible, visibleIndexOfAll };
}

/**
 * 当前播放时间对应的**可见**组下标：先在全量分组里二分定位，再映射到可见分组。
 * 当前所在句整组被隐藏时返回 -1（此刻不高亮任何行，也不延长上一句）。
 */
export function activeVisibleIndex(
  resolved: ResolvedLyricGroups,
  currentTime: number
): number {
  const index = activeGroupIndex(resolved.all, currentTime);
  if (index < 0) return -1;
  return resolved.visibleIndexOfAll[index] ?? -1;
}

/** 该行是否带可用的逐字时间轴。 */
export function hasWordTiming(
  line: LyricLine | undefined
): line is LyricLine & { words: LyricWord[] } {
  return !!line?.words && line.words.length > 0;
}

/**
 * 逐字进度：返回每个音节在 currentTime 下的完成度 0..1。
 * 已唱完为 1、未开始为 0、进行中按线性插值；零时长音节到点即 1。
 */
export function wordProgress(words: LyricWord[], currentTime: number): number[] {
  return words.map((word) => {
    if (currentTime >= word.end) return 1;
    if (currentTime < word.start) return 0;
    const span = word.end - word.start;
    return span <= 0 ? 1 : Math.min(1, Math.max(0, (currentTime - word.start) / span));
  });
}

/** 一句唱完后多久算进入间奏（秒）。 */
export const INTERMISSION_DELAY_SECONDS = 2;
/** 句尾到下一句起点至少留这么长的空档才按间奏处理（秒），短停顿不淡出。 */
export const INTERMISSION_MIN_GAP_SECONDS = 6;

/** 一组（同时间戳的原文 / 译文）的结束时间：取组内各行 `end` 的最大值；都没有则 undefined。 */
export function groupEnd(group: LyricGroup): number | undefined {
  let end: number | undefined;
  for (const line of group.lines) {
    if (typeof line.end === "number" && Number.isFinite(line.end) && line.end > group.time) {
      end = end === undefined ? line.end : Math.max(end, line.end);
    }
  }
  return end;
}

/**
 * 当前句是否已进入间奏：只有带可靠结束时间的句子（逐字来源）才会判定。
 * 条件：播放位置超过句尾 `INTERMISSION_DELAY_SECONDS`，且句尾到下一可见句起点的空档
 * 不少于 `INTERMISSION_MIN_GAP_SECONDS`（末句按无下一句处理）。三处显示组件据此把当前句
 * 淡出，避免长间奏期间一直高亮一句已经唱完的歌词（对照 LDDC 桌面歌词的淡入淡出）。
 */
export function isInIntermission(
  resolved: ResolvedLyricGroups,
  activeVisible: number,
  currentTime: number
): boolean {
  if (activeVisible < 0) return false;
  const group = resolved.visible[activeVisible];
  if (!group) return false;
  const end = groupEnd(group);
  if (end === undefined) return false;
  if (currentTime < end + INTERMISSION_DELAY_SECONDS) return false;
  const next = resolved.visible[activeVisible + 1];
  const gap = next ? next.time - end : Number.POSITIVE_INFINITY;
  return gap >= INTERMISSION_MIN_GAP_SECONDS;
}
