import {
  Heart,
  Pause,
  Play,
  Repeat,
  Shuffle,
  SkipBack,
  SkipForward,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";

export function PlaybackControls() {
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const shuffleMode = usePlayerStore((s) => s.shuffleMode);
  const loopMode = usePlayerStore((s) => s.loopMode);
  const liked = usePlayerStore((s) => s.liked);
  const trackId = usePlayerStore((s) => s.currentTrack()?.id);
  const togglePlayback = usePlayerStore((s) => s.togglePlayback);
  const nextTrack = usePlayerStore((s) => s.nextTrack);
  const prevTrack = usePlayerStore((s) => s.prevTrack);
  const toggleShuffle = usePlayerStore((s) => s.toggleShuffle);
  const toggleLoop = usePlayerStore((s) => s.toggleLoop);
  const toggleLike = usePlayerStore((s) => s.toggleLike);

  const hasTrack = !!trackId;
  const isLiked = trackId ? !!liked[trackId] : false;

  return (
    <div className="flex items-center gap-5">
      <button
        onClick={toggleShuffle}
        title="随机播放"
        aria-label="随机播放"
        className={cn(
          "transition-all duration-200 hover:scale-115 active:scale-90",
          shuffleMode
            ? "text-cyan-600 hover:text-cyan-700 scale-110"
            : "text-slate-500 hover:text-slate-800"
        )}
      >
        <Shuffle className="w-3.5 h-3.5" />
      </button>

      <button
        onClick={prevTrack}
        disabled={!hasTrack}
        title="上一首"
        aria-label="上一首"
        className="text-slate-500 hover:text-slate-800 hover:scale-115 active:scale-90 transition-all duration-200 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:text-slate-500 disabled:hover:scale-100 disabled:active:scale-100"
      >
        <SkipBack className="w-4 h-4" />
      </button>

      <button
        onClick={togglePlayback}
        disabled={!hasTrack}
        title={isPlaying ? "暂停" : "播放"}
        aria-label={isPlaying ? "暂停" : "播放"}
        className={cn(
          "w-10 h-10 rounded-full bg-cyan-600 hover:bg-cyan-500 text-white flex items-center justify-center transform hover:scale-108 active:scale-95 transition-all duration-200",
          "shadow-[0_4px_12px_rgba(8,145,178,0.25)]",
          isPlaying && "shadow-[0_4px_16px_rgba(8,145,178,0.45)]",
          !hasTrack && "cursor-not-allowed opacity-35 hover:scale-100 hover:bg-cyan-600 active:scale-100"
        )}
      >
        {isPlaying ? (
          <Pause className="w-4 h-4" />
        ) : (
          <Play className="w-4 h-4 ml-0.5" />
        )}
      </button>

      <button
        onClick={nextTrack}
        disabled={!hasTrack}
        title="下一首"
        aria-label="下一首"
        className="text-slate-500 hover:text-slate-800 hover:scale-115 active:scale-90 transition-all duration-200 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:text-slate-500 disabled:hover:scale-100 disabled:active:scale-100"
      >
        <SkipForward className="w-4 h-4" />
      </button>

      <button
        onClick={toggleLoop}
        title="单曲循环"
        aria-label="单曲循环"
        className={cn(
          "transition-all duration-200 hover:scale-115 active:scale-90",
          loopMode
            ? "text-cyan-600 hover:text-cyan-700 scale-110"
            : "text-slate-500 hover:text-slate-800"
        )}
      >
        <Repeat className="w-3.5 h-3.5" />
      </button>

      <button
        onClick={() => {
          if (trackId) toggleLike(trackId);
        }}
        disabled={!hasTrack}
        title={isLiked ? "取消收藏" : "收藏当前曲目"}
        aria-label={isLiked ? "取消收藏" : "收藏当前曲目"}
        className={cn(
          "transition-all duration-200 hover:scale-120 active:scale-90",
          isLiked
            ? "text-rose-500 scale-110"
            : "text-slate-500 hover:text-rose-500",
          !hasTrack && "cursor-not-allowed opacity-35 hover:text-slate-500 hover:scale-100 active:scale-100"
        )}
      >
        <Heart
          className="w-3.5 h-3.5"
          fill={isLiked ? "currentColor" : "none"}
        />
      </button>
    </div>
  );
}
