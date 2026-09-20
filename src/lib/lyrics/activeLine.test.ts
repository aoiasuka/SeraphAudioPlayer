import { describe, expect, it } from "vitest";
import {
  activeGroupIndex,
  activeVisibleIndex,
  groupEnd,
  groupLyricsByTime,
  hasWordTiming,
  INTERMISSION_DELAY_MS,
  INTERMISSION_MIN_GAP_MS,
  isInIntermission,
  lyricsPositionMs,
  resolveVisibleGroups,
  wordEndMs,
  wordProgress,
} from "./activeLine";
import type { LyricLine } from "@/types/track";

function line(startMs: number, text: string): LyricLine {
  return { startMs, text };
}

describe("groupLyricsByTime", () => {
  it("keeps distinct timestamps as separate groups", () => {
    const groups = groupLyricsByTime([line(0, "a"), line(5000, "b"), line(10_000, "c")]);
    expect(groups.map((g) => g.startMs)).toEqual([0, 5000, 10_000]);
    expect(groups.every((g) => g.lines.length === 1)).toBe(true);
  });

  it("merges same-timestamp lines into one group (bilingual lyrics)", () => {
    const groups = groupLyricsByTime([line(5000, "原文"), line(5005, "译文"), line(9000, "下一句")]);
    expect(groups).toHaveLength(2);
    expect(groups[0].lines.map((l) => l.text)).toEqual(["原文", "译文"]);
  });

  it("handles empty input", () => {
    expect(groupLyricsByTime([])).toEqual([]);
  });
});

describe("activeGroupIndex", () => {
  const groups = groupLyricsByTime([line(0, "a"), line(10_000, "b"), line(20_000, "c")]);

  it("returns -1 before the first line", () => {
    expect(activeGroupIndex(groupLyricsByTime([line(3000, "x")]), 1000)).toBe(-1);
  });

  it("locates the group whose time has been reached", () => {
    expect(activeGroupIndex(groups, 0)).toBe(0);
    expect(activeGroupIndex(groups, 9980)).toBe(0);
    expect(activeGroupIndex(groups, 10_000)).toBe(1);
    expect(activeGroupIndex(groups, 15_000)).toBe(1);
    expect(activeGroupIndex(groups, 25_000)).toBe(2);
  });

  it("tolerates timestamps within epsilon just before the boundary", () => {
    // currentMs + ε >= 10000 → 9995 已算进入第二句
    expect(activeGroupIndex(groups, 9995)).toBe(1);
  });

  it("handles empty groups", () => {
    expect(activeGroupIndex([], 5000)).toBe(-1);
  });
});

describe("resolveVisibleGroups / activeVisibleIndex（隐藏句不并入上一句）", () => {
  const hidden = (startMs: number, text: string): LyricLine => ({ startMs, text, hidden: true });

  it("中间一句被隐藏：播放到该句区间返回 -1，下一句返回正确的 visible 下标", () => {
    const resolved = resolveVisibleGroups([line(0, "第一句"), hidden(10_000, "作词：某人"), line(20_000, "第三句")]);
    expect(resolved.all).toHaveLength(3);
    expect(resolved.visible.map((g) => g.lines[0].text)).toEqual(["第一句", "第三句"]);
    expect(resolved.visibleIndexOfAll).toEqual([0, -1, 1]);
    expect(activeVisibleIndex(resolved, 5000)).toBe(0);
    expect(activeVisibleIndex(resolved, 10_000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 15_000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 20_000)).toBe(1);
    expect(activeVisibleIndex(resolved, 99_000)).toBe(1);
  });

  it("首句隐藏：首句区间返回 -1，尚未到第一句也返回 -1", () => {
    const resolved = resolveVisibleGroups([hidden(0, "作曲：某人"), line(10_000, "正文")]);
    expect(resolved.visibleIndexOfAll).toEqual([-1, 0]);
    expect(activeVisibleIndex(resolved, -1000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 3000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 10_000)).toBe(0);
  });

  it("末句隐藏：播放到末句后不再停留在上一句", () => {
    const resolved = resolveVisibleGroups([line(0, "正文"), hidden(30_000, "发行：某厂牌")]);
    expect(activeVisibleIndex(resolved, 29_000)).toBe(0);
    expect(activeVisibleIndex(resolved, 30_000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 100_000)).toBe(-1);
  });

  it("同一组里部分行隐藏时保留可见行且不改组时间；无隐藏行时复用原组对象", () => {
    const resolved = resolveVisibleGroups([line(5000, "原文"), hidden(5005, "译文"), line(9000, "下一句")]);
    expect(resolved.visible[0].lines.map((l) => l.text)).toEqual(["原文"]);
    expect(resolved.visible[0].startMs).toBe(5000);
    expect(resolved.visible[1]).toBe(resolved.all[1]);
    expect(activeVisibleIndex(resolved, 6000)).toBe(0);
  });

  it("全部隐藏或空输入：visible 为空，任何时间都返回 -1", () => {
    const resolved = resolveVisibleGroups([hidden(0, "a"), hidden(10_000, "b")]);
    expect(resolved.visible).toEqual([]);
    expect(activeVisibleIndex(resolved, 5000)).toBe(-1);
    expect(activeVisibleIndex(resolveVisibleGroups([]), 5000)).toBe(-1);
  });
});

