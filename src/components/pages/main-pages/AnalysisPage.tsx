import { useEffect, useRef, useState } from "react";
import {
  binFreq,
  drawLevelsMeter,
  drawLoudnessDeviationBar,
  drawSoundField,
  drawSpectrogram,
  drawSpectrumChart,
  formatFreq,
  prepCanvas,
  resolveArchiveColors,
  spectrumBinFromX,
  type ArchiveColors,
  type SoundFieldMode,
  type SpectrogramMode,
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
import { usePlayerStore } from "@/store/player";

const POLL_INTERVAL_MS = 33; // ~30fps

const fmt1 = (value: number | null) => (value === null ? "--.-" : value.toFixed(1));

/** 面板套壳：档案编号 + 标题 + 右侧实时读数位 */
function Panel({
  no,
  title,
  metaRef,
  metaText,
  className,
  children,
}: {
  no: string;
  title: string;
  metaRef?: React.MutableRefObject<HTMLSpanElement | null>;
  metaText?: string;
  className?: string;
  children: React.ReactNode;
}) {
  return (
    <section
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

/**
 * 声学分析页：响度（EBU R128）/ 电平 / 声场 / 频谱 / 瀑布 五仪表。
 * 桌面运行时由播放输出的真实样本驱动（get_analysis_frame ~30fps 轮询）；
 * 纯浏览器开发模式回退到内置模拟信号源。
 */
export function AnalysisPage() {
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const currentTrackId = usePlayerStore((s) => s.currentTrack()?.id ?? null);

  const [fieldMode, setFieldMode] = useState<SoundFieldMode>("polar");
  const [cascadeMode, setCascadeMode] = useState<SpectrogramMode>("ridge");
  const [target, setTarget] = useState(-14);

  const viewRef = useRef(createAnalysisView());
  const colorsRef = useRef<ArchiveColors | null>(null);
  const trailRef = useRef<Float32Array[]>([]);
  const cursorBinRef = useRef<number | null>(null);
  const spectrumGeomRef = useRef<SpectrumGeometry | null>(null);
  const fieldModeRef = useRef(fieldMode);
  const cascadeModeRef = useRef(cascadeMode);
  const heatCanvasRef = useRef<HTMLCanvasElement | null>(null);

  const loudBarCanvas = useRef<HTMLCanvasElement | null>(null);
  const levelsCanvas = useRef<HTMLCanvasElement | null>(null);
  const fieldCanvas = useRef<HTMLCanvasElement | null>(null);
  const spectrumCanvas = useRef<HTMLCanvasElement | null>(null);
  const cascadeCanvas = useRef<HTMLCanvasElement | null>(null);

  const loudMeta = useRef<HTMLSpanElement | null>(null);
  const fieldMeta = useRef<HTMLSpanElement | null>(null);
  const spectrumMeta = useRef<HTMLSpanElement | null>(null);
  const readouts = useRef<Record<string, HTMLElement | null>>({});
  const setReadout = (key: string) => (el: HTMLElement | null) => {
    readouts.current[key] = el;
  };

  fieldModeRef.current = fieldMode;
  cascadeModeRef.current = cascadeMode;
  viewRef.current.loud.target = target;

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

  // 渲染循环：步进弹道学 + 画五块画布 + 刷新数字读数
  useEffect(() => {
    let disposed = false;
    let raf = 0;

    const renderLoop = () => {
      if (disposed) return;
      const view = viewRef.current;
      const now = performance.now() / 1000;
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
        const prepared = prepCanvas(canvas);
        if (!prepared) return;
        painter(prepared.ctx, prepared.w, prepared.h);
      };

      draw(loudBarCanvas, (ctx, w, h) =>
        drawLoudnessDeviationBar(ctx, w, h, view, colors)
      );
      draw(levelsCanvas, (ctx, w, h) => drawLevelsMeter(ctx, w, h, view, colors));
      draw(fieldCanvas, (ctx, w, h) =>
        drawSoundField(ctx, w, h, view, colors, fieldModeRef.current, trail)
      );
      draw(spectrumCanvas, (ctx, w, h) => {
        spectrumGeomRef.current = drawSpectrumChart(
          ctx,
          w,
          h,
          view,
          colors,
          cursorBinRef.current
        );
      });
      draw(cascadeCanvas, (ctx, w, h) => {
        heatCanvasRef.current ??= document.createElement("canvas");
        drawSpectrogram(
          ctx,
          w,
          h,
          view,
          colors,
          cascadeModeRef.current,
          heatCanvasRef.current
        );
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

      raf = window.requestAnimationFrame(renderLoop);
    };
    raf = window.requestAnimationFrame(renderLoop);

    return () => {
      disposed = true;
      window.cancelAnimationFrame(raf);
    };
  }, []);

  const standby = isTauriRuntime() && !isPlaying;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pr-1 2xl:grid 2xl:grid-cols-[minmax(300px,29%)_minmax(0,1fr)_minmax(230px,18%)] 2xl:grid-rows-[minmax(0,1fr)_minmax(0,1.05fr)] 2xl:overflow-hidden 2xl:pr-0">
      {/* 左列：响度（自适应高度）+ 电平表（吃掉剩余高度） */}
      <div className="flex shrink-0 flex-col gap-3 2xl:row-span-2 2xl:min-h-0 2xl:shrink">
        <Panel no="NO.01" title="LOUDNESS · 响度" metaRef={loudMeta} className="shrink-0">
          {standby ? (
            <p className="mb-1.5 border-[1.5px] border-dashed border-brown px-2 py-1 font-tw text-[10px] font-bold text-brown">
              待机 HOLD —— 播放曲目后仪表实时运行
            </p>
          ) : null}
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
              TRUE PEAK MAX <b ref={setReadout("loudTp")} className="text-ink">--.-</b> dBTP≈
            </span>
            <span
              ref={setReadout("overFlag")}
              className="border-[1.5px] border-line px-1.5 py-0.5 font-tw text-[9px] font-bold tracking-[2px] text-ink3 transition-colors"
            >
              OVER
            </span>
          </div>
          <div className="flex items-center gap-2.5 border-t-[1.5px] border-line pt-2">
            <label className="flex shrink-0 items-center gap-2 font-tw text-[10px] font-bold tracking-[1px] text-ink3">
              TARGET 目标
              <select
                value={target}
                onChange={(event) => setTarget(Number(event.target.value))}
                className="h-7 cursor-pointer border-[1.5px] border-line bg-card px-1.5 font-tw text-[11px] font-bold text-ink2 outline-none transition-colors hover:border-ink focus:border-ink"
                aria-label="响度目标"
              >
                <option value={-14}>-14 · 流媒体</option>
                <option value={-16}>-16 · 播客</option>
                <option value={-23}>-23 · EBU 广播</option>
                <option value={-9}>-9 · 母带参考</option>
              </select>
            </label>
            <div className="relative h-[40px] min-w-0 flex-1">
              <canvas
                ref={loudBarCanvas}
                className="absolute inset-0 h-full w-full"
                role="img"
                aria-label="整体响度相对目标的偏差标尺"
              />
            </div>
          </div>
        </Panel>

        <Panel
          no="NO.02"
          title="LEVELS · 电平表"
          metaText="dBFS · PEAK+RMS"
          className="min-h-[280px] flex-1 2xl:min-h-0"
        >
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 pb-2 font-tw text-[10px] font-bold tracking-[1px] text-ink3">
            <span>
              PEAK <b ref={setReadout("peakL")} className="text-[13px] text-ink">--.-</b> /{" "}
              <b ref={setReadout("peakR")} className="text-[13px] text-ink">--.-</b>
            </span>
            <span>
              RMS <b ref={setReadout("rmsL")} className="text-[13px] text-ink">--.-</b> /{" "}
              <b ref={setReadout("rmsR")} className="text-[13px] text-ink">--.-</b>
            </span>
            <span
              ref={setReadout("clipFlag")}
              className="border-[1.5px] border-line px-1.5 py-0.5 text-[9px] tracking-[2px] text-ink3 transition-colors"
            >
              CLIP
            </span>
          </div>
          <div className="relative min-h-0 flex-1">
            <canvas
              ref={levelsCanvas}
              className="absolute inset-0 h-full w-full"
              role="img"
              aria-label="左右声道峰值与均方根电平条，红区为 -6 dBFS 以上"
            />
          </div>
        </Panel>
      </div>

      {/* 右下角：声场（近方形，极坐标不变形） */}
      <Panel
        no="NO.03"
        title="SOUND FIELD · 声场"
        metaRef={fieldMeta}
        className="min-h-[280px] shrink-0 2xl:col-start-3 2xl:row-start-2 2xl:min-h-0"
      >
          <div className="relative min-h-0 flex-1">
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
                onClick={() => setFieldMode("polar")}
              />
              <ModeTab
                active={fieldMode === "lissajous"}
                label="LISSAJOUS 李萨如"
                onClick={() => setFieldMode("lissajous")}
              />
            </div>
            <span className="truncate font-tw text-[9px] tracking-[1px] text-ink3">
              ρ&gt;0 同相 · ρ&lt;0 反相
            </span>
          </div>
      </Panel>

      {/* 右上：频谱独占最大幅面 */}
      <Panel
        no="NO.04"
        title="SPECTRUM · 频谱"
        metaRef={spectrumMeta}
        className="min-h-[300px] shrink-0 2xl:col-start-2 2xl:col-span-2 2xl:row-start-1 2xl:min-h-0"
      >
          <div className="relative min-h-0 flex-1">
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

        <Panel no="NO.05" title="SPECTROGRAM · 频谱瀑布" className="min-h-[280px] shrink-0 2xl:col-start-2 2xl:row-start-2 2xl:min-h-0">
          <div className="relative min-h-0 flex-1">
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
                active={cascadeMode === "ridge"}
                label="RIDGE 山脊"
                onClick={() => setCascadeMode("ridge")}
              />
              <ModeTab
                active={cascadeMode === "heat"}
                label="HEAT 热图"
                onClick={() => setCascadeMode("heat")}
              />
            </div>
            <span className="truncate font-tw text-[9px] tracking-[1px] text-ink3">
              {cascadeMode === "ridge"
                ? "窗口 ≈5.8S · 64 帧 × 96 频点"
                : "色带：纸 → 浅褐 → 棕 → 印章红 → 墨"}
            </span>
          </div>
        </Panel>
    </div>
  );
}
