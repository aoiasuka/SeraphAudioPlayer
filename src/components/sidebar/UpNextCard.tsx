import { MoreHorizontal } from "lucide-react";
import { usePlayerStore } from "@/store/player";

export function UpNextCard() {
  const nextTrack = usePlayerStore((s) => s.nextTrack);
  const next = usePlayerStore((s) => s.nextTrackPreview());

  if (!next) return null;

  return (
    <div className="space-y-2">
      <h3 className="text-[10px] font-bold text-slate-400 tracking-wider uppercase">
        下一首播放
      </h3>
      <div
        onClick={nextTrack}
        className="flex items-center justify-between p-2 bg-white/70 hover:bg-white border border-black/[0.04] rounded-lg cursor-pointer transition-all shadow-[0_2px_8px_rgba(0,0,0,0.02)]"
      >
        <div className="min-w-0">
          <div>
            <h4 className="text-xs font-semibold text-slate-800 line-clamp-1">
              {next.title}
            </h4>
            <p className="text-[10px] text-slate-500">{next.artist}</p>
          </div>
        </div>
        <button
          className="w-6 h-6 flex items-center justify-center text-slate-400 hover:text-slate-700 rounded transition-colors"
          onClick={(e) => e.stopPropagation()}
          aria-label="更多"
        >
          <MoreHorizontal className="w-3 h-3" />
        </button>
      </div>
    </div>
  );
}
