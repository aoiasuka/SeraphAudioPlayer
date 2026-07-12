import { coverSrc } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

export function AlbumArt() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  const cover = coverSrc(track?.cover);
  if (!cover) return null;

  const glow1 = track?.glow1 ?? "#06b6d4";
  const glow2 = track?.glow2 ?? "#8b5cf6";

  return (
    <div
      className="relative w-[clamp(170px,34vh,360px)] h-[clamp(170px,34vh,360px)] flex-shrink-0 transition-transform duration-700 ease-out hover:scale-[1.04]"
      style={{
        "--album-glow-1": glow1,
        "--album-glow-2": glow2,
        "--album-breath-duration": isPlaying ? "8s" : "24s",
      } as React.CSSProperties}
    >
      <div
        className="absolute inset-0 blur-3xl rounded-2xl animate-pulse transition-all duration-1000"
        style={{
          backgroundColor: glow1,
          opacity: isPlaying ? 0.22 : 0.08,
        }}
      />
      <img
        src={cover}
        alt="Album Art"
        className="w-full h-full object-cover rounded-2xl border border-white/80 album-breath shadow-[0_15px_40px_rgba(0,0,0,0.08)] z-10 relative"
      />
    </div>
  );
}