describe("逐字进度", () => {
  const words = [
    { startMs: 1000, endMs: 2000, text: "a" },
    { startMs: 2000, endMs: 2000, text: "b" },
    { startMs: 2000, endMs: 4000, text: "c" },
  ];

  it("hasWordTiming 只认非空 words", () => {
    expect(hasWordTiming(undefined)).toBe(false);
    expect(hasWordTiming(line(0, "x"))).toBe(false);
    expect(hasWordTiming({ ...line(0, "x"), words: [] })).toBe(false);
    expect(hasWordTiming({ ...line(0, "x"), words })).toBe(true);
  });

  it("wordProgress 按线性插值，零时长音节到点即 1", () => {
    expect(wordProgress(words, 0)).toEqual([0, 0, 0]);
    expect(wordProgress(words, 1500)).toEqual([0.5, 0, 0]);
    expect(wordProgress(words, 2000)).toEqual([1, 1, 0]);
    expect(wordProgress(words, 3000)).toEqual([1, 1, 0.5]);
    expect(wordProgress(words, 9000)).toEqual([1, 1, 1]);
  });

  it("终点未知的音节用下一音节起点，末音节用行终点，都没有则零时长", () => {
    const open = [
      { startMs: 1000, text: "a" },
      { startMs: 2000, text: "b" },
    ];
    expect(wordEndMs(open, 0)).toBe(2000);
    expect(wordEndMs(open, 1, 4000)).toBe(4000);
    expect(wordEndMs(open, 1)).toBe(2000);
    expect(wordProgress(open, 1500, 4000)).toEqual([0.5, 0]);
    expect(wordProgress(open, 3000, 4000)).toEqual([1, 0.5]);
    expect(wordProgress(open, 3000)).toEqual([1, 1]);
  });
});

describe("间奏判定（行结束时间）", () => {
  const timed = (startMs: number, endMs: number, text: string): LyricLine => ({ startMs, endMs, text });

  it("groupEnd 取组内最大 end，无 end 或 end 不晚于起点则 undefined", () => {
    expect(groupEnd({ startMs: 1000, lines: [line(1000, "a")] })).toBeUndefined();
    expect(groupEnd({ startMs: 1000, lines: [timed(1000, 1000, "a")] })).toBeUndefined();
    expect(groupEnd({ startMs: 1000, lines: [timed(1000, 3000, "a"), timed(1000, 5000, "译")] })).toBe(5000);
  });

  it("句尾之后超过延迟且空档够长才算间奏；下一句临近不算", () => {
    const resolved = resolveVisibleGroups([
      timed(0, 4000, "第一句"),
      timed(20_000, 24_000, "第二句"),
      timed(25_000, 28_000, "第三句"),
    ]);
    // 第一句 4s 结束，下一句 20s → 空档 16s
    expect(isInIntermission(resolved, 0, 3000)).toBe(false);
    expect(isInIntermission(resolved, 0, 4000 + INTERMISSION_DELAY_MS - 100)).toBe(false);
    expect(isInIntermission(resolved, 0, 4000 + INTERMISSION_DELAY_MS)).toBe(true);
    expect(isInIntermission(resolved, 0, 19_000)).toBe(true);
    // 第二句 24s 结束，下一句 25s → 空档 1s，不淡出
    expect(isInIntermission(resolved, 1, 24_900)).toBe(false);
    // 末句：无下一句按无限空档
    expect(isInIntermission(resolved, 2, 28_000 + INTERMISSION_DELAY_MS)).toBe(true);
  });

  it("没有结束时间的行级歌词永不判定为间奏；越界下标返回 false", () => {
    const resolved = resolveVisibleGroups([line(0, "a"), line(30_000, "b")]);
    expect(isInIntermission(resolved, 0, 20_000)).toBe(false);
    expect(isInIntermission(resolved, -1, 20_000)).toBe(false);
    expect(isInIntermission(resolved, 5, 20_000)).toBe(false);
    expect(INTERMISSION_MIN_GAP_MS).toBeGreaterThan(INTERMISSION_DELAY_MS);
  });

  it("隐藏句不参与下一句空档计算（按可见分组）", () => {
    const resolved = resolveVisibleGroups([
      timed(0, 4000, "第一句"),
      { startMs: 5000, text: "作词：某人", hidden: true },
      timed(20_000, 24_000, "第二句"),
    ]);
    expect(isInIntermission(resolved, 0, 10_000)).toBe(true);
  });
});

describe("lyricsPositionMs", () => {
  it("秒 → 毫秒并按输出延迟回拨，坏值按 0 处理且结果不小于 0", () => {
    expect(lyricsPositionMs(10, 0.15)).toBe(9850);
    expect(lyricsPositionMs(10, 0)).toBe(10_000);
    expect(lyricsPositionMs(10, -1)).toBe(10_000);
    expect(lyricsPositionMs(10, Number.NaN)).toBe(10_000);
    expect(lyricsPositionMs(10, Number.POSITIVE_INFINITY)).toBe(10_000);
    expect(lyricsPositionMs(0.05, 0.2)).toBe(0);
    expect(lyricsPositionMs(Number.NaN, 0)).toBe(0);
  });
});
