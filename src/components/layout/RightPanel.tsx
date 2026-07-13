import { UpNextCard } from "@/components/sidebar/UpNextCard";
import { LyricsPanel } from "@/components/sidebar/LyricsPanel";
import { AudioInfoCard } from "@/components/sidebar/AudioInfoCard";
import { SpectrumPanel } from "@/components/sidebar/SpectrumPanel";
import { usePlayerStore } from "@/store/player";

export function RightPanel() {
  const hasTrack = usePlayerStore((s) => s.currentTrack() !== null);

  if (!hasTrack) return null;

  return (
    <aside className="w-[clamp(300px,26vw,372px)] shrink-0 min-h-0 bg-paper2 border-l-2 border-ink flex flex-col p-[clamp(14px,1.6vw,20px)] overflow-hidden z-20 gap-[clamp(12px,2vh,18px)]">
      <SpectrumPanel />
      <UpNextCard />
      <LyricsPanel />
      <AudioInfoCard />
    </aside>
  );
}
