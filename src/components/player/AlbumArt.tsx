import { coverSrc } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

/** 曲目氛围色只可能是后端 color_pair 色板的 hex 值；非法值回落默认，
 *  不让曲库缓存中的异常字符串进入 CSSOM（2026-08-16 审查纵深）。 */
const HEX_COLOR = /^#[0-9a-f]{3,8}$/i;

function safeGlow(value: string | undefined, fallback: string): string {
  return value && HEX_COLOR.test(value) ? value : fallback;
}

export function AlbumArt() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  const cover = coverSrc(track?.cover);
  if (!cover) return null;

  const glow1 = safeGlow(track?.glow1, "#06b6d4");
  const glow2 = safeGlow(track?.glow2, "#8b5cf6");

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
