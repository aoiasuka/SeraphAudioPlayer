import { useEffect, useRef } from "react";
import type { Track } from "@/types/track";

const BAR_COUNT = 96;
const SIDE_PADDING = 0;
const CSS_HEIGHT = 32;
const CYAN = [8, 145, 178] as const;
const SLATE = [71, 85, 105] as const;
const CYAN_RGB = CYAN.join(", ");
const SLATE_RGB = SLATE.join(", ");

function getWaveformShape(trackId: string, barCount: number, h: number) {
  // L-11: 用完整 trackId hash 生成多样化 seed，避免所有曲目共用同一形状。
  let hash = 0;
  for (let i = 0; i < trackId.length; i += 1) {
    hash = (hash * 31 + trackId.charCodeAt(i)) >>> 0;
  }
  const seed = 0.8 + ((hash >>> 8) & 0xff) / 255 * 1.4; // 0.8 ~ 2.2 之间
  const phase = (hash & 0xff) / 255 * Math.PI;
  const shape: number[] = [];
  for (let i = 0; i < barCount; i++) {
    const envelope = Math.sin((i / barCount) * Math.PI);
    const peak1 = Math.sin(i * 0.15 * seed + phase) * 0.28;
    const peak2 = Math.cos(i * 0.35 + seed) * 0.14;
    const detail = Math.sin(i * 0.85) * 0.05;
    let amp = (0.35 + peak1 + peak2 + detail) * envelope;
    amp = Math.max(0.12, amp);
    shape.push(h * 0.25 + amp * h * 0.7);
  }
  return shape;
}

function mixColor(
  from: readonly [number, number, number],
  to: readonly [number, number, number],
  amount: number
) {
  const t = Math.max(0, Math.min(1, amount));
  return from.map((value, index) =>
    Math.round(value + (to[index] - value) * t)
  );
}

/**
 * 把 HTML 设计稿中的 Canvas 波形迁到 React：
 * - 静态 baseline（按 trackId 生成）
 * - 时间相关的呼吸 / 涟漪叠加
 * - 已播 / 未播双色（cyan 渐变 vs slate 半透明）
 */
export function useWaveform(
  canvasRef: React.RefObject<HTMLCanvasElement | null>,
  options: {
    track: Track | null;
    currentTime: number;
    isPlaying: boolean;
  }
) {
  const { track, currentTime, isPlaying } = options;
  const playbackRef = useRef({ currentTime, isPlaying });
  const scheduleDrawRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    const wasPlaying = playbackRef.current.isPlaying;
    playbackRef.current = { currentTime, isPlaying };

    if (!isPlaying || (!wasPlaying && isPlaying)) {
      scheduleDrawRef.current?.();
    }
  }, [currentTime, isPlaying]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !track) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const canvasEl = canvas;
    const context = ctx;
    const activeTrack = track;

    let raf = 0;
    let cssWidth = 0;
    let cssHeight = CSS_HEIGHT;
    let dpr = window.devicePixelRatio || 1;
    let baseline = getWaveformShape(activeTrack.id, BAR_COUNT, cssHeight);

    const resize = () => {
      const parent = canvasEl.parentElement;
      if (!parent) return;
      const nextDpr = window.devicePixelRatio || 1;
      const nextCssWidth = Math.max(0, parent.clientWidth - SIDE_PADDING * 2);
      const nextCssHeight = CSS_HEIGHT;
      const nextWidth = Math.round(nextCssWidth * nextDpr);
      const nextHeight = Math.round(nextCssHeight * nextDpr);

      cssWidth = nextCssWidth;
      cssHeight = nextCssHeight;
      dpr = nextDpr;
      baseline = getWaveformShape(activeTrack.id, BAR_COUNT, cssHeight);

      if (canvasEl.width !== nextWidth) canvasEl.width = nextWidth;
      if (canvasEl.height !== nextHeight) canvasEl.height = nextHeight;
      canvasEl.style.width = `${cssWidth}px`;
      canvasEl.style.height = `${cssHeight}px`;
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
    };

    function draw(time = 0) {
      if (cssWidth <= 0) {
        raf = 0;
        return;
      }

      context.clearRect(0, 0, cssWidth, cssHeight);

      const hasDuration = activeTrack.duration > 0;
      const { currentTime: latestTime, isPlaying: latestIsPlaying } =
        playbackRef.current;
      const ratio = hasDuration
        ? Math.max(0, Math.min(1, latestTime / activeTrack.duration))
        : 0;
      const barWidth = cssWidth / BAR_COUNT;
      const fadeStart = ratio - 0.014;
      const fadeEnd = ratio + 0.038;

      for (let i = 0; i < BAR_COUNT; i++) {
        const harmonic = latestIsPlaying
          ? Math.sin(time * 0.006 - i * 0.18) * 0.06
          : 0;
        const dynamicScale = 1.0 + (latestIsPlaying ? harmonic : 0);
        const baseValue = baseline[i] * dynamicScale;
        const barH = Math.max(3, Math.min(cssHeight, baseValue));

        const x = i * barWidth;
        const y = (cssHeight - barH) / 2;
        const barRatio = i / BAR_COUNT;

        if (!hasDuration) {
          const shimmer = 0.12 + Math.sin(i * 0.34) * 0.04;
          context.fillStyle = `rgba(${CYAN_RGB}, ${shimmer})`;
        } else if (barRatio <= fadeStart) {
          context.fillStyle = `rgba(${CYAN_RGB}, ${0.68 + barRatio * 0.22})`;
        } else if (barRatio >= fadeEnd) {
          context.fillStyle = `rgba(${SLATE_RGB}, 0.18)`;
        } else {
          const fade = (barRatio - fadeStart) / (fadeEnd - fadeStart);
          const [r, g, b] = mixColor(CYAN, SLATE, fade);
          const alpha = 0.66 + (0.18 - 0.66) * fade;
          context.fillStyle = `rgba(${r}, ${g}, ${b}, ${alpha})`;
        }

        const r = 2;
        const w = Math.max(2, barWidth - 3);
        const xx = x + 1.5;
        context.beginPath();
        const ctxWithRound = context as CanvasRenderingContext2D & {
          roundRect?: (
            x: number,
            y: number,
            w: number,
            h: number,
            r: number
          ) => void;
        };
        if (typeof ctxWithRound.roundRect === "function") {
          ctxWithRound.roundRect(xx, y, w, barH, r);
        } else {
          context.rect(xx, y, w, barH);
        }
        context.fill();
      }

      if (playbackRef.current.isPlaying) {
        raf = requestAnimationFrame(draw);
      } else {
        raf = 0;
      }
    }

    const scheduleDraw = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(draw);
    };

    const handleResize = () => {
      resize();
      scheduleDraw();
    };

    resize();
    window.addEventListener("resize", handleResize);
    const parentElement = canvasEl.parentElement;
    const resizeObserver =
      typeof ResizeObserver === "undefined" || !parentElement
        ? null
        : new ResizeObserver(handleResize);

    if (resizeObserver && parentElement) {
      resizeObserver.observe(parentElement);
    }
    scheduleDrawRef.current = scheduleDraw;
    scheduleDraw();

    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", handleResize);
      resizeObserver?.disconnect();
      if (scheduleDrawRef.current === scheduleDraw) {
        scheduleDrawRef.current = null;
      }
    };
  }, [canvasRef, track]);
}

export const WAVEFORM_SIDE_PADDING = SIDE_PADDING;
