import { useRef, useState, type RefObject } from "react";
import { Heart, ListMusic, Pause, Play, Repeat1, Shuffle, SkipBack, SkipForward, Volume2, VolumeX } from "lucide-react";
import { useWaveform } from "@/hooks/useWaveform";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

export function ImmersiveProgress({ track }: { track: Track }) {
  const currentTime = usePlayerStore((s) => s.currentTime);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const seek = usePlayerStore((s) => s.seek);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragging = useRef(false);
  const [previewTime, setPreviewTime] = useState<number | null>(null);
  const duration = Number.isFinite(track.duration) && track.duration > 0 ? track.duration : 0;
  const elapsed = previewTime ?? currentTime;
  const time = Number.isFinite(elapsed) ? Math.max(0, duration ? Math.min(elapsed, duration) : elapsed) : 0;
  useWaveform(canvasRef, { track, currentTime: time, isPlaying });

  const cancelDrag = () => { dragging.current = false; setPreviewTime(null); };

  return (
    <div className="immersive-progress">
      <time className="immersive-elapsed">{formatSeconds(time)}</time>
      <div className="immersive-progress-track">
        {/* 延续主播放器的装饰波形，真实进度与 seek 均来自同一个播放 store。 */}
        <canvas ref={canvasRef} aria-hidden="true" />
        <i className="immersive-playhead" style={{ left: `${duration ? time / duration * 100 : 0}%` }} aria-hidden="true" />
        <input
          type="range"
          min={0}
          max={duration || 1}
          step={0.1}
          value={duration ? time : 0}
          disabled={!duration}
          aria-label="播放进度"
          aria-valuetext={`${formatSeconds(time)} / ${duration ? formatSeconds(duration) : "时长未知"}`}
          onPointerDown={(event) => {
            if (!duration) return;
            dragging.current = true;
            setPreviewTime(time);
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onChange={(event) => {
            const value = Number(event.currentTarget.value);
            if (dragging.current) setPreviewTime(value);
            else seek(value);
          }}
          onPointerUp={(event) => {
            if (!dragging.current) return;
            const value = Number(event.currentTarget.value);
            cancelDrag();
            seek(value);
          }}
          onPointerCancel={cancelDrag}
          onLostPointerCapture={cancelDrag}
          onBlur={cancelDrag}
        />
      </div>
      <time>{duration ? formatSeconds(duration) : "--:--"}</time>
    </div>
  );
}

function TransportButtons() {
  const playing = usePlayerStore((s) => s.isPlaying);
  const shuffle = usePlayerStore((s) => s.shuffleMode);
  const loop = usePlayerStore((s) => s.loopMode);
  const togglePlayback = usePlayerStore((s) => s.togglePlayback);
  const prevTrack = usePlayerStore((s) => s.prevTrack);
  const nextTrack = usePlayerStore((s) => s.nextTrack);
  const toggleShuffle = usePlayerStore((s) => s.toggleShuffle);
  const toggleLoop = usePlayerStore((s) => s.toggleLoop);
  return (
    <div className="immersive-transport-buttons">
      <button className="immersive-icon" aria-label="随机播放" aria-pressed={shuffle} title="随机播放" onClick={toggleShuffle}><Shuffle size={16} strokeWidth={1.4} /></button>
      <button className="immersive-icon immersive-skip" aria-label="上一首" title="上一首" onClick={prevTrack}><SkipBack size={16} strokeWidth={1.3} /></button>
      <button className="immersive-icon immersive-play" aria-label={playing ? "暂停" : "播放"} title={playing ? "暂停" : "播放"} onClick={togglePlayback}>{playing ? <Pause size={20} strokeWidth={1.4} /> : <Play size={20} strokeWidth={1.4} />}</button>
      <button className="immersive-icon immersive-skip" aria-label="下一首" title="下一首" onClick={nextTrack}><SkipForward size={16} strokeWidth={1.3} /></button>
      <button className="immersive-icon" aria-label="单曲循环" aria-pressed={loop} title="单曲循环" onClick={toggleLoop}><Repeat1 size={16} strokeWidth={1.4} /></button>
    </div>
  );
}

function TrackActions({ track, queueOpen, queueTriggerRef, onOpenQueue }: TransportProps) {
  const liked = usePlayerStore((s) => Boolean(s.liked[track.id]));
  const toggleLike = usePlayerStore((s) => s.toggleLike);
  const volume = usePlayerStore((s) => s.volume);
  const setVolume = usePlayerStore((s) => s.setVolume);
  const toggleMute = usePlayerStore((s) => s.toggleMute);
  return (
    <div className="immersive-track-actions">
      <button className="immersive-icon" aria-label="收藏当前曲目" aria-pressed={liked} title={liked ? "取消收藏" : "收藏"} onClick={() => toggleLike(track.id)}><Heart size={17} strokeWidth={1.4} fill={liked ? "currentColor" : "none"} /></button>
      <button className="immersive-icon" aria-label={volume === 0 ? "取消静音" : "静音"} title={volume === 0 ? "取消静音" : "静音"} onClick={toggleMute}>{volume === 0 ? <VolumeX size={16} strokeWidth={1.4} /> : <Volume2 size={16} strokeWidth={1.4} />}</button>
      <input className="immersive-volume" type="range" min={0} max={1} step={0.01} value={volume} aria-label="音量" aria-valuetext={`${Math.round(volume * 100)}%`} style={{ background: `linear-gradient(to right, var(--ink) ${volume * 100}%, var(--line) ${volume * 100}%)` }} onChange={(event) => setVolume(Number(event.currentTarget.value))} />
      <button ref={queueTriggerRef} className="immersive-icon" aria-label="打开播放队列" aria-expanded={queueOpen} aria-haspopup="dialog" title="播放队列" onClick={onOpenQueue}><ListMusic size={18} strokeWidth={1.3} /></button>
    </div>
  );
}

interface TransportProps {
  track: Track;
  queueOpen: boolean;
  queueTriggerRef: RefObject<HTMLButtonElement>;
  onOpenQueue: () => void;
}

export function ImmersiveTransport(props: TransportProps) {
  const { track } = props;
  const prefix = `${track.format} `;
  const quality = track.bitdepth.startsWith(prefix) ? track.bitdepth.slice(prefix.length) : track.bitdepth;
  return (
    <footer className="immersive-transport">
      <ImmersiveProgress key={track.id} track={track} />
      <div className="immersive-controls">
        <div className="immersive-quality" title={[track.format, quality, track.sampleRate].filter(Boolean).join(" · ")}><span>{track.format || "AUDIO"}</span><small>{quality || track.sampleRate || "—"}</small></div>
        <TransportButtons />
        <TrackActions {...props} />
      </div>
    </footer>
  );
}
