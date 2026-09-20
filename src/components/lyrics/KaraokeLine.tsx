import { useMemo } from "react";
import { wordProgress } from "@/lib/lyrics/activeLine";
import { cn } from "@/lib/utils";
import type { LyricWord } from "@/types/track";

interface KaraokeLineProps {
  words: LyricWord[];
  /** 平滑后的播放位置（毫秒） */
  currentMs: number;
  /** 行终点（毫秒）：末音节终点未知时的兜底 */
  lineEndMs?: number;
  className?: string;
  /**
   * 已唱 / 未唱颜色。**必须是具体颜色值，不能用 currentColor**：
   * 音节自身 `color: transparent`（靠 background-clip 显字），currentColor 会随之变透明，
   * 整行看不见（v0.6.0 修过的坑）。
   */
  sungColor?: string;
  unsungColor?: string;
}

/**
 * 逐字歌词行：每个音节用 background-clip 渐变按进度从左到右“填色”。
 * 只依赖 currentMs 与 words，父组件只在当前行挂载它。
 */
export function KaraokeLine({
  words,
  currentMs,
  lineEndMs,
  className,
  sungColor = "var(--ink)",
  unsungColor = "rgba(43, 39, 34, 0.35)",
}: KaraokeLineProps) {
  const progress = useMemo(
    () => wordProgress(words, currentMs, lineEndMs),
    [words, currentMs, lineEndMs]
  );
  return (
    <span className={cn("karaoke-line", className)} data-testid="karaoke-line">
      {words.map((word, index) => {
        const ratio = progress[index];
        const percent = `${Math.round(ratio * 100)}%`;
        return (
          <span
            key={`${word.startMs}-${index}`}
            className={cn(
              "karaoke-word",
              ratio >= 1 && "is-sung",
              ratio > 0 && ratio < 1 && "is-singing"
            )}
            data-progress={ratio.toFixed(2)}
            style={{
              // 兜底色：不支持 background-clip:text 的环境按普通文字显示
              color: sungColor,
              backgroundImage: `linear-gradient(90deg, ${sungColor} ${percent}, ${unsungColor} ${percent})`,
            }}
          >
            {word.text}
          </span>
        );
      })}
    </span>
  );
}
