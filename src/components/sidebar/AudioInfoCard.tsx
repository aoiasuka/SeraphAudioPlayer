import { usePlayerStore } from "@/store/player";

interface Row {
  label: string;
  value: string;
}

export function AudioInfoCard() {
  const track = usePlayerStore((s) => s.currentTrack());

  if (!track) return null;

  const parsedSampleRate = track.bitdepth.includes(" / ")
    ? track.bitdepth.split(" / ").slice(1).join(" / ")
    : undefined;
  const sampleRate = track.sampleRate ?? parsedSampleRate ?? "Unknown";

  const rows: Row[] = [
    { label: "Format:", value: track.format },
    { label: "Bitrate:", value: track.bitrate },
    {
      label: "Sample Rate:",
      value: sampleRate,
    },
    { label: "Channels:", value: track.channels },
    { label: "File Size:", value: track.size },
  ];

  return (
    <div className="space-y-2">
      <h3 className="text-[10px] font-bold text-slate-400 tracking-wider uppercase">
        Audio Info 音频信息
      </h3>
      <div className="p-3 bg-white/60 border border-black/[0.04] rounded-lg space-y-2 text-[10px] font-mono text-slate-600">
        {rows.map((r, i) => (
          <div
            key={r.label}
            className={`flex justify-between py-0.5 ${
              i < rows.length - 1 ? "border-b border-black/[0.02]" : ""
            }`}
          >
            <span className="text-slate-400">{r.label}</span>
            <span className="text-slate-700 font-semibold">{r.value}</span>
          </div>
        ))}
      </div>
    </div>
  );
}
