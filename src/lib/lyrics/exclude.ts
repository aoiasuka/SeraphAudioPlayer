import type { LyricLine, LyricsExcludeRule } from "@/types/track";

/**
 * 歌词排除规则（设置 → 歌词设置 → 歌词排除配置）。
 *
 * 只在显示层过滤：曲库里的歌词原样保留，规则改动即时生效、随时可逆。
 * 关键词按大小写不敏感的子串匹配；正则按 JS 语法（默认加 `i`），
 * 编译失败的规则跳过而不是让整篇歌词消失。
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

/** 校验正则能否编译；返回错误信息或 null。 */
export function validateRegexPattern(pattern: string): string | null {
  try {
    new RegExp(pattern, "i");
    return null;
  } catch (err) {
    return err instanceof Error ? err.message : "无效的正则表达式";
  }
}

export function compileExcludeRules(
  rules: LyricsExcludeRule[]
): (line: LyricLine) => boolean {
  const keywords: string[] = [];
  const regexes: RegExp[] = [];
  for (const rule of rules) {
    if (rule.kind === "keyword") {
      keywords.push(rule.pattern.toLowerCase());
    } else {
      try {
        regexes.push(new RegExp(rule.pattern, "i"));
      } catch {
        // 坏规则跳过
      }
    }
  }
  if (keywords.length === 0 && regexes.length === 0) return () => false;
  return (line) => {
    const haystacks = [line.text, line.translation ?? "", line.roman ?? ""];
    for (const value of haystacks) {
      if (!value) continue;
      const lower = value.toLowerCase();
      if (keywords.some((keyword) => lower.includes(keyword))) return true;
      if (regexes.some((regex) => regex.test(value))) return true;
    }
    return false;
  };
}

/** 应用排除规则；无规则时原数组直接返回（保持引用稳定，避免无谓重渲染）。 */
export function filterLyricsByRules(
  lyrics: LyricLine[],
  rules: LyricsExcludeRule[]
): LyricLine[] {
  if (rules.length === 0 || lyrics.length === 0) return lyrics;
  const excluded = compileExcludeRules(rules);
  const filtered = lyrics.filter((line) => !excluded(line));
  return filtered.length === lyrics.length ? lyrics : filtered;
}
