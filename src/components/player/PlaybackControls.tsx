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
    <div className="flex items-center gap-3">
      <button
        onClick={toggleShuffle}
        title="随机播放"
        aria-label="随机播放"
        className={cn(
          "stamp-btn w-[38px] h-8 flex items-center justify-center",
          shuffleMode ? "bg-ink text-paper" : "text-ink"
        )}
      >
        <Shuffle className="w-3.5 h-3.5" />
      </button>

      <button
        onClick={prevTrack}
        disabled={!hasTrack}
        title="上一首"
        aria-label="上一首"
        className="stamp-btn w-[38px] h-8 flex items-center justify-center text-ink disabled:cursor-not-allowed disabled:opacity-40"
      >
        <SkipBack className="w-4 h-4" />
      </button>

      <button
        onClick={togglePlayback}
        disabled={!hasTrack}
        title={isPlaying ? "暂停" : "播放"}
        aria-label={isPlaying ? "暂停" : "播放"}
        className={cn(
          "stamp-btn w-[46px] h-8 flex items-center justify-center bg-ink text-paper",
          !hasTrack && "cursor-not-allowed opacity-40"
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
        className="stamp-btn w-[38px] h-8 flex items-center justify-center text-ink disabled:cursor-not-allowed disabled:opacity-40"
      >
        <SkipForward className="w-4 h-4" />
      </button>

      <button
        onClick={toggleLoop}
        title="单曲循环"
        aria-label="单曲循环"
        className={cn(
          "stamp-btn w-[38px] h-8 flex items-center justify-center",
          loopMode ? "bg-ink text-paper" : "text-ink"
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
          "stamp-btn w-[38px] h-8 flex items-center justify-center",
          isLiked ? "text-stamp" : "text-ink hover:text-stamp",
          !hasTrack && "cursor-not-allowed opacity-40"
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
