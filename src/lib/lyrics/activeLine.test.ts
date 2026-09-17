import { describe, expect, it } from "vitest";
import {
  activeGroupIndex,
  groupLyricsByTime,
  hasWordTiming,
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
