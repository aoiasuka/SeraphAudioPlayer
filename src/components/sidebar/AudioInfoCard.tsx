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
      <div className="border-[1.5px] border-ink bg-card p-4 shadow-[3px_3px_0_rgba(43,39,34,0.12)]">
        <h3 className="flex items-center justify-between font-tw text-[9px] tracking-[3px] text-ink3 mb-2.5">
          <span>SPEC SHEET — 音频信息</span>
          <i className="not-italic font-bold text-stamp">● REC</i>
        </h3>
        <div className="font-tw text-[12px] leading-[2] text-ink">
          {rows.map((r) => {
            const label = r.label.replace(":", "").toUpperCase();
            const dots = ".".repeat(Math.max(2, 12 - label.length));
            return (
              <div key={r.label}>
                <span className="text-ink3">
                  {label}
                  {dots}{" "}
                </span>
                <b className="font-bold">{r.value}</b>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
