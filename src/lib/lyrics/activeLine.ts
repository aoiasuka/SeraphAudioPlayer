import type { LyricLine, LyricWord } from "@/types/track";

/**
 * 歌词分组与当前行定位纯函数。**时间一律毫秒整数**（2026-09-20 模型重构起）。
 *
 * 从 LyricsPanel 提炼,主窗口歌词稿与任务栏歌词条共用同一口径。分组按行角色（B2）：
 * - 主唱（main）：一行一组；相邻、同起点（±epsilon）、agent 相同或都没有的两条主唱行
 *   合并成一组——这是来源不明的双语 LRC 的兜底形态；agent 不同的同起点主唱行是对唱，各自成组。
 * - 和声（background）：挂到所在的主句组（起点落在该组时间范围内），组内单独存放；
 *   没有可挂的主句时自成一组。
 * - 制作信息（credit）：自成一组，相邻同起点的制作信息行并成一块；从不并入主句。
 * 定位按播放位置二分找「最近开始的组」，再把仍在唱的更早组一并算作活动（多活动区间）。
 */

export interface LyricGroup {
  /** 组起点（毫秒） */
  startMs: number;
  /** 主句 + 双语兜底的同起点行（lines[0] 是主句）；制作信息组里全是制作信息行 */
  lines: LyricLine[];
  /** 挂在这一句下的和声行（TTML `x-bg`），可能为空 */
  background: LyricLine[];
}

export const SAME_TIMESTAMP_EPSILON_MS = 10;

function roleOf(line: LyricLine) {
  return line.role ?? "main";
}

function sameStart(a: number, b: number) {
  return Math.abs(a - b) <= SAME_TIMESTAMP_EPSILON_MS;
}

/** 组的最晚结束时间（含和声）；没有可靠结束时间则 undefined。 */
export function groupEnd(group: LyricGroup): number | undefined {
  let end: number | undefined;
  const consider = (line: LyricLine) => {
    if (typeof line.endMs === "number" && Number.isFinite(line.endMs) && line.endMs > group.startMs) {
      end = end === undefined ? line.endMs : Math.max(end, line.endMs);
    }
  };
  for (const line of group.lines) consider(line);
  for (const line of group.background) consider(line);
  return end;
}

/** 按角色分组（见文件头说明）。输入按起点升序（解析后的自然顺序）。 */
export function groupLyricsByTime(lyrics: LyricLine[]): LyricGroup[] {
  const groups: LyricGroup[] = [];

  for (const line of lyrics) {
    const previous = groups[groups.length - 1];
    const role = roleOf(line);

    if (role === "background") {
      // 挂到最近的主句：起点落在其范围内（无结束时间的主句范围延续到下一句）
      const host = previous && roleOf(previous.lines[0]) === "main" ? previous : undefined;
      if (host) {
        const end = groupEnd(host);
        const withinHost =
          line.startMs + SAME_TIMESTAMP_EPSILON_MS >= host.startMs &&
          (end === undefined || line.startMs <= end + SAME_TIMESTAMP_EPSILON_MS);
        if (withinHost) {
          host.background.push(line);
          continue;
        }
      }
      groups.push({ startMs: line.startMs, lines: [line], background: [] });
      continue;
    }

    if (previous && sameStart(previous.startMs, line.startMs)) {
      const head = previous.lines[0];
      const headRole = roleOf(head);
      if (role === "credit" && headRole === "credit") {
        previous.lines.push(line);
        continue;
      }
      // 双语兜底：同起点、两条主唱、agent 一致（或都没有）才合并；对唱各自成组。
      // 带逐字时间轴的那条做主句（lines[0]）：LDDC 默认按「音译、原文、译文」顺序写文件，
      // 否则罗马音会被当成主句大字显示、真正的原文降级成小字且丢掉逐字高亮。
      if (role === "main" && headRole === "main" && (head.agent ?? "") === (line.agent ?? "")) {
        if (hasWordTiming(line) && !hasWordTiming(head)) {
          previous.lines.unshift(line);
        } else {
          previous.lines.push(line);
        }
        continue;
      }
    }

    groups.push({ startMs: line.startMs, lines: [line], background: [] });
  }

  return groups;
}

/**
 * 二分定位当前播放位置（毫秒）对应的歌词组下标;尚未到达第一句时返回 -1。
 * 要求 groups 按起点升序(解析后的自然顺序)。
 */
