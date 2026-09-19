import { describe, expect, it } from "vitest";
import { candidateBadges, describeCandidate } from "./candidate";
import type { LyricLine } from "@/types/track";

describe("describeCandidate / candidateBadges", () => {
  it("识别逐字、两种译文形态与音译", () => {
    const plain: LyricLine[] = [
      { time: 1, text: "a" },
      { time: 2, text: "b" },
    ];
    expect(describeCandidate(plain)).toEqual({ wordSynced: false, translation: false, roman: false });
    expect(candidateBadges(plain)).toEqual(["逐行"]);

    const bilingualLrc: LyricLine[] = [
      { time: 1, text: "a" },
      { time: 1.005, text: "甲" },
    ];
    expect(candidateBadges(bilingualLrc)).toEqual(["逐行", "译文"]);

    const ttml: LyricLine[] = [
      { time: 1, text: "a", translation: "甲", roman: "ei", words: [{ start: 1, end: 2, text: "a" }] },
    ];
    expect(candidateBadges(ttml)).toEqual(["逐字", "译文", "音译"]);

    // 同时间戳但同文本（去重残留）不算译文；空 words 不算逐字
    const dup: LyricLine[] = [
      { time: 1, text: "a", words: [] },
      { time: 1, text: "a" },
    ];
    expect(candidateBadges(dup)).toEqual(["逐行"]);
    expect(candidateBadges([])).toEqual(["逐行"]);
  });
});
