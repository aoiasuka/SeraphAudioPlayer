import { UpNextCard } from "@/components/sidebar/UpNextCard";
import { LyricsPanel } from "@/components/sidebar/LyricsPanel";
import { AudioInfoCard } from "@/components/sidebar/AudioInfoCard";
import { SpectrumPanel } from "@/components/sidebar/SpectrumPanel";
import { usePlayerStore } from "@/store/player";

export function RightPanel() {
  const hasTrack = usePlayerStore((s) => s.currentTrack() !== null);
  const analysisMode = usePlayerStore((s) => s.activeView === "analysis");

  if (!hasTrack) return null;

  // 声学分析全屏模式：频谱/下一首/音频信息让位给分析仪表，只保留歌词栏
  if (analysisMode) {
    return (
      <aside className="w-[clamp(240px,20vw,320px)] shrink-0 min-h-0 bg-paper2 border-l-2 border-ink flex flex-col p-[clamp(12px,1.4vw,18px)] overflow-hidden z-20">
        <LyricsPanel />
      </aside>
    );
  }

  return (
    <aside className="w-[clamp(300px,26vw,372px)] shrink-0 min-h-0 bg-paper2 border-l-2 border-ink flex flex-col p-[clamp(14px,1.6vw,20px)] overflow-hidden z-20 gap-[clamp(12px,2vh,18px)]">
      <SpectrumPanel />
      <UpNextCard />
      <LyricsPanel />
      <AudioInfoCard />
    </aside>
  );
}
