import { UpNextCard } from "@/components/sidebar/UpNextCard";
import { LyricsPanel } from "@/components/sidebar/LyricsPanel";
import { AudioInfoCard } from "@/components/sidebar/AudioInfoCard";
import { usePlayerStore } from "@/store/player";

export function RightPanel() {
  const hasTrack = usePlayerStore((s) => s.currentTrack() !== null);

  if (!hasTrack) return null;

  return (
    <aside className="w-[clamp(280px,25vw,360px)] shrink-0 min-h-0 bg-[#f8fafc]/80 border-l border-black/[0.04] flex flex-col p-[clamp(14px,1.6vw,18px)] overflow-hidden z-20 gap-[clamp(12px,2vh,18px)]">
      <UpNextCard />
      <LyricsPanel />
      <AudioInfoCard />
    </aside>
  );
}
