import { useEffect, useRef, useState } from "react";

/**
 * 逐字高亮用的平滑时钟（单位：**毫秒**，与歌词模型一致）。
 *
 * store 的播放位置由后端 Progress 事件驱动（百毫秒级），直接拿来驱动
 * 音节高亮会一顿一顿。这里以最近一次位置为锚点，播放中用 rAF 按
 * 墙钟外推，暂停/未播放时冻结在锚点；每次锚点更新都重新校准，不会漂移。
 * 只在 `KaraokeLine` 内部使用（2026-09-22 起）：rAF 每帧的 setState 只重渲染当前行的
 * 音节，不再牵动整篇歌词列表；rAF 循环随当前行卸载而停止，不会常驻。
 */
/**
 * 外推上限（L4-02）：进度事件约 250 ms 一次，锚点之后最多外推这么久。
 * 进度事件停了而播放状态仍是「播放中」（解码卡在断连网络盘、输出断流、曲尾到下一首之间）时，
 * 高亮停在上限处等下一个事件，而不是一直往前走、与实际声音脱节。
 */
export const SMOOTH_TIME_MAX_EXTRAPOLATION_MS = 500;

export function useSmoothTime(currentMs: number, playing: boolean, enabled = true) {
  const [smooth, setSmooth] = useState(currentMs);
  const anchor = useRef({ time: currentMs, at: 0 });

  useEffect(() => {
    anchor.current = {
      time: currentMs,
      at: typeof performance !== "undefined" ? performance.now() : Date.now(),
    };
    setSmooth(currentMs);
  }, [currentMs]);

  useEffect(() => {
    if (!enabled || !playing || typeof requestAnimationFrame !== "function") return;
    let frame = 0;
    const step = () => {
      const now = typeof performance !== "undefined" ? performance.now() : Date.now();
      const elapsed = Math.min(Math.max(0, now - anchor.current.at), SMOOTH_TIME_MAX_EXTRAPOLATION_MS);
      setSmooth(anchor.current.time + elapsed);
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [enabled, playing]);

  return enabled ? smooth : currentMs;
}
