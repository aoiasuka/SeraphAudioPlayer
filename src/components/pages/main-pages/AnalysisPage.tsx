import { useEffect, useRef, useState } from "react";
import {
  binFreq,
  drawLevelsMeter,
  drawLoudnessDeviationBar,
  drawOscilloscope,
  drawSoundField,
  drawSpectrogram,
  drawSpectrumChart,
  drawVuMeter,
  formatFreq,
  prepCanvas,
  resolveArchiveColors,
  spectrumBinFromX,
  type ArchiveColors,
  type SpectrumGeometry,
} from "@/lib/analysis/render";
import { createAnalysisSimulator } from "@/lib/analysis/simulator";
import {
  applyAnalysisFrame,
  createAnalysisView,
  resetAnalysisSession,
  stepAnalysisView,
} from "@/lib/analysis/view";
import { ANALYSIS_BIN_COUNT, type AnalysisFrame } from "@/lib/analysis/types";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import {
  useAnalysisSettingsStore,
  type AnalysisPanelId,
} from "@/store/analysisSettings";
import { usePlayerStore } from "@/store/player";

const POLL_INTERVAL_MS = 33; // ~30fps 数据泵
/** 渲染节流：播放时 ~30fps，待机衰减时 ~14fps，都低于 rAF 的 60fps */
const DRAW_INTERVAL_ACTIVE = 1 / 32;
const DRAW_INTERVAL_STANDBY = 1 / 14;

const fmt1 = (value: number | null) => (value === null ? "--.-" : value.toFixed(1));

/** 面板套壳：档案编号 + 标题 + 右侧实时读数位 */
function Panel({
  no,
  title,
  metaRef,
  metaText,
  className,
  style,
  children,
}: {
  no: string;
  title: string;
  metaRef?: React.MutableRefObject<HTMLSpanElement | null>;
  metaText?: string;
  className?: string;
  style?: React.CSSProperties;
  children: React.ReactNode;
}) {
  return (
    <section
      style={style}
      className={cn(
        "flex min-h-0 flex-col border-[1.5px] border-ink bg-card shadow-[3px_3px_0_rgba(43,39,34,0.1)]",
        className
      )}
    >
      <header className="flex items-center gap-2.5 border-b-[1.5px] border-line px-2.5 py-1.5">
        <span className="shrink-0 bg-ink px-1.5 py-0.5 font-tw text-[9px] font-bold tracking-[2px] text-paper">
          {no}
        </span>
        <h2 className="truncate font-tw text-[10px] font-bold tracking-[2px] text-ink2">
          {title}
        </h2>
        <span
          ref={metaRef}
          className="ml-auto whitespace-nowrap font-tw text-[10px] font-bold text-brown"
        >
          {metaText ?? ""}
        </span>
      </header>
      <div className="flex min-h-0 flex-1 flex-col p-2.5">{children}</div>
    </section>
  );
}

function ModeTab({
  active,
  label,
  onClick,
}: {
  active: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={cn(
        "border-[1.5px] px-2.5 py-0.5 font-tw text-[10px] font-bold tracking-[1.5px] transition-colors",
        active
          ? "border-ink bg-ink text-paper"
          : "border-line bg-card text-ink2 hover:border-ink hover:text-ink"
      )}
    >
      {label}
    </button>
  );
}

/** 设置浮层里的档案风勾选行 */
function SettingToggle({
  label,
  checked,
  onChange,
  indent,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean) => void;
  indent?: boolean;
}) {
  return (
    <label
      className={cn(
        "flex cursor-pointer items-center gap-2 py-0.5 font-tw text-[11px] text-ink2 hover:text-ink",
        indent && "pl-5"
      )}
    >
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked)}
        className="sr-only"
      />
      <span
        aria-hidden
        className={cn(
          "flex h-3.5 w-3.5 shrink-0 items-center justify-center border-[1.5px] border-ink text-[9px] font-bold leading-none",
          checked ? "bg-ink text-paper" : "bg-card text-transparent"
        )}
      >
        ×
      </span>
      <span className="truncate">{label}</span>
    </label>
  );
}

