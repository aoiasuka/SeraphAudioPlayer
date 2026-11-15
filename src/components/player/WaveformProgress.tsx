import { useRef, useState } from "react";
import { formatSeconds } from "@/lib/format";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import { useWaveform, WAVEFORM_SIDE_PADDING } from "@/hooks/useWaveform";
import { useImmersiveStore } from "@/store/immersive";

const KEY_STEP_SECONDS = 5;
const KEY_PAGE_SECONDS = 30;

export function WaveformProgress() {
  const immersiveOpen = useImmersiveStore((s) => s.isOpen);
  const track = usePlayerStore((s) => s.currentTrack());
  const currentTime = usePlayerStore((s) => s.currentTime);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const seek = usePlayerStore((s) => s.seek);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [dragTime, setDragTime] = useState<number | null>(null);
  const draggingRef = useRef(false);

  const displayTime = dragTime ?? currentTime;

  useWaveform(canvasRef, { track, currentTime: displayTime, isPlaying, enabled: !immersiveOpen });

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
  // FE-03：左右方向键步进 5 秒，PageUp / PageDown 步进 30 秒，Home / End 跳首尾。
  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (!canSeek) return;
    const steps: Record<string, number> = {
      ArrowLeft: -KEY_STEP_SECONDS,
      ArrowDown: -KEY_STEP_SECONDS,
      ArrowRight: KEY_STEP_SECONDS,
      ArrowUp: KEY_STEP_SECONDS,
      PageDown: -KEY_PAGE_SECONDS,
      PageUp: KEY_PAGE_SECONDS,
    };
    let target: number;
    if (e.key === "Home") target = 0;
    else if (e.key === "End") target = track.duration;
    else if (e.key in steps) target = currentTime + steps[e.key];
    else return;
    e.preventDefault();
    seek(Math.max(0, Math.min(track.duration, target)));
  };
  const currentLabel = formatSeconds(displayTime);
  const durationLabel = canSeek ? formatSeconds(track.duration) : "--:--";

  return (
    <div className="w-full pt-3">
      <div
        className="group grid h-11 w-full grid-cols-[52px_minmax(0,1fr)_52px] items-center gap-3 border-[1.5px] border-ink bg-paper2 px-3 transition-all"
        role="slider"
        aria-label="播放进度"
        aria-orientation="horizontal"
        tabIndex={canSeek ? 0 : -1}
        aria-disabled={!canSeek}
        aria-valuemin={0}
        aria-valuemax={track.duration || 0}
        aria-valuenow={Math.min(displayTime, track.duration || 0)}
        aria-valuetext={`${currentLabel} / ${durationLabel}`}
        onKeyDown={onKeyDown}
      >
        <span className="px-1 text-center font-tw text-[11px] font-bold tabular-nums text-ink2">
          {currentLabel}
        </span>
        <div
          ref={containerRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={finishDrag}
          onPointerCancel={finishDrag}
          className={cn(
            "relative flex h-8 min-w-0 touch-none items-center overflow-hidden bg-card",
            canSeek ? "cursor-pointer" : "cursor-default"
          )}
        >
          <canvas
            ref={canvasRef}
            className="h-8 w-full opacity-100 transition-opacity group-hover:opacity-100"
          />
        </div>
        <span className="px-1 text-center font-tw text-[11px] font-bold tabular-nums text-ink2">
          {durationLabel}
        </span>
      </div>
    </div>
  );
}
