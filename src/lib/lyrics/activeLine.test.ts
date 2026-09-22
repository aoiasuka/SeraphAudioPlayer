import { describe, expect, it } from "vitest";
import {
  activeGroupIndex,
  activeGroupRange,
  activeVisibleIndex,
  activeVisibleRange,
  groupEnd,
  groupHasWordTiming,
  groupLyricsByTime,
  hasWordTiming,
  INTERMISSION_DELAY_MS,
  INTERMISSION_MIN_GAP_MS,
  isInIntermission,
  lyricsPositionMs,
  resolveVisibleGroups,
  wordEndMs,
  wordProgress,
  type LyricGroup,
} from "./activeLine";
import type { LyricLine } from "@/types/track";

function line(startMs: number, text: string): LyricLine {
  return { startMs, text };
}

function group(startMs: number, lines: LyricLine[], background: LyricLine[] = []): LyricGroup {
  return { startMs, lines, background };
}

describe("groupLyricsByTime", () => {
  it("keeps distinct timestamps as separate groups", () => {
    const groups = groupLyricsByTime([line(0, "a"), line(5000, "b"), line(10_000, "c")]);
    expect(groups.map((g) => g.startMs)).toEqual([0, 5000, 10_000]);
    expect(groups.every((g) => g.lines.length === 1 && g.background.length === 0)).toBe(true);
  });

  it("merges same-timestamp lines into one group (bilingual lyrics)", () => {
    const groups = groupLyricsByTime([line(5000, "原文"), line(5005, "译文"), line(9000, "下一句")]);
    expect(groups).toHaveLength(2);
    expect(groups[0].lines.map((l) => l.text)).toEqual(["原文", "译文"]);
  });

  it("handles empty input", () => {
    expect(groupLyricsByTime([])).toEqual([]);
  });

  it("同起点但 agent 不同的两条主句是对唱，不按双语合并", () => {
    const groups = groupLyricsByTime([
      { startMs: 5000, text: "You", agent: "v1" },
      { startMs: 5000, text: "Me", agent: "v2" },
      { startMs: 9000, text: "Together", agent: "v1" },
      { startMs: 9000, text: "一起", agent: "v1" },
    ]);
    expect(groups.map((g) => g.lines.map((l) => l.text))).toEqual([["You"], ["Me"], ["Together", "一起"]]);
  });

  it("制作信息自成一组、同起点的制作信息并成一块，从不并入主句", () => {
    const groups = groupLyricsByTime([
      { startMs: 0, text: "作词：某人", role: "credit" },
      { startMs: 0, text: "作曲：某人", role: "credit" },
      line(0, "第一句"),
      line(4000, "第二句"),
      { startMs: 4000, text: "编曲：某人", role: "credit" },
    ]);
    expect(groups.map((g) => g.lines.map((l) => l.text))).toEqual([
      ["作词：某人", "作曲：某人"],
      ["第一句"],
      ["第二句"],
      ["编曲：某人"],
    ]);
    expect(groups[0].lines.every((l) => l.role === "credit")).toBe(true);
  });

  it("和声挂到起点落在其范围内的主句；主句已结束或没有主句时自成一组", () => {
    const groups = groupLyricsByTime([
      { startMs: 1000, endMs: 4000, text: "Main" },
      { startMs: 2000, endMs: 3500, text: "(oh)", role: "background" },
      { startMs: 3000, endMs: 4000, text: "(ah)", role: "background" },
      { startMs: 6000, endMs: 7000, text: "(alone)", role: "background" },
      line(9000, "无结束时间的主句"),
      { startMs: 9500, text: "(still)", role: "background" },
    ]);
    expect(groups.map((g) => [g.lines.map((l) => l.text), g.background.map((l) => l.text)])).toEqual([
      [["Main"], ["(oh)", "(ah)"]],
      [["(alone)"], []],
      [["无结束时间的主句"], ["(still)"]],
    ]);
    // 组结束时间含和声
    expect(groupEnd(groups[0])).toBe(4000);
    expect(groupHasWordTiming(groups[0])).toBe(false);
    groups[0].background[0].words = [{ startMs: 2000, endMs: 3500, text: "(oh)" }];
    expect(groupHasWordTiming(groups[0])).toBe(true);
    expect(groupHasWordTiming(undefined)).toBe(false);
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

describe("activeGroupRange / activeVisibleRange（多活动区间）", () => {
  const timed = (startMs: number, endMs: number, text: string): LyricLine => ({ startMs, endMs, text });

  it("没有结束时间的行级歌词退化为单句语义", () => {
    const groups = groupLyricsByTime([line(0, "a"), line(10_000, "b")]);
    expect(activeGroupRange(groups, -100)).toEqual({ primary: -1, active: [] });
    expect(activeGroupRange(groupLyricsByTime([line(3000, "x")]), 1000)).toEqual({ primary: -1, active: [] });
    expect(activeGroupRange(groups, 5000)).toEqual({ primary: 0, active: [0] });
    expect(activeGroupRange(groups, 12_000)).toEqual({ primary: 1, active: [1] });
  });

  it("对唱重叠：两句都活动，主句是最早开始且仍在唱的那句；先结束的退出", () => {
    // A 10–20 与 B 15–25 重叠；C 30–32 短插句、D 30–40 长句
    const groups = groupLyricsByTime([
      timed(10_000, 20_000, "A"),
      timed(15_000, 25_000, "B"),
      timed(30_000, 40_000, "D"),
      timed(31_000, 32_000, "C"),
    ]);
    expect(activeGroupRange(groups, 12_000)).toEqual({ primary: 0, active: [0] });
    expect(activeGroupRange(groups, 16_000)).toEqual({ primary: 0, active: [0, 1] });
    expect(activeGroupRange(groups, 21_000)).toEqual({ primary: 1, active: [1] });
    // 26s：都唱完了，退化为最近开始的一句（供间奏淡出）
    expect(activeGroupRange(groups, 26_000)).toEqual({ primary: 1, active: [1] });
    // 短插句结束后主句回到仍在唱的长句
    expect(activeGroupRange(groups, 31_500)).toEqual({ primary: 2, active: [2, 3] });
    expect(activeGroupRange(groups, 33_000)).toEqual({ primary: 2, active: [2] });
  });

  it("映射到可见分组：隐藏句剔除，主句被隐藏时按 -1", () => {
    const resolved = resolveVisibleGroups([
      timed(10_000, 20_000, "A"),
      { ...timed(15_000, 25_000, "作词：某人"), hidden: true },
      timed(30_000, 31_000, "C"),
    ]);
    expect(activeVisibleRange(resolved, 16_000)).toEqual({ primary: 0, active: [0] });
    expect(activeVisibleRange(resolved, 21_000)).toEqual({ primary: -1, active: [] });
    expect(activeVisibleRange(resolved, 30_500)).toEqual({ primary: 1, active: [1] });
    expect(activeVisibleRange(resolved, 5000)).toEqual({ primary: -1, active: [] });
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

  it("被隐藏的制作信息块（显示选项关闭）整组不可见，主句照常；隐藏的和声只从组里剔除", () => {
    const resolved = resolveVisibleGroups([
      { startMs: 0, text: "作词：某人", role: "credit", hidden: true },
      { startMs: 0, text: "作曲：某人", role: "credit", hidden: true },
      { startMs: 18_000, endMs: 22_000, text: "第一句" },
      { startMs: 19_000, endMs: 20_000, text: "(oh)", role: "background", hidden: true },
      { startMs: 20_000, endMs: 21_000, text: "(ah)", role: "background" },
    ]);
    expect(resolved.visibleIndexOfAll).toEqual([-1, 0]);
    expect(resolved.visible[0].background.map((l) => l.text)).toEqual(["(ah)"]);
    expect(activeVisibleIndex(resolved, 5000)).toBe(-1);
    expect(activeVisibleIndex(resolved, 19_000)).toBe(0);
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
    expect(groupEnd(group(1000, [line(1000, "a")]))).toBeUndefined();
    expect(groupEnd(group(1000, [timed(1000, 1000, "a")]))).toBeUndefined();
    expect(groupEnd(group(1000, [timed(1000, 3000, "a"), timed(1000, 5000, "译")]))).toBe(5000);
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

describe("2026-09-22 audit", () => {
  it("same-start bilingual group: the word-synced line becomes the head (LDDC roma, orig, ts order)", () => {
    const roman = { startMs: 5000, text: "kon ni chi wa" };
    const orig = { startMs: 5000, endMs: 7000, text: "konnichiwa", words: [{ startMs: 5000, endMs: 7000, text: "konnichiwa" }] };
    const ts = { startMs: 5000, text: "hello" };
    const groups = groupLyricsByTime([roman, orig, ts]);
    expect(groups).toHaveLength(1);
    expect(groups[0].lines.map((l) => l.text)).toEqual(["konnichiwa", "kon ni chi wa", "hello"]);
    const plain = groupLyricsByTime([{ startMs: 1, text: "a" }, { startMs: 1, text: "b" }]);
    expect(plain[0].lines.map((l) => l.text)).toEqual(["a", "b"]);
  });
});
