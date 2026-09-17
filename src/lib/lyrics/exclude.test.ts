import { describe, expect, it } from "vitest";
import {
  compileExcludeRules,
  filterLyricsByRules,
  sanitizeExcludeRules,
  validateRegexPattern,
} from "./exclude";
import type { LyricLine } from "@/types/track";

const lyrics: LyricLine[] = [
  { time: 0, text: "作词：某人" },
  { time: 1, text: "Hello World", translation: "你好世界" },
  { time: 2, text: "第二句", roman: "di er ju" },
];

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

describe("filterLyricsByRules", () => {
  it("无规则时原数组引用直接返回", () => {
    expect(filterLyricsByRules(lyrics, [])).toBe(lyrics);
  });

  it("关键词大小写不敏感，正则命中译文/音译也排除", () => {
    const filtered = filterLyricsByRules(lyrics, [
      { id: "a", kind: "keyword", pattern: "hello" },
      { id: "b", kind: "regex", pattern: "^作词" },
    ]);
    expect(filtered.map((l) => l.text)).toEqual(["第二句"]);
    const byRoman = filterLyricsByRules(lyrics, [{ id: "c", kind: "regex", pattern: "er ju$" }]);
    expect(byRoman.map((l) => l.text)).toEqual(["作词：某人", "Hello World"]);
  });

  it("坏正则跳过而不是清空歌词", () => {
    const excluded = compileExcludeRules([{ id: "x", kind: "regex", pattern: "(" }]);
    expect(lyrics.some(excluded)).toBe(false);
    expect(validateRegexPattern("(")).not.toBeNull();
    expect(validateRegexPattern("^ok$")).toBeNull();
  });
});
