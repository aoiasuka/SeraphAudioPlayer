import { useMemo } from "react";
import { useSmoothTime } from "@/hooks/useSmoothTime";
import { wordProgress } from "@/lib/lyrics/activeLine";
import { cn } from "@/lib/utils";
import type { LyricWord } from "@/types/track";

interface KaraokeLineProps {
  words: LyricWord[];
  /** 播放位置锚点（毫秒，可听位置）；平滑外推在组件内部完成 */
  currentMs: number;
  /** 播放中才按墙钟外推；暂停时冻结在锚点 */
  playing?: boolean;
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
 *
 * 平滑时钟（`useSmoothTime`，rAF 每帧外推）**只在这里**跑：此前由父组件持有 `smoothMs`，
 * 每一帧都让整篇歌词列表（几百行）重渲染一次；现在 60 fps 的更新只落在当前行的音节 span 上，
 * 父组件只随后端 Progress 事件（约 10 Hz）重渲染（2026-09-22）。父组件只在当前行挂载它。
 */
export function KaraokeLine({
  words,
  currentMs,
  playing = false,
  lineEndMs,
  className,
  sungColor = "var(--ink)",
  unsungColor = "rgba(43, 39, 34, 0.35)",
}: KaraokeLineProps) {
  const smoothMs = useSmoothTime(currentMs, playing);
  const progress = useMemo(
    () => wordProgress(words, smoothMs, lineEndMs),
    [words, smoothMs, lineEndMs]
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
