import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from "react";
import { createAnalysisSession, pollVisualizer } from "@/lib/analysis/polling";
import { applyAnalysisFrame, createAnalysisView, stepAnalysisView } from "@/lib/analysis/view";
import { ANALYSIS_BIN_COUNT, SPECTRUM_DB_FLOOR, LEVEL_DB_FLOOR, type AnalysisFrame } from "@/lib/analysis/types";
import { binFreq, prepCanvas, resolveArchiveColors, spectrumBinFromX, type SpectrumGeometry } from "@/lib/analysis/render";
import { formatSeconds } from "@/lib/format";
import { isTauriRuntime } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { drawImmersiveField, drawImmersiveLevels, drawImmersiveSpectrum, IMMERSIVE_FIELD_TRAIL_FRAMES, levelLabel } from "./analysisRender";

export function useImmersiveAnalysis(playing: boolean, frozen: boolean) {
  const [context] = useState(() => ({ view: createAnalysisView(), sessionId: createAnalysisSession(), trail: [] as Float32Array[], frameTime: 0 }));
  const spectrumRef = useRef<HTMLCanvasElement>(null);
  const fieldRef = useRef<HTMLCanvasElement>(null);
  const levelsRef = useRef<HTMLCanvasElement>(null);
  const frequencyRef = useRef<HTMLOutputElement>(null);
  const correlationRef = useRef<HTMLOutputElement>(null);
  const statusRef = useRef<HTMLSpanElement>(null);
  const frameTimeRef = useRef<HTMLSpanElement>(null);
  const levelsTextRef = useRef<HTMLOutputElement>(null);
  const geometryRef = useRef<SpectrumGeometry>({ x0: 0, x1: 1 });
  const cursorRef = useRef<number | null>(null);
  const scheduleRef = useRef<() => void>();

  useEffect(() => {
    let disposed = false;
    let raf = 0;
    let lastDraw = -Infinity;
    const sizes = new Map<Element, { w: number; h: number }>();
    const colors = resolveArchiveColors();
    const motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");
    const setText = (node: HTMLElement | null, value: string) => {
      if (node && node.textContent !== value) node.textContent = value;
    };
    const draw = (ref: RefObject<HTMLCanvasElement>, painter: (ctx: CanvasRenderingContext2D, w: number, h: number) => void, minimumPixelRatio = 1) => {
      const canvas = ref.current;
      if (!canvas?.parentElement) return;
      const prepared = prepCanvas(canvas, sizes.get(canvas.parentElement), minimumPixelRatio);
      if (prepared) painter(prepared.ctx, prepared.w, prepared.h);
    };

    const render = (time: number) => {
      raf = 0;
      if (disposed || document.visibilityState === "hidden") return;
      const interval = motionQuery.matches ? 100 : playing ? 1000 / 30 : 1000 / 14;
      if (time - lastDraw < interval) { schedule(); return; }
      lastDraw = time;
      const { view, trail } = context;
      if (!frozen) {
        stepAnalysisView(view, time / 1000);
        const points = view.stereo.pts;
        if (points.length && trail[trail.length - 1] !== points) {
          trail.push(points);
          if (trail.length > IMMERSIVE_FIELD_TRAIL_FRAMES) trail.shift();
        } else if (!points.length) trail.length = 0;
      }
      draw(spectrumRef, (ctx, w, h) => { geometryRef.current = drawImmersiveSpectrum(ctx, w, h, view, colors, cursorRef.current); });
      // 声场面积较小，使用至少 2 倍像素密度保留斜线、文字和散点细节。
      draw(fieldRef, (ctx, w, h) => drawImmersiveField(ctx, w, h, colors, trail), 2);
      draw(levelsRef, (ctx, w, h) => drawImmersiveLevels(ctx, w, h, view, colors));
      const bin = cursorRef.current;
      if (bin === null) setText(frequencyRef.current, "20 Hz — 20 kHz · dBFS");
      else {
        const frequency = binFreq(bin, ANALYSIS_BIN_COUNT);
        const label = frequency >= 1000 ? `${(frequency / 1000).toFixed(2)} kHz` : `${Math.round(frequency)} Hz`;
        const db = view.spectrumDb[bin];
        setText(frequencyRef.current, `${label} / ${view.hasData ? db <= SPECTRUM_DB_FLOOR + 0.1 ? `≤ ${SPECTRUM_DB_FLOOR}` : db.toFixed(1) : "—"} dBFS`);
      }
      setText(frameTimeRef.current, `FRAME / ${view.hasData ? formatSeconds(context.frameTime) : "--:--"}`);
      setText(statusRef.current, frozen ? "FROZEN / 已冻结" : !playing ? "PAUSED / 已暂停" : view.hasData && time / 1000 - view.lastFrameAt < 1 ? "LIVE / 实时" : "等待音频信号");
      const correlation = view.stereo.corr;
      setText(correlationRef.current, view.hasData ? `${correlation >= 0 ? "+" : ""}${correlation.toFixed(2)}` : "—");
      correlationRef.current?.parentElement?.classList.toggle("is-negative", view.hasData && correlation < 0);
      setText(levelsTextRef.current, `左声道 RMS ${levelLabel(view.levels.l.rmsDb, view.hasData)}，峰值 ${levelLabel(view.levels.l.holdDb, view.hasData)}；右声道 RMS ${levelLabel(view.levels.r.rmsDb, view.hasData)}，峰值 ${levelLabel(view.levels.r.holdDb, view.hasData)} dBFS`);

      // 无数据、冻结或暂停且余晖衰尽后停帧；新的音频帧、尺寸和游标仍能唤醒绘制。
      const decaying = view.peakHoldDb.some((value) => value > SPECTRUM_DB_FLOOR + 0.15)
        || view.levels.l.holdDb > LEVEL_DB_FLOOR + 0.15 || view.levels.r.holdDb > LEVEL_DB_FLOOR + 0.15;
      if (!frozen && view.hasData && (playing || decaying)) schedule();
    };
    function schedule() {
      if (!disposed && !raf && document.visibilityState !== "hidden") raf = window.requestAnimationFrame(render);
    }
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) sizes.set(entry.target, { w: entry.contentRect.width, h: entry.contentRect.height });
      schedule();
    });
    for (const ref of [spectrumRef, fieldRef, levelsRef]) {
      const parent = ref.current?.parentElement;
      if (parent) {
        sizes.set(parent, { w: parent.clientWidth, h: parent.clientHeight });
        observer.observe(parent);
      }
    }
    const onVisibilityChange = () => { window.cancelAnimationFrame(raf); raf = 0; schedule(); };
    scheduleRef.current = schedule;
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("resize", schedule);
    motionQuery.addEventListener("change", schedule);
    schedule();
    return () => {
      disposed = true;
      window.cancelAnimationFrame(raf);
      observer.disconnect();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("resize", schedule);
      motionQuery.removeEventListener("change", schedule);
      if (scheduleRef.current === schedule) scheduleRef.current = undefined;
    };
  }, [context, playing, frozen]);

  useEffect(() => {
    // 桌面只显示真实引擎帧；浏览器无音频后端时保持等待状态。
    if (!isTauriRuntime() || !playing || frozen) return;
    return pollVisualizer<AnalysisFrame>("get_analysis_frame", context.sessionId, (frame) => {
      applyAnalysisFrame(context.view, frame, performance.now() / 1000);
      context.frameTime = usePlayerStore.getState().currentTime;
      scheduleRef.current?.();
    }, { spectrum: true, loudness: false, levels: true, field: true, scope: false });
  }, [context, playing, frozen]);

  const onPointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    cursorRef.current = spectrumBinFromX(event.clientX - event.currentTarget.getBoundingClientRect().left, geometryRef.current, ANALYSIS_BIN_COUNT);
    scheduleRef.current?.();
  };
  const onPointerLeave = () => { cursorRef.current = null; scheduleRef.current?.(); };
  const onKeyDown = (event: KeyboardEvent<HTMLCanvasElement>) => {
    let bin = cursorRef.current ?? Math.floor(ANALYSIS_BIN_COUNT / 2);
    if (event.key === "ArrowLeft") bin -= 1;
    else if (event.key === "ArrowRight") bin += 1;
    else if (event.key === "Home") bin = 0;
    else if (event.key === "End") bin = ANALYSIS_BIN_COUNT - 1;
    else return;
    event.preventDefault();
    cursorRef.current = Math.max(0, Math.min(ANALYSIS_BIN_COUNT - 1, bin));
    scheduleRef.current?.();
  };
  return { spectrumRef, fieldRef, levelsRef, frequencyRef, correlationRef, statusRef, frameTimeRef, levelsTextRef, onPointerMove, onPointerLeave, onKeyDown };
}
