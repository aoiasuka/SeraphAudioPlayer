import { useRef, useState } from "react";
import { formatSeconds } from "@/lib/format";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import { useWaveform, WAVEFORM_SIDE_PADDING } from "@/hooks/useWaveform";

export function WaveformProgress() {
  const track = usePlayerStore((s) => s.currentTrack());
  const currentTime = usePlayerStore((s) => s.currentTime);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const seek = usePlayerStore((s) => s.seek);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [dragTime, setDragTime] = useState<number | null>(null);
  const draggingRef = useRef(false);

  const displayTime = dragTime ?? currentTime;

  useWaveform(canvasRef, { track, currentTime: displayTime, isPlaying });

  const timeFromClientX = (clientX: number) => {
    if (!track || track.duration <= 0) return;
    const el = containerRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const clickX = clientX - rect.left - WAVEFORM_SIDE_PADDING;
    const realWidth = rect.width - WAVEFORM_SIDE_PADDING * 2;
    const pct = Math.max(0, Math.min(1, clickX / realWidth));
    return pct * track.duration;
  };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!canSeek) return;
    const nextTime = timeFromClientX(e.clientX);
    if (nextTime === undefined) return;
    draggingRef.current = true;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragTime(nextTime);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    const nextTime = timeFromClientX(e.clientX);
    if (nextTime !== undefined) setDragTime(nextTime);
  };

  const finishDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!draggingRef.current) return;
    draggingRef.current = false;
    const nextTime = timeFromClientX(e.clientX) ?? dragTime;
    setDragTime(null);
    if (nextTime !== null && nextTime !== undefined) seek(nextTime);
  };

  if (!track) return null;

  const canSeek = track.duration > 0;
  const currentLabel = formatSeconds(displayTime);
  const durationLabel = canSeek ? formatSeconds(track.duration) : "--:--";

  return (
    <div className="w-full pt-3">
      <div
        className="group grid h-12 w-full grid-cols-[52px_minmax(0,1fr)_52px] items-center gap-3 rounded-lg border border-cyan-700/10 bg-white/55 px-3 shadow-[inset_0_1px_0_rgba(255,255,255,0.75),0_8px_22px_rgba(15,23,42,0.04)] transition-all hover:border-cyan-700/18 hover:bg-white/70"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={track.duration || 0}
        aria-valuenow={Math.min(displayTime, track.duration || 0)}
      >
        <span className="rounded-md bg-white/70 px-2 py-1 text-center font-mono text-[10px] font-semibold tabular-nums text-slate-600 shadow-[0_1px_6px_rgba(15,23,42,0.04)]">
          {currentLabel}
        </span>
        <div
          ref={containerRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={finishDrag}
          onPointerCancel={finishDrag}
          className={cn(
            "relative flex h-8 min-w-0 touch-none items-center overflow-hidden rounded-md bg-gradient-to-r from-cyan-950/[0.035] via-white/35 to-cyan-950/[0.035]",
            canSeek ? "cursor-pointer" : "cursor-default"
          )}
        >
          <canvas
            ref={canvasRef}
            className="h-8 w-full opacity-100 transition-opacity group-hover:opacity-100"
          />
        </div>
        <span className="rounded-md bg-white/70 px-2 py-1 text-center font-mono text-[10px] font-semibold tabular-nums text-slate-600 shadow-[0_1px_6px_rgba(15,23,42,0.04)]">
          {durationLabel}
        </span>
      </div>
    </div>
  );
}