export function activeGroupIndex(groups: LyricGroup[], currentMs: number): number {
  let low = 0;
  let high = groups.length - 1;
  let match = -1;

  while (low <= high) {
    const mid = Math.floor((low + high) / 2);
    if (currentMs + SAME_TIMESTAMP_EPSILON_MS >= groups[mid].startMs) {
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
  /** 每组剔除 hidden 行后仍非空的组；组内 lines / background 只保留可见行 */
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
    const background = group.background.filter((line) => !line.hidden);
    visibleIndexOfAll.push(visible.length);
    const unchanged =
      lines.length === group.lines.length && background.length === group.background.length;
    visible.push(unchanged ? group : { startMs: group.startMs, lines, background });
  }
  return { all, visible, visibleIndexOfAll };
}

/**
 * 当前播放位置（毫秒）对应的**可见**组下标：先在全量分组里二分定位，再映射到可见分组。
 * 当前所在句整组被隐藏时返回 -1（此刻不高亮任何行，也不延长上一句）。
 */
export function activeVisibleIndex(resolved: ResolvedLyricGroups, currentMs: number): number {
  const index = activeGroupIndex(resolved.all, currentMs);
  if (index < 0) return -1;
  return resolved.visibleIndexOfAll[index] ?? -1;
}

/** 多活动区间往前最多回看这么多组（对唱 / 合唱同时在唱的句子不会更多）。 */
const OVERLAP_LOOKBACK = 4;

/**
 * 多活动区间：同一时刻可能有多句在唱（对唱重叠、和声延续）。
 * 返回全量分组下标：`primary` 是滚动与逐字锚定的主句——所有仍在唱的句子里最早开始的那句；
 * `active` 是全部活动句（升序，含 primary）。「仍在唱」= 已开始且带可靠结束时间、尚未结束；
 * 最近开始的一句没有结束时间（行级歌词）时它总是活动的。没有任何句在唱时退化为
 * `activeGroupIndex` 的单句语义（最近开始的一句，间奏判定据此淡出）。尚未到第一句时都为 -1 / 空。
 */
export function activeGroupRange(
  groups: LyricGroup[],
  currentMs: number
): { primary: number; active: number[] } {
  const latest = activeGroupIndex(groups, currentMs);
  if (latest < 0) return { primary: -1, active: [] };
  const active: number[] = [];
  for (let index = Math.max(0, latest - OVERLAP_LOOKBACK); index <= latest; index += 1) {
    const end = groupEnd(groups[index]);
    if (end === undefined ? index === latest : end > currentMs) active.push(index);
  }
  if (active.length === 0) active.push(latest);
  return { primary: active[0], active };
}

/** `activeGroupRange` 映射到可见分组：整组隐藏的句子剔除；主句被隐藏时按 -1 处理。 */
export function activeVisibleRange(
  resolved: ResolvedLyricGroups,
  currentMs: number
): { primary: number; active: number[] } {
  const range = activeGroupRange(resolved.all, currentMs);
  const toVisible = (index: number) => resolved.visibleIndexOfAll[index] ?? -1;
  return {
    primary: range.primary < 0 ? -1 : toVisible(range.primary),
    active: range.active.map(toVisible).filter((index) => index >= 0),
  };
}

/**
 * 歌词定位用的播放位置（**毫秒**）：引擎上报的 `currentTime`（秒）是已送入设备缓冲的位置，
 * 比可听音频领先 `outputLatency`（秒，Progress 事件携带）。三处歌词组件都用它做行定位、
 * 逐字锚定与间奏判定；进度条仍显示原始 `currentTime`。坏值按 0 处理，结果不小于 0。
 */
export function lyricsPositionMs(currentTimeSeconds: number, outputLatencySeconds: number): number {
  const latency =
    Number.isFinite(outputLatencySeconds) && outputLatencySeconds > 0 ? outputLatencySeconds : 0;
  const seconds = Number.isFinite(currentTimeSeconds) ? currentTimeSeconds : 0;
  return Math.max(0, Math.round((seconds - latency) * 1000));
}

/** 该行是否带可用的逐字时间轴。 */
export function hasWordTiming(
  line: LyricLine | undefined
): line is LyricLine & { words: LyricWord[] } {
  return !!line?.words && line.words.length > 0;
}

/** 组里是否有任何一行（主句或和声）带逐字时间轴——决定是否启用平滑时钟。 */
export function groupHasWordTiming(group: LyricGroup | undefined): boolean {
  if (!group) return false;
  return hasWordTiming(group.lines[0]) || group.background.some((line) => hasWordTiming(line));
}

/**
 * 音节的有效终点：自身 `endMs`，缺省用下一音节起点，再缺省用行终点，都没有则视为零时长。
 */
export function wordEndMs(words: LyricWord[], index: number, lineEndMs?: number): number {
  const word = words[index];
  if (typeof word.endMs === "number") return word.endMs;
  const next = words[index + 1];
  if (next) return next.startMs;
  if (typeof lineEndMs === "number" && lineEndMs > word.startMs) return lineEndMs;
  return word.startMs;
}

/**
 * 逐字进度：返回每个音节在 currentMs 下的完成度 0..1。
 * 已唱完为 1、未开始为 0、进行中按线性插值；零时长音节到点即 1。
 */
export function wordProgress(words: LyricWord[], currentMs: number, lineEndMs?: number): number[] {
  return words.map((word, index) => {
    const end = wordEndMs(words, index, lineEndMs);
    if (currentMs >= end) return 1;
    if (currentMs < word.startMs) return 0;
    const span = end - word.startMs;
    return span <= 0 ? 1 : Math.min(1, Math.max(0, (currentMs - word.startMs) / span));
  });
}

/** 一句唱完后多久算进入间奏（毫秒）。 */
export const INTERMISSION_DELAY_MS = 2000;
/** 句尾到下一句起点至少留这么长的空档才按间奏处理（毫秒），短停顿不淡出。 */
export const INTERMISSION_MIN_GAP_MS = 6000;

/**
 * 当前句是否已进入间奏：只有带可靠结束时间的句子（逐字来源）才会判定。
 * 条件：播放位置超过句尾 `INTERMISSION_DELAY_MS`，且句尾到下一可见句起点的空档
 * 不少于 `INTERMISSION_MIN_GAP_MS`（末句按无下一句处理）。三处显示组件据此把当前句
 * 淡出，避免长间奏期间一直高亮一句已经唱完的歌词（对照 LDDC 桌面歌词的淡入淡出）。
 */
export function isInIntermission(
  resolved: ResolvedLyricGroups,
  activeVisible: number,
  currentMs: number
): boolean {
  if (activeVisible < 0) return false;
  const group = resolved.visible[activeVisible];
  if (!group) return false;
  const end = groupEnd(group);
  if (end === undefined) return false;
  if (currentMs < end + INTERMISSION_DELAY_MS) return false;
  const next = resolved.visible[activeVisible + 1];
  const gap = next ? next.startMs - end : Number.POSITIVE_INFINITY;
  return gap >= INTERMISSION_MIN_GAP_MS;
}
