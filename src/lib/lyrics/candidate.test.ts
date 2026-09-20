import { describe, expect, it } from "vitest";
import { candidateBadges, describeCandidate } from "./candidate";
import type { LyricLine } from "@/types/track";

describe("describeCandidate / candidateBadges", () => {
  it("识别逐字、两种译文形态与音译", () => {
    const plain: LyricLine[] = [
      { startMs: 1000, text: "a" },
      { startMs: 2000, text: "b" },
    ];
    expect(describeCandidate(plain)).toEqual({ wordSynced: false, translation: false, roman: false });
    expect(candidateBadges(plain)).toEqual(["逐行"]);

    const bilingualLrc: LyricLine[] = [
      { startMs: 1000, text: "a" },
      { startMs: 1005, text: "甲" },
    ];
    expect(candidateBadges(bilingualLrc)).toEqual(["逐行", "译文"]);

    const ttml: LyricLine[] = [
      {
        startMs: 1000,
        text: "a",
        translations: [{ text: "甲" }],
        roman: { text: "ei" },
        words: [{ startMs: 1000, endMs: 2000, text: "a" }],
      },
    ];
    expect(candidateBadges(ttml)).toEqual(["逐字", "译文", "音译"]);

    // 同时间戳但同文本（去重残留）不算译文；空 words / 空 translations 不算
    const dup: LyricLine[] = [
      { startMs: 1000, text: "a", words: [], translations: [] },
      { startMs: 1000, text: "a" },
    ];
    expect(candidateBadges(dup)).toEqual(["逐行"]);
    expect(candidateBadges([])).toEqual(["逐行"]);
  });
});
