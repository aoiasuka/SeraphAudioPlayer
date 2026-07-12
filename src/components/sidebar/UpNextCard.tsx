import { MoreHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import { coverSrc } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

export function UpNextCard() {
  const nextTrack = usePlayerStore((s) => s.nextTrack);
  const next = usePlayerStore((s) => s.nextTrackPreview());
  const [coverFailed, setCoverFailed] = useState(false);
  const cover = coverSrc(next?.cover);

  useEffect(() => {
    setCoverFailed(false);
  }, [cover]);

  if (!next) return null;

  return (
    <div className="space-y-2">
      <h3 className="font-tw text-[10px] font-bold text-ink3 tracking-[3px] uppercase">
        UP NEXT — 下一首
      </h3>
      <div
        onClick={nextTrack}
        className="archive-card flex items-center justify-between gap-2.5 p-2.5 cursor-pointer"
      >
        <div className="flex min-w-0 items-center gap-2.5">
          {cover && !coverFailed ? (
            <img
              src={cover}
              alt=""
              draggable={false}
              onError={() => setCoverFailed(true)}
              className="h-9 w-9 shrink-0 border border-ink/20 object-cover"
            />
          ) : null}
          <div className="min-w-0">
            <h4 className="font-serif text-xs font-semibold text-ink line-clamp-1">
              {next.title}
            </h4>
            <p className="font-tw text-[10px] text-ink2 mt-0.5">{next.artist}</p>
          </div>
        </div>
        <button
          className="w-6 h-6 flex items-center justify-center text-ink3 hover:text-ink transition-colors"
          onClick={(e) => e.stopPropagation()}
          aria-label="更多"
        >
          <MoreHorizontal className="w-3 h-3" />
        </button>
      </div>
    </div>
  );
}
