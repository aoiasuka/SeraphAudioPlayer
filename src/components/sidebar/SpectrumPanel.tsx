import { useEffect, useRef } from "react";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

interface SpectrumFrame {
  bins: number[];
  peakLeft: number;
  peakRight: number;
}

const POLL_INTERVAL_MS = 33; // ~30fps
const BIN_COUNT = 48;

/**
 * 实时频谱面板：播放中轮询后端 FFT 帧绘制柱状频谱，
 * 暂停/停止后本地衰减到零（不再打 IPC）。纯浏览器开发模式不渲染。
 */
export function SpectrumPanel() {
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const binsRef = useRef<number[]>(new Array(BIN_COUNT).fill(0));
  const rafRef = useRef<number>(0);

  useEffect(() => {
    if (!isTauriRuntime()) return;

    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;

    const inkColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue("--ink")
        .trim() || "#2b2722";
    const stampColor =
      getComputedStyle(document.documentElement)
        .getPropertyValue("--stamp")
        .trim() || "#b4553c";

    const draw = () => {
      const ratio = window.devicePixelRatio || 1;
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      if (width === 0 || height === 0) return;
      if (canvas.width !== width * ratio || canvas.height !== height * ratio) {
        canvas.width = width * ratio;
        canvas.height = height * ratio;
      }
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      context.clearRect(0, 0, width, height);

      const bins = binsRef.current;
      const gap = 2;
      const barWidth = (width - gap * (bins.length - 1)) / bins.length;
      for (let i = 0; i < bins.length; i += 1) {
        const level = Math.min(1, Math.max(0, bins[i]));
        const barHeight = Math.max(1, level * (height - 2));
        const x = i * (barWidth + gap);
        // 高电平柱透出印章红，其余墨色，贴合档案视觉
        context.fillStyle = level > 0.82 ? stampColor : inkColor;
        context.globalAlpha = 0.28 + level * 0.72;
        context.fillRect(x, height - barHeight, barWidth, barHeight);
      }
      context.globalAlpha = 1;
    };

    let pollTimer: number | null = null;
    let disposed = false;

    // 中-12：仅在“正在播放，或仍有非零柱需要衰减动画”时续帧。
    // 暂停且频谱归零后停掉 RAF，避免以刷新率持续重绘一张全零图空转 CPU/GPU。
    // 播放恢复时 effect 依赖 isPlaying 变化会重跑，重新启动渲染循环。
    const needsRender = () =>
      isPlaying || binsRef.current.some((value) => value > 0.001);

    const renderLoop = () => {
      if (disposed) return;
      draw();
      if (needsRender()) {
        rafRef.current = window.requestAnimationFrame(renderLoop);
      } else {
        rafRef.current = 0;
      }
    };
    rafRef.current = window.requestAnimationFrame(renderLoop);

    if (isPlaying) {
      pollTimer = window.setInterval(() => {
        void invoke<SpectrumFrame | null>("get_spectrum_frame")
          .then((frame) => {
            if (disposed || !frame) return;
            binsRef.current = frame.bins;
          })
          .catch(() => {
            // 轮询失败静默；下一拍重试
          });
      }, POLL_INTERVAL_MS);
    } else {
      // 暂停后本地指数衰减到零，视觉自然收敛
      pollTimer = window.setInterval(() => {
        const decayed = binsRef.current.map((value) => value * 0.82);
        binsRef.current = decayed;
        if (decayed.every((value) => value < 0.004)) {
          binsRef.current = new Array(BIN_COUNT).fill(0);
          if (pollTimer !== null) {
            window.clearInterval(pollTimer);
            pollTimer = null;
          }
        }
      }, POLL_INTERVAL_MS);
    }

    return () => {
      disposed = true;
      if (pollTimer !== null) window.clearInterval(pollTimer);
      window.cancelAnimationFrame(rafRef.current);
    };
  }, [isPlaying]);

  if (!isTauriRuntime()) return null;

  return (
    <div className="space-y-2">
      <h3 className="font-tw text-[10px] font-bold text-ink3 tracking-[3px] uppercase">
        SPECTRUM — 频谱
      </h3>
      <div className="border-[1.5px] border-ink bg-card p-2">
        <canvas ref={canvasRef} className="h-[56px] w-full" aria-hidden />
      </div>
    </div>
  );
}
