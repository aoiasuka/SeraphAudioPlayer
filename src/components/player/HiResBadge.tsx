import { usePlayerStore } from "@/store/player";

export function HiResBadge() {
  const track = usePlayerStore((s) => s.currentTrack());
  if (!track) return null;

  return (
    <div className="hires-badge flex items-center gap-2 px-2.5 py-1 rounded text-[10px] text-seraph-gold-dark font-semibold tracking-wide">
      <span className="font-bold border border-seraph-gold/60 px-1 py-[0.2px] rounded text-[8px] bg-seraph-gold-light">
        Hi-Res
      </span>
      <span>{track.bitdepth}</span>
    </div>
  );
}
