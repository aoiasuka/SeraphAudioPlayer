import type { LyricLine } from "@/types/track";
import { SAME_TIMESTAMP_EPSILON_MS } from "./activeLine";

/** 在线歌词候选的能力标签：是否逐字、是否带译文、是否带音译（供候选卡片标注）。 */
export interface CandidateCapabilities {
  wordSynced: boolean;
  translation: boolean;
  roman: boolean;
}

/**
 * 从候选歌词行推断能力。译文两种形态都算：`translations` 字段，或来源不明的双语 LRC 里
 * 相邻同起点、文本不同的两条**主唱**行（和声 / 制作信息行不参与）；音译看 `roman` 字段。
 */
export function describeCandidate(lyrics: LyricLine[]): CandidateCapabilities {
  let wordSynced = false;
  let translation = false;
  let roman = false;
  for (let index = 0; index < lyrics.length; index += 1) {
    const line = lyrics[index];
    if (line.words && line.words.length > 0) wordSynced = true;
    if (line.translations && line.translations.length > 0) translation = true;
    if (line.roman) roman = true;
    const next = lyrics[index + 1];
    if (
      next &&
      (line.role ?? "main") === "main" &&
      (next.role ?? "main") === "main" &&
      Math.abs(next.startMs - line.startMs) <= SAME_TIMESTAMP_EPSILON_MS &&
      next.text !== line.text
    ) {
      translation = true;
    }
    if (wordSynced && translation && roman) break;
  }
  return { wordSynced, translation, roman };
}

/** 候选卡片上的短标签，顺序固定：逐字 / 逐行 → 译文 → 音译。 */
export function candidateBadges(lyrics: LyricLine[]): string[] {
  const caps = describeCandidate(lyrics);
  const badges = [caps.wordSynced ? "逐字" : "逐行"];
  if (caps.translation) badges.push("译文");
  if (caps.roman) badges.push("音译");
  return badges;
}
