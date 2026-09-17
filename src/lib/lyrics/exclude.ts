import type { LyricLine, LyricsExcludeRule } from "@/types/track";

/**
 * 歌词排除规则（设置 → 歌词设置 → 歌词排除配置）。
 *
 * v0.6.0 起匹配在 **Rust 侧**完成（`ipc/library/exclude.rs`，`regex` crate）：前端只维护
 * 规则列表并经 `set_lyrics_exclude_rules` 同步；后端回传歌词时给命中的行打
 * `LyricLine.hidden`，这里只按标记过滤。正则语法因此是 Rust 的（无环视、无反向引用），
 * 校验也走 `validate_lyrics_exclude_rules` IPC。
 */

export const MAX_EXCLUDE_RULES = 50;
export const MAX_EXCLUDE_PATTERN_CHARS = 200;

export function sanitizeExcludeRules(value: unknown): LyricsExcludeRule[] {
  if (!Array.isArray(value)) return [];
  const seen = new Set<string>();
  const rules: LyricsExcludeRule[] = [];
  for (const item of value) {
    if (!item || typeof item !== "object") continue;
    const raw = item as Record<string, unknown>;
    const kind = raw.kind === "regex" ? "regex" : raw.kind === "keyword" ? "keyword" : null;
    if (!kind || typeof raw.pattern !== "string") continue;
    const pattern = raw.pattern.trim().slice(0, MAX_EXCLUDE_PATTERN_CHARS);
    if (!pattern) continue;
    const id =
      typeof raw.id === "string" && raw.id ? raw.id : `${kind}:${pattern}`;
    if (seen.has(id)) continue;
    seen.add(id);
    rules.push({ id, kind, pattern });
    if (rules.length >= MAX_EXCLUDE_RULES) break;
  }
  return rules;
}

/** 去掉后端标记为 hidden 的行；没有任何隐藏行时原数组引用直接返回（避免无谓重渲染）。 */
export function visibleLyrics(lyrics: LyricLine[]): LyricLine[] {
  if (lyrics.length === 0 || !lyrics.some((line) => line.hidden)) return lyrics;
  return lyrics.filter((line) => !line.hidden);
}
