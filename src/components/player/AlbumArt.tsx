import { usePlayerStore } from "@/store/player";

export function AlbumArt() {
  const track = usePlayerStore((s) => s.currentTrack());
  if (!track?.cover) return null;

  return (
    <div className="relative w-[clamp(170px,34vh,360px)] h-[clamp(170px,34vh,360px)] flex-shrink-0">
      <div className="absolute inset-0 bg-cyan-500/5 blur-3xl rounded-2xl animate-pulse" />
      <img
        src={track.cover}
        alt="Album Art"
        className="w-full h-full object-cover rounded-2xl border border-white/80 album-breath shadow-[0_15px_40px_rgba(0,0,0,0.08)] z-10 relative"
      />
    </div>
  );
}
