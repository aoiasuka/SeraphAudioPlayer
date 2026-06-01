import { Volume2, Volume1, VolumeX } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import { Slider } from "@/components/ui/slider";

export function VolumeControl() {
  const volume = usePlayerStore((s) => s.volume);
  const setVolume = usePlayerStore((s) => s.setVolume);
  const toggleMute = usePlayerStore((s) => s.toggleMute);

  let VolumeIcon = Volume2;
  let iconClass = "text-cyan-600";
  if (volume === 0) {
    VolumeIcon = VolumeX;
    iconClass = "text-slate-400";
  } else if (volume < 0.4) {
    VolumeIcon = Volume1;
    iconClass = "text-slate-500";
  }

  return (
    <div className="flex items-center gap-1.5">
      <button
        onClick={toggleMute}
        className="text-slate-500 hover:text-slate-800 transition-colors"
        aria-label="静音切换"
      >
        <VolumeIcon className={`w-3 h-3 ${iconClass}`} />
      </button>
      <Slider
        className="w-14"
        value={volume}
        onChange={(e) => setVolume(parseFloat(e.currentTarget.value))}
      />
    </div>
  );
}
