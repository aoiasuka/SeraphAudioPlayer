import { useMemo } from "react";
import { wordProgress } from "@/lib/lyrics/activeLine";
import { cn } from "@/lib/utils";
import type { LyricWord } from "@/types/track";

interface KaraokeLineProps {
  words: LyricWord[];
  /** 平滑后的播放时间（秒） */
  currentTime: number;
  className?: string;
  /** 已唱 / 未唱颜色（CSS 颜色值），默认沿用当前文字色与其 38% 透明版 */
  sungColor?: string;
  unsungColor?: string;
}

/**
 * 逐字歌词行：每个音节用 background-clip 渐变按进度从左到右“填色”。
 * 只依赖 currentTime 与 words，父组件只在当前行挂载它。
 */
export function KaraokeLine({
  words,
  currentTime,
  className,
  sungColor = "currentColor",
  unsungColor = "color-mix(in srgb, currentColor 38%, transparent)",
}: KaraokeLineProps) {
  const progress = useMemo(() => wordProgress(words, currentTime), [words, currentTime]);
  return (
    <span className={cn("karaoke-line", className)} data-testid="karaoke-line">
      {words.map((word, index) => {
        const ratio = progress[index];
        const percent = `${Math.round(ratio * 100)}%`;
        return (
          <span
            key={`${word.start}-${index}`}
            className={cn(
              "karaoke-word",
              ratio >= 1 && "is-sung",
              ratio > 0 && ratio < 1 && "is-singing"
            )}
            data-progress={ratio.toFixed(2)}
            style={{
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
