import { useEffect, useRef, useState } from "react";

/**
 * 逐字高亮用的平滑时钟。
 *
 * store 的 currentTime 由后端 Progress 事件驱动（百毫秒级），直接拿来驱动
 * 音节高亮会一顿一顿。这里以最近一次 currentTime 为锚点，播放中用 rAF 按
 * 墙钟外推，暂停/未播放时冻结在锚点；每次锚点更新都重新校准，不会漂移。
 * 只在需要逐字渲染的当前行挂载，rAF 循环不会常驻。
 */
export function useSmoothTime(currentTime: number, playing: boolean, enabled = true) {
  const [smooth, setSmooth] = useState(currentTime);
  const anchor = useRef({ time: currentTime, at: 0 });

  useEffect(() => {
    anchor.current = {
      time: currentTime,
      at: typeof performance !== "undefined" ? performance.now() : Date.now(),
    };
    setSmooth(currentTime);
  }, [currentTime]);

  useEffect(() => {
    if (!enabled || !playing || typeof requestAnimationFrame !== "function") return;
    let frame = 0;
    const step = () => {
      const now = typeof performance !== "undefined" ? performance.now() : Date.now();
      setSmooth(anchor.current.time + (now - anchor.current.at) / 1000);
      frame = requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
    return () => cancelAnimationFrame(frame);
  }, [enabled, playing]);

  return enabled ? smooth : currentTime;
}
