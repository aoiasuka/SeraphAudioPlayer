import { describe, expect, it } from "vitest";
import {
  activeGroupIndex,
  activeVisibleIndex,
  groupLyricsByTime,
  hasWordTiming,
  resolveVisibleGroups,
  wordProgress,
} from "./activeLine";
import type { LyricLine } from "@/types/track";

function line(time: number, text: string): LyricLine {
  return { time, text };
}

describe("groupLyricsByTime", () => {
  it("keeps distinct timestamps as separate groups", () => {
    const groups = groupLyricsByTime([
      line(0, "a"),
      line(5, "b"),
      line(10, "c"),
    ]);
    expect(groups.map((g) => g.time)).toEqual([0, 5, 10]);
    expect(groups.every((g) => g.lines.length === 1)).toBe(true);
  });

  it("merges same-timestamp lines into one group (bilingual lyrics)", () => {
    const groups = groupLyricsByTime([
      line(5, "原文"),
      line(5.005, "译文"),
      line(9, "下一句"),
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0].lines.map((l) => l.text)).toEqual(["原文", "译文"]);
  });

  it("handles empty input", () => {
    expect(groupLyricsByTime([])).toEqual([]);
  });
});

describe("activeGroupIndex", () => {
  const groups = groupLyricsByTime([
    line(0, "a"),
    line(10, "b"),
    line(20, "c"),
  ]);

  it("returns -1 before the first line", () => {
    expect(activeGroupIndex(groupLyricsByTime([line(3, "x")]), 1)).toBe(-1);
  });

  it("locates the group whose time has been reached", () => {
    expect(activeGroupIndex(groups, 0)).toBe(0);
    expect(activeGroupIndex(groups, 9.98)).toBe(0);
    expect(activeGroupIndex(groups, 10)).toBe(1);
    expect(activeGroupIndex(groups, 15)).toBe(1);
    expect(activeGroupIndex(groups, 25)).toBe(2);
  });

  it("tolerates timestamps within epsilon just before the boundary", () => {
    // currentTime + ε >= time:10 → 9.995 已算进入第二句
    expect(activeGroupIndex(groups, 9.995)).toBe(1);
  });

  it("handles empty groups", () => {
    expect(activeGroupIndex([], 5)).toBe(-1);
  });
});

describe("resolveVisibleGroups / activeVisibleIndex（隐藏句不并入上一句）", () => {
  const hidden = (time: number, text: string): LyricLine => ({ time, text, hidden: true });

  it("中间一句被隐藏：播放到该句区间返回 -1，下一句返回正确的 visible 下标", () => {
    const resolved = resolveVisibleGroups([
      line(0, "第一句"),
      hidden(10, "作词：某人"),
      line(20, "第三句"),
    ]);
    expect(resolved.all).toHaveLength(3);
    expect(resolved.visible.map((g) => g.lines[0].text)).toEqual(["第一句", "第三句"]);
    expect(resolved.visibleIndexOfAll).toEqual([0, -1, 1]);
    expect(activeVisibleIndex(resolved, 5)).toBe(0);
    expect(activeVisibleIndex(resolved, 10)).toBe(-1);
    expect(activeVisibleIndex(resolved, 15)).toBe(-1);
    expect(activeVisibleIndex(resolved, 20)).toBe(1);
    expect(activeVisibleIndex(resolved, 99)).toBe(1);
  });

  it("首句隐藏：首句区间返回 -1，尚未到第一句也返回 -1", () => {
    const resolved = resolveVisibleGroups([hidden(0, "作曲：某人"), line(10, "正文")]);
    expect(resolved.visibleIndexOfAll).toEqual([-1, 0]);
    expect(activeVisibleIndex(resolved, -1)).toBe(-1);
    expect(activeVisibleIndex(resolved, 3)).toBe(-1);
    expect(activeVisibleIndex(resolved, 10)).toBe(0);
  });

  it("末句隐藏：播放到末句后不再停留在上一句", () => {
    const resolved = resolveVisibleGroups([line(0, "正文"), hidden(30, "发行：某厂牌")]);
    expect(activeVisibleIndex(resolved, 29)).toBe(0);
    expect(activeVisibleIndex(resolved, 30)).toBe(-1);
    expect(activeVisibleIndex(resolved, 100)).toBe(-1);
  });

  it("同一组里部分行隐藏时保留可见行且不改组时间；无隐藏行时复用原组对象", () => {
    const resolved = resolveVisibleGroups([
      line(5, "原文"),
      hidden(5.005, "译文"),
      line(9, "下一句"),
    ]);
    expect(resolved.visible[0].lines.map((l) => l.text)).toEqual(["原文"]);
    expect(resolved.visible[0].time).toBe(5);
    expect(resolved.visible[1]).toBe(resolved.all[1]);
    expect(activeVisibleIndex(resolved, 6)).toBe(0);
  });

  it("全部隐藏或空输入：visible 为空，任何时间都返回 -1", () => {
    const resolved = resolveVisibleGroups([hidden(0, "a"), hidden(10, "b")]);
    expect(resolved.visible).toEqual([]);
    expect(activeVisibleIndex(resolved, 5)).toBe(-1);
    expect(activeVisibleIndex(resolveVisibleGroups([]), 5)).toBe(-1);
  });
});

describe("逐字进度", () => {
  const words = [
    { start: 1, end: 2, text: "a" },
    { start: 2, end: 2, text: "b" },
    { start: 2, end: 4, text: "c" },
  ];

  it("hasWordTiming 只认非空 words", () => {
    expect(hasWordTiming(undefined)).toBe(false);
    expect(hasWordTiming(line(0, "x"))).toBe(false);
    expect(hasWordTiming({ ...line(0, "x"), words: [] })).toBe(false);
    expect(hasWordTiming({ ...line(0, "x"), words })).toBe(true);
  });

  it("wordProgress 按线性插值，零时长音节到点即 1", () => {
    expect(wordProgress(words, 0)).toEqual([0, 0, 0]);
    expect(wordProgress(words, 1.5)).toEqual([0.5, 0, 0]);
    expect(wordProgress(words, 2)).toEqual([1, 1, 0]);
    expect(wordProgress(words, 3)).toEqual([1, 1, 0.5]);
    expect(wordProgress(words, 9)).toEqual([1, 1, 1]);
  });
});
