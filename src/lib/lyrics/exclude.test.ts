import { describe, expect, it } from "vitest";
import { sanitizeExcludeRules, visibleLyrics } from "./exclude";
import type { LyricLine } from "@/types/track";

describe("sanitizeExcludeRules", () => {
  it("丢弃坏元素、去重、截断并补 id", () => {
    const rules = sanitizeExcludeRules([
      null,
      { kind: "keyword", pattern: "  作词 " },
      { kind: "keyword", pattern: "作词" },
      { kind: "regex", pattern: "^x" },
      { kind: "nope", pattern: "x" },
      { kind: "keyword", pattern: "" },
    ]);
    expect(rules).toEqual([
      { id: "keyword:作词", kind: "keyword", pattern: "作词" },
      { id: "regex:^x", kind: "regex", pattern: "^x" },
    ]);
    expect(sanitizeExcludeRules("bad")).toEqual([]);
  });
});

describe("visibleLyrics", () => {
  const lyrics: LyricLine[] = [
    { startMs: 0, text: "作词：某人", hidden: true },
    { startMs: 1000, text: "Hello" },
    { startMs: 2000, text: "第二句", hidden: false },
  ];

  it("按后端 hidden 标记过滤；无隐藏行时原引用返回", () => {
    expect(visibleLyrics(lyrics).map((l) => l.text)).toEqual(["Hello", "第二句"]);
    const clean = lyrics.slice(1);
    expect(visibleLyrics(clean)).toBe(clean);
    expect(visibleLyrics([])).toEqual([]);
  });
});