/** 2xl（1536px）断点检测：宽屏用仪表网格，窄屏回退纵向滚动 */
function useWideLayout() {
  const [wide, setWide] = useState(
    () =>
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia("(min-width: 1536px)").matches
  );
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return undefined;
    const query = window.matchMedia("(min-width: 1536px)");
    const onChange = () => setWide(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return wide;
}

const RIGHT_PANEL_ORDER: AnalysisPanelId[] = [
  "spectrum",
  "scope",
  "spectrogram",
  "field",
];

interface WideLayout {
  container: React.CSSProperties;
  leftCol: React.CSSProperties | null;
  place: Partial<Record<AnalysisPanelId, React.CSSProperties>>;
}

/**
 * 按可见面板集合计算宽屏网格：
 * 左列固定放响度 + 电平；右区按 [频谱, 示波器, 瀑布, 声场] 的可见顺序填充——
 * 4 块 = 频谱大幅 / 示波器扁条 / 瀑布+声场同行；3 块 = 首块大幅 + 两块同行；
 * 2 块 = 各占整行；1 块 = 独占。面板隐藏后其余自动补位。
 */
function computeWideLayout(panels: Record<AnalysisPanelId, boolean>): WideLayout {
  const hasLeft = panels.loudness || panels.levels;
  const right = RIGHT_PANEL_ORDER.filter((id) => panels[id]);
  const place: WideLayout["place"] = {};
  const c1 = hasLeft ? 2 : 1;
  const c2 = c1 + 1;
  const span = `${c1} / ${c2 + 1}`;
  const at = (column: string, row: string): React.CSSProperties => ({
    gridColumn: column,
    gridRow: row,
    minHeight: 0,
    minWidth: 0,
  });

  let rowsTemplate = "minmax(0,1fr)";
  if (right.length === 4) {
    rowsTemplate = "minmax(0,1fr) minmax(0,0.55fr) minmax(0,1fr)";
    place[right[0]] = at(span, "1");
    place[right[1]] = at(span, "2");
    place[right[2]] = at(String(c1), "3");
    place[right[3]] = at(String(c2), "3");
  } else if (right.length === 3) {
    rowsTemplate = "minmax(0,1.15fr) minmax(0,1fr)";
    place[right[0]] = at(span, "1");
    place[right[1]] = at(String(c1), "2");
    place[right[2]] = at(String(c2), "2");
  } else if (right.length === 2) {
    rowsTemplate = "minmax(0,1fr) minmax(0,1fr)";
    place[right[0]] = at(span, "1");
    place[right[1]] = at(span, "2");
  } else if (right.length === 1) {
    place[right[0]] = at(span, "1");
  }
  const rowCount = right.length === 4 ? 3 : right.length >= 2 ? 2 : 1;

  const columns =
    right.length === 0
      ? "minmax(0,1fr)"
      : hasLeft
        ? "minmax(300px,29%) minmax(0,1.15fr) minmax(0,1fr)"
        : "minmax(0,1.15fr) minmax(0,1fr)";

  return {
    container: {
      display: "grid",
      gridTemplateColumns: columns,
      gridTemplateRows: rowsTemplate,
      gap: "0.75rem",
      overflow: "hidden",
    },
    leftCol: hasLeft
      ? { gridColumn: "1", gridRow: `1 / ${rowCount + 1}`, minHeight: 0 }
      : null,
    place,
  };
}

/**
 * 声学分析页：响度（EBU R128）/ 电平（条表·VU）/ 声场 / 频谱 / 瀑布 / 示波器
 * 六仪表。桌面运行时由播放输出的真实样本驱动（get_analysis_frame ~30fps 轮询）；
 * 纯浏览器开发模式回退到内置模拟信号源。面板可见性与显示内容由
 * 声学分析设置（页眉「PANELS」浮层）控制并持久化。
 */
export function AnalysisPage() {
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const currentTrackId = usePlayerStore((s) => s.currentTrack()?.id ?? null);

  const settings = useAnalysisSettingsStore();
  const {
    panels,
    loudnessTarget,
    loudnessShowDeviation,
    levelsMode,
    levelsShowPeak,
    levelsShowRms,
    fieldMode,
    fieldShowCorrelation,
    spectrumShowPeakHold,
    spectrogramMode,
    scopeSplit,
    scopeTrigger,
    setPanelVisible,
  } = settings;

  const wide = useWideLayout();
  const layout = wide ? computeWideLayout(panels) : null;
  const visibleCount = RIGHT_PANEL_ORDER.filter((id) => panels[id]).length
    + (panels.loudness ? 1 : 0)
    + (panels.levels ? 1 : 0);

  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsBoxRef = useRef<HTMLDivElement | null>(null);

  const viewRef = useRef(createAnalysisView());
  const colorsRef = useRef<ArchiveColors | null>(null);
  const trailRef = useRef<Float32Array[]>([]);
  const cursorBinRef = useRef<number | null>(null);
  const spectrumGeomRef = useRef<SpectrumGeometry | null>(null);
  const heatCanvasRef = useRef<HTMLCanvasElement | null>(null);
  const spectroCacheRef = useRef<{ canvas: HTMLCanvasElement; key: string } | null>(
    null
  );
  const lastDrawRef = useRef(0);

  // 设置项经 ref 供渲染循环读取（不重建 rAF 循环）
  const optsRef = useRef({
    fieldMode,
    fieldShowCorrelation,
    spectrogramMode,
    levelsMode,
    levelsShowPeak,
    levelsShowRms,
    spectrumShowPeakHold,
    scopeSplit,
    scopeTrigger,
    standby: false,
  });
  optsRef.current.fieldMode = fieldMode;
  optsRef.current.fieldShowCorrelation = fieldShowCorrelation;
  optsRef.current.spectrogramMode = spectrogramMode;
  optsRef.current.levelsMode = levelsMode;
  optsRef.current.levelsShowPeak = levelsShowPeak;
  optsRef.current.levelsShowRms = levelsShowRms;
  optsRef.current.spectrumShowPeakHold = spectrumShowPeakHold;
  optsRef.current.scopeSplit = scopeSplit;
  optsRef.current.scopeTrigger = scopeTrigger;

  const loudBarCanvas = useRef<HTMLCanvasElement | null>(null);
  const levelsCanvas = useRef<HTMLCanvasElement | null>(null);
  const fieldCanvas = useRef<HTMLCanvasElement | null>(null);
  const spectrumCanvas = useRef<HTMLCanvasElement | null>(null);
  const cascadeCanvas = useRef<HTMLCanvasElement | null>(null);
  const scopeCanvas = useRef<HTMLCanvasElement | null>(null);

  const loudMeta = useRef<HTMLSpanElement | null>(null);
  const fieldMeta = useRef<HTMLSpanElement | null>(null);
  const spectrumMeta = useRef<HTMLSpanElement | null>(null);
  const readouts = useRef<Record<string, HTMLElement | null>>({});
  const setReadout = (key: string) => (el: HTMLElement | null) => {
    readouts.current[key] = el;
  };

  viewRef.current.loud.target = loudnessTarget;

  // 画布容器尺寸：ResizeObserver 缓存，渲染循环零 layout 读取
  const sizesRef = useRef(new WeakMap<Element, { w: number; h: number }>());
  const roRef = useRef<ResizeObserver | null>(null);
  const observeBox = (el: HTMLDivElement | null) => {
    if (!el || typeof ResizeObserver === "undefined") return;
    roRef.current ??= new ResizeObserver((entries) => {
      for (const entry of entries) {
        sizesRef.current.set(entry.target, {
          w: entry.contentRect.width,
          h: entry.contentRect.height,
        });
      }
    });
    roRef.current.observe(el);
  };
  useEffect(() => () => roRef.current?.disconnect(), []);

  const standby = isTauriRuntime() && !isPlaying;
  optsRef.current.standby = standby;

  // 数据泵：桌面轮询后端；纯浏览器用模拟器
  useEffect(() => {
    const view = viewRef.current;
    if (isTauriRuntime()) {
      if (!isPlaying) return undefined;
      const timer = window.setInterval(() => {
        void invoke<AnalysisFrame | null>("get_analysis_frame")
          .then((frame) => {
            if (frame) applyAnalysisFrame(view, frame, performance.now() / 1000);
          })
          .catch(() => {
            // 轮询失败静默；下一拍重试
          });
      }, POLL_INTERVAL_MS);
      return () => window.clearInterval(timer);
    }
    const simulator = createAnalysisSimulator();
    const timer = window.setInterval(() => {
      applyAnalysisFrame(
        view,
        simulator.next(POLL_INTERVAL_MS / 1000),
        performance.now() / 1000
      );
    }, POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [isPlaying]);

  // 换曲目：清后端积分/LRA/真峰会话值，前端同步清空显示
  useEffect(() => {
    if (!currentTrackId) return;
    resetAnalysisSession(viewRef.current);
    if (isTauriRuntime()) void invoke("reset_analysis_meters").catch(() => {});
  }, [currentTrackId]);

  // 设置浮层外点关闭
  useEffect(() => {
    if (!settingsOpen) return undefined;
    const onPointerDown = (event: PointerEvent) => {
      const box = settingsBoxRef.current;
      if (box && !box.contains(event.target as Node)) setSettingsOpen(false);
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [settingsOpen]);

  // 渲染循环：节流步进弹道学 + 画可见画布 + 刷新数字读数
  useEffect(() => {
    let disposed = false;
    let raf = 0;

    const renderLoop = () => {
      if (disposed) return;
      raf = window.requestAnimationFrame(renderLoop);
      const now = performance.now() / 1000;
      const opts = optsRef.current;
      const minInterval = opts.standby
        ? DRAW_INTERVAL_STANDBY
        : DRAW_INTERVAL_ACTIVE;
      if (now - lastDrawRef.current < minInterval) return;
      lastDrawRef.current = now;

      const view = viewRef.current;
      stepAnalysisView(view, now);
      const colors = (colorsRef.current ??= resolveArchiveColors());

      // 声场余晖：保留最近 6 帧散点
      const trail = trailRef.current;
      if (view.stereo.pts.length > 0) {
        const latest = trail[trail.length - 1];
        if (!latest || latest !== view.stereo.pts) {
          trail.push(view.stereo.pts);
          if (trail.length > 6) trail.shift();
        }
      }

      const draw = (
        canvasRef: React.RefObject<HTMLCanvasElement | null>,
        painter: (ctx: CanvasRenderingContext2D, w: number, h: number) => void
      ) => {
        const canvas = canvasRef.current;
        if (!canvas) return;
        const size = canvas.parentElement
          ? sizesRef.current.get(canvas.parentElement)
          : undefined;
        const prepared = prepCanvas(canvas, size);
        if (!prepared) return;
        painter(prepared.ctx, prepared.w, prepared.h);
      };

      draw(loudBarCanvas, (ctx, w, h) =>
        drawLoudnessDeviationBar(ctx, w, h, view, colors)
      );
      draw(levelsCanvas, (ctx, w, h) => {
        if (opts.levelsMode === "vu") {
          drawVuMeter(ctx, w, h, view, colors);
        } else {
          drawLevelsMeter(ctx, w, h, view, colors, {
            showPeak: opts.levelsShowPeak,
            showRms: opts.levelsShowRms,
          });
        }
      });
      draw(fieldCanvas, (ctx, w, h) =>
        drawSoundField(
          ctx,
          w,
          h,
          view,
          colors,
          opts.fieldMode,
          trail,
          opts.fieldShowCorrelation
        )
      );
      draw(spectrumCanvas, (ctx, w, h) => {
        spectrumGeomRef.current = drawSpectrumChart(
          ctx,
          w,
          h,
          view,
          colors,
          cursorBinRef.current,
          { showPeakHold: opts.spectrumShowPeakHold }
        );
      });
      draw(scopeCanvas, (ctx, w, h) =>
        drawOscilloscope(ctx, w, h, view, colors, {
          split: opts.scopeSplit,
          trigger: opts.scopeTrigger,
        })
      );
      draw(cascadeCanvas, (ctx, w, h) => {
        // 瀑布离屏缓存：仅在走纸推进 / 模式 / 尺寸变化时重绘（~11fps），其余帧 blit
        const dpr = window.devicePixelRatio || 1;
        const key = `${view.historyVersion}|${opts.spectrogramMode}|${w}x${h}|${dpr}`;
        let cache = spectroCacheRef.current;
        if (!cache) {
          cache = { canvas: document.createElement("canvas"), key: "" };
          spectroCacheRef.current = cache;
        }
        if (cache.key !== key) {
          const cacheW = Math.max(1, Math.round(w * dpr));
          const cacheH = Math.max(1, Math.round(h * dpr));
          if (cache.canvas.width !== cacheW || cache.canvas.height !== cacheH) {
            cache.canvas.width = cacheW;
            cache.canvas.height = cacheH;
          }
          const cacheCtx = cache.canvas.getContext("2d");
          if (!cacheCtx) return;
          cacheCtx.setTransform(dpr, 0, 0, dpr, 0, 0);
          heatCanvasRef.current ??= document.createElement("canvas");
          drawSpectrogram(
            cacheCtx,
            w,
            h,
            view,
            colors,
            opts.spectrogramMode,
            heatCanvasRef.current
          );
          cache.key = key;
        }
        ctx.clearRect(0, 0, w, h);
        ctx.drawImage(cache.canvas, 0, 0, w, h);
      });

      // 数字读数（直接写 textContent，避免 30fps 重渲染整页）
      const nodes = readouts.current;
      const set = (key: string, text: string) => {
        const el = nodes[key];
        if (el && el.textContent !== text) el.textContent = text;
      };
      set("loudM", fmt1(view.loud.m));
      set("loudS", fmt1(view.loud.s));
      set("loudI", fmt1(view.loud.i));
      set("loudLra", view.loud.lra === null ? "-.-" : view.loud.lra.toFixed(1));
      set("loudTp", fmt1(view.loud.tpMax));
      set("peakL", fmt1(view.levels.l.holdDb));
      set("peakR", fmt1(view.levels.r.holdDb));
      set("rmsL", fmt1(view.levels.l.rmsDb));
      set("rmsR", fmt1(view.levels.r.rmsDb));

      const over = view.loud.tpMax !== null && view.loud.tpMax > -1;
      nodes.overFlag?.classList.toggle("bg-stamp", over);
      nodes.overFlag?.classList.toggle("border-stamp", over);
      nodes.overFlag?.classList.toggle("text-paper", over);
      nodes.clipFlag?.classList.toggle("bg-stamp", view.levels.clip);
      nodes.clipFlag?.classList.toggle("border-stamp", view.levels.clip);
      nodes.clipFlag?.classList.toggle("text-paper", view.levels.clip);

      if (loudMeta.current) {
        const dev = view.loud.i === null ? null : view.loud.i - view.loud.target;
        loudMeta.current.textContent =
          dev === null ? "" : `Δ TARGET ${dev >= 0 ? "+" : ""}${dev.toFixed(1)} LU`;
      }
      if (fieldMeta.current) {
        const corr = view.stereo.corr;
        fieldMeta.current.textContent = `ρ ${corr >= 0 ? "+" : ""}${corr.toFixed(2)}`;
      }
      if (spectrumMeta.current) {
        const bin = cursorBinRef.current;
        spectrumMeta.current.textContent =
          bin === null
            ? `${ANALYSIS_BIN_COUNT} BANDS · 20Hz–20kHz`
            : `${formatFreq(binFreq(bin, ANALYSIS_BIN_COUNT))} Hz · ${view.spectrumDb[bin].toFixed(1)} dB`;
      }
    };
    raf = window.requestAnimationFrame(renderLoop);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(raf);
    };
  }, []);

  const narrowPanel = "min-h-[280px] shrink-0";

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      {/* 页眉行：档案标识 + 待机印 + 面板设置入口 */}
      <div className="mb-2 flex h-7 shrink-0 items-center gap-2.5">
        <span className="bg-ink px-2 py-0.5 font-tw text-[10px] font-bold tracking-[2px] text-paper">
          ACOUSTIC ANALYSIS
        </span>
        <span className="hidden truncate font-tw text-[10px] font-bold tracking-[1px] text-ink3 sm:inline">
          声学分析 · 实时仪表档案
        </span>
        {standby ? (
          <span className="border-[1.5px] border-dashed border-brown px-2 py-0.5 font-tw text-[9px] font-bold tracking-[1px] text-brown">
            待机 HOLD · 播放后实时运行
          </span>
        ) : null}
        <div ref={settingsBoxRef} className="relative ml-auto">
          <button
            type="button"
            aria-expanded={settingsOpen}
            onClick={() => setSettingsOpen((open) => !open)}
            className={cn(
              "border-[1.5px] px-2.5 py-0.5 font-tw text-[10px] font-bold tracking-[1.5px] transition-colors",
              settingsOpen
                ? "border-ink bg-ink text-paper"
                : "border-ink bg-card text-ink hover:bg-paper2"
            )}
          >
            PANELS 面板设置
          </button>
          {settingsOpen ? (
            <div className="absolute right-0 top-8 z-40 max-h-[70vh] w-[300px] overflow-y-auto border-[1.5px] border-ink bg-card p-3 shadow-[4px_4px_0_rgba(43,39,34,0.18)]">
              <p className="mb-2 border-b-[1.5px] border-line pb-1.5 font-tw text-[10px] font-bold tracking-[2px] text-ink2">
                ANALYSIS SETTINGS · 显示内容
              </p>
              <div className="space-y-2">
                <div>
                  <SettingToggle
                    label="NO.01 LOUDNESS · 响度"
                    checked={panels.loudness}
                    onChange={(v) => setPanelVisible("loudness", v)}
                  />
                  {panels.loudness ? (
                    <SettingToggle
                      indent
                      label="目标偏差标尺行"
                      checked={loudnessShowDeviation}
                      onChange={settings.setLoudnessShowDeviation}
                    />
                  ) : null}
                </div>
                <div>
                  <SettingToggle
                    label="NO.02 LEVELS · 电平表"
                    checked={panels.levels}
                    onChange={(v) => setPanelVisible("levels", v)}
                  />
                  {panels.levels && levelsMode === "bar" ? (
                    <>
                      <SettingToggle
                        indent
                        label="PEAK 峰值行"
                        checked={levelsShowPeak}
                        onChange={settings.setLevelsShowPeak}
                      />
                      <SettingToggle
                        indent
                        label="RMS 均方根行"
                        checked={levelsShowRms}
                        onChange={settings.setLevelsShowRms}
                      />
                    </>
                  ) : null}
                </div>
                <div>
                  <SettingToggle
                    label="NO.03 SOUND FIELD · 声场"
                    checked={panels.field}
                    onChange={(v) => setPanelVisible("field", v)}
                  />
                  {panels.field ? (
                    <SettingToggle
                      indent
                      label="相关度表（ρ）"
                      checked={fieldShowCorrelation}
                      onChange={settings.setFieldShowCorrelation}
                    />
                  ) : null}
                </div>
                <div>
                  <SettingToggle
                    label="NO.04 SPECTRUM · 频谱"
                    checked={panels.spectrum}
                    onChange={(v) => setPanelVisible("spectrum", v)}
                  />
                  {panels.spectrum ? (
                    <SettingToggle
                      indent
                      label="峰值保持虚线"
                      checked={spectrumShowPeakHold}
                      onChange={settings.setSpectrumShowPeakHold}
                    />
                  ) : null}
                </div>
                <SettingToggle
                  label="NO.05 SPECTROGRAM · 瀑布"
                  checked={panels.spectrogram}
                  onChange={(v) => setPanelVisible("spectrogram", v)}
                />
                <div>
                  <SettingToggle
                    label="NO.06 OSCILLOSCOPE · 示波器"
                    checked={panels.scope}
                    onChange={(v) => setPanelVisible("scope", v)}
                  />
                  {panels.scope ? (
                    <>
                      <SettingToggle
                        indent
                        label="触发对齐（锁相稳定波形）"
                        checked={scopeTrigger}
                        onChange={settings.setScopeTrigger}
                      />
                      <SettingToggle
                        indent
                        label="L/R 分离显示"
                        checked={scopeSplit}
                        onChange={settings.setScopeSplit}
                      />
                    </>
                  ) : null}
                </div>
              </div>
              <button
                type="button"
                onClick={settings.resetAnalysisSettings}
                className="mt-3 w-full border-[1.5px] border-line px-2 py-1 font-tw text-[10px] font-bold tracking-[1.5px] text-ink2 transition-colors hover:border-ink hover:text-ink"
              >
                恢复默认设置
              </button>
            </div>
          ) : null}
        </div>
      </div>

      {visibleCount === 0 ? (
        <div className="flex min-h-0 flex-1 items-center justify-center border-[1.5px] border-dashed border-line">
          <p className="font-tw text-[11px] font-bold tracking-[1px] text-ink3">
            全部面板已隐藏 —— 在右上角「PANELS 面板设置」中重新开启
          </p>
        </div>
      ) : (
        <div
          style={layout?.container}
          className={cn(
            "min-h-0 flex-1",
            !wide && "flex flex-col gap-3 overflow-y-auto pr-1"
          )}
        >
          {/* 左列：响度（自适应高度）+ 电平表（吃掉剩余高度） */}
          {panels.loudness || panels.levels ? (
            <div
              style={layout?.leftCol ?? undefined}
              className={cn("flex flex-col gap-3", wide ? "min-h-0" : "shrink-0")}
            >
              {panels.loudness ? (
                <Panel
                  no="NO.01"
                  title="LOUDNESS · 响度"
                  metaRef={loudMeta}
                  className="shrink-0"
                >
                  <div className="grid grid-cols-3 items-end gap-2 border-b-[1.5px] border-line pb-2.5 pt-1 text-center">
                    <div>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        SHORT TERM 短期
                      </span>
                      <span
                        ref={setReadout("loudS")}
                        className="font-tw text-[clamp(21px,1.4vw,27px)] font-bold leading-tight text-ink [font-variant-numeric:tabular-nums]"
                      >
                        --.-
                      </span>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        LUFS · 3S
                      </span>
                    </div>
                    <div>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        INTEGRATED 整体
                      </span>
                      <span
                        ref={setReadout("loudI")}
                        className="font-tw text-[clamp(29px,2vw,40px)] font-bold leading-tight text-stamp [font-variant-numeric:tabular-nums]"
                      >
                        --.-
                      </span>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        LUFS
                      </span>
                    </div>
                    <div>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        MOMENTARY 瞬时
                      </span>
                      <span
                        ref={setReadout("loudM")}
                        className="font-tw text-[clamp(21px,1.4vw,27px)] font-bold leading-tight text-ink [font-variant-numeric:tabular-nums]"
                      >
                        --.-
                      </span>
                      <span className="block font-tw text-[9px] font-bold tracking-[1.5px] text-ink3">
                        LUFS · 400MS
                      </span>
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-1 py-2 font-tw text-[11px] text-ink2">
                    <span>
                      LRA <b ref={setReadout("loudLra")} className="text-ink">-.-</b> LU
                    </span>
                    <span>
                      TRUE PEAK MAX{" "}
                      <b ref={setReadout("loudTp")} className="text-ink">--.-</b> dBTP≈
                    </span>
                    <span
                      ref={setReadout("overFlag")}
                      className="border-[1.5px] border-line px-1.5 py-0.5 font-tw text-[9px] font-bold tracking-[2px] text-ink3 transition-colors"
                    >
                      OVER
                    </span>
                  </div>
                  {loudnessShowDeviation ? (
                    <div className="flex items-center gap-2.5 border-t-[1.5px] border-line pt-2">
                      <label className="flex shrink-0 items-center gap-2 font-tw text-[10px] font-bold tracking-[1px] text-ink3">
                        TARGET 目标
                        <select
                          value={loudnessTarget}
                          onChange={(event) =>
                            settings.setLoudnessTarget(Number(event.target.value))
                          }
                          className="h-7 cursor-pointer border-[1.5px] border-line bg-card px-1.5 font-tw text-[11px] font-bold text-ink2 outline-none transition-colors hover:border-ink focus:border-ink"
                          aria-label="响度目标"
                        >
                          <option value={-14}>-14 · 流媒体</option>
                          <option value={-16}>-16 · 播客</option>
                          <option value={-23}>-23 · EBU 广播</option>
                          <option value={-9}>-9 · 母带参考</option>
                        </select>
                      </label>
                      <div
                        ref={observeBox}
                        className="relative h-[40px] min-w-0 flex-1"
                      >
                        <canvas
                          ref={loudBarCanvas}
                          className="absolute inset-0 h-full w-full"
                          role="img"
                          aria-label="整体响度相对目标的偏差标尺"
                        />
                      </div>
                    </div>
                  ) : null}
                </Panel>
              ) : null}

              {panels.levels ? (
                <Panel
                  no="NO.02"
                  title="LEVELS · 电平表"
                  metaText={
                    levelsMode === "vu" ? "VU · 0VU = -18 dBFS" : "dBFS · PEAK+RMS"
                  }
                  className={cn("flex-1", wide ? "min-h-0" : "min-h-[280px]")}
                >
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-1 pb-2 font-tw text-[10px] font-bold tracking-[1px] text-ink3">
                    <span>
                      PEAK{" "}
                      <b ref={setReadout("peakL")} className="text-[13px] text-ink">--.-</b>{" "}
                      / <b ref={setReadout("peakR")} className="text-[13px] text-ink">--.-</b>
                    </span>
                    <span>
                      RMS{" "}
                      <b ref={setReadout("rmsL")} className="text-[13px] text-ink">--.-</b>{" "}
                      / <b ref={setReadout("rmsR")} className="text-[13px] text-ink">--.-</b>
                    </span>
                    <span
                      ref={setReadout("clipFlag")}
                      className="border-[1.5px] border-line px-1.5 py-0.5 text-[9px] tracking-[2px] text-ink3 transition-colors"
                    >
                      CLIP
                    </span>
                  </div>
                  <div ref={observeBox} className="relative min-h-0 flex-1">
                    <canvas
                      ref={levelsCanvas}
                      className="absolute inset-0 h-full w-full"
                      role="img"
                      aria-label={
                        levelsMode === "vu"
                          ? "左右声道模拟 VU 表盘，0 VU 对应 -18 dBFS"
                          : "左右声道峰值与均方根电平行，红区为 -6 dBFS 以上"
                      }
                    />
                  </div>
                  <div className="flex items-center justify-between gap-2 pt-2">
                    <div className="flex gap-1.5">
                      <ModeTab
                        active={levelsMode === "bar"}
                        label="BAR 条表"
                        onClick={() => settings.setLevelsMode("bar")}
                      />
                      <ModeTab
                        active={levelsMode === "vu"}
                        label="VU 表盘"
                        onClick={() => settings.setLevelsMode("vu")}
                      />
                    </div>
                    <span className="truncate font-tw text-[9px] tracking-[1px] text-ink3">
                      {levelsMode === "vu"
                        ? "300MS 表针弹道 · 超 0VU 亮灯"
                        : "PEAK / RMS 分行 · 保持 1.4S"}
                    </span>
                  </div>
                </Panel>
              ) : null}
            </div>
          ) : null}

          {/* 声场（近方形，极坐标不变形） */}
          {panels.field ? (
            <Panel
              no="NO.03"
              title="SOUND FIELD · 声场"
              metaRef={fieldMeta}
              style={layout?.place.field}
              className={cn(!wide && narrowPanel)}
            >
              <div ref={observeBox} className="relative min-h-0 flex-1">
                <canvas
                  ref={fieldCanvas}
                  className="absolute inset-0 h-full w-full"
                  role="img"
                  aria-label="立体声声场散点与相关度表"
                />
              </div>
              <div className="flex items-center justify-between gap-2 pt-2">
                <div className="flex gap-1.5">
                  <ModeTab
                    active={fieldMode === "polar"}
                    label="POLAR 极坐标"
                    onClick={() => settings.setFieldMode("polar")}
                  />
                  <ModeTab
                    active={fieldMode === "lissajous"}
                    label="LISSAJOUS 李萨如"
                    onClick={() => settings.setFieldMode("lissajous")}
                  />
                </div>
                <span className="truncate font-tw text-[9px] tracking-[1px] text-ink3">
                  ρ&gt;0 同相 · ρ&lt;0 反相
                </span>
              </div>
            </Panel>
          ) : null}

          {/* 频谱：右区首位大幅面 */}
          {panels.spectrum ? (
            <Panel
              no="NO.04"
              title="SPECTRUM · 频谱"
              metaRef={spectrumMeta}
              style={layout?.place.spectrum}
              className={cn(!wide && "min-h-[300px] shrink-0")}
            >
              <div ref={observeBox} className="relative min-h-0 flex-1">
                <canvas
                  ref={spectrumCanvas}
                  className="absolute inset-0 h-full w-full cursor-crosshair"
                  role="img"
                  aria-label="实时频谱曲线，对数频轴 20Hz 至 20kHz，含峰值保持虚线"
                  onMouseMove={(event) => {
                    const geometry = spectrumGeomRef.current;
                    if (!geometry) return;
                    const rect = event.currentTarget.getBoundingClientRect();
                    cursorBinRef.current = spectrumBinFromX(
                      event.clientX - rect.left,
                      geometry,
                      ANALYSIS_BIN_COUNT
                    );
                  }}
                  onMouseLeave={() => {
                    cursorBinRef.current = null;
                  }}
                />
              </div>
            </Panel>
          ) : null}

          {/* 示波器：时间域波形（宽屏为扁长条） */}
          {panels.scope ? (
            <Panel
              no="NO.06"
              title="OSCILLOSCOPE · 示波器"
              metaText={scopeTrigger ? "≈21MS 窗 · 锁相" : "≈43MS 窗 · 走带"}
              style={layout?.place.scope}
              className={cn(!wide && "min-h-[220px] shrink-0")}
            >
              <div ref={observeBox} className="relative min-h-0 flex-1">
                <canvas
                  ref={scopeCanvas}
                  className="absolute inset-0 h-full w-full"
                  role="img"
                  aria-label="时间域波形示波器：左声道墨色、右声道印章红"
                />
              </div>
            </Panel>
          ) : null}

          {/* 频谱瀑布 */}
          {panels.spectrogram ? (
            <Panel
              no="NO.05"
              title="SPECTROGRAM · 频谱瀑布"
              style={layout?.place.spectrogram}
              className={cn(!wide && narrowPanel)}
            >
              <div ref={observeBox} className="relative min-h-0 flex-1">
                <canvas
                  ref={cascadeCanvas}
                  className="absolute inset-0 h-full w-full"
                  role="img"
                  aria-label="频谱瀑布：山脊模式为逐帧频谱线堆叠，热图模式为时间-频率能量色图"
                />
              </div>
              <div className="flex items-center justify-between gap-2 pt-2">
                <div className="flex gap-1.5">
                  <ModeTab
                    active={spectrogramMode === "ridge"}
                    label="RIDGE 山脊"
                    onClick={() => settings.setSpectrogramMode("ridge")}
                  />
                  <ModeTab
                    active={spectrogramMode === "heat"}
                    label="HEAT 热图"
                    onClick={() => settings.setSpectrogramMode("heat")}
                  />
                </div>
                <span className="truncate font-tw text-[9px] tracking-[1px] text-ink3">
                  {spectrogramMode === "ridge"
                    ? "窗口 ≈5.8S · 64 帧 × 96 频点"
                    : "色带：纸 → 浅褐 → 棕 → 印章红 → 墨"}
                </span>
              </div>
            </Panel>
          ) : null}
        </div>
      )}
    </div>
  );
}
