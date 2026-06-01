import { Disc3 } from "lucide-react";
import { usePlayerStore } from "@/store/player";

export function TrackInfo() {
  const track = usePlayerStore((s) => s.currentTrack());
  if (!track) return null;

  return (
    <div className="space-y-1.5">
      <h2 className="text-2xl font-bold tracking-tight text-slate-800 line-clamp-1">
        {track.title}
      </h2>
      <p className="text-xs font-semibold text-slate-500">
        {track.artist}
      </p>
      <p className="text-xs text-slate-400 flex items-center justify-center gap-1.5">
        <Disc3 className="w-3 h-3" />
        {track.album}
      </p>
    </div>
  );
}
