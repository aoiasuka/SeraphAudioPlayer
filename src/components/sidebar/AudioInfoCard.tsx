import { ImagePlus, Loader2 } from "lucide-react";
import { useState } from "react";
import { describeOutputChain, isDsdPath } from "@/lib/outputChain";
import { cn } from "@/lib/utils";
import { useEqStore } from "@/store/eq";
import { usePlayerStore } from "@/store/player";
import { isStreamingTrack } from "@/components/pages/main-pages/trackFilters";

interface Row {
  label: string;
  value: string;
  /** 需要用户留意的值（发生了重采样）用印章红 */
  notable?: boolean;
}

function SpecRows({ rows }: { rows: Row[] }) {
  return (
    <div className="font-tw text-[12px] leading-[2] text-ink">
      {rows.map((r) => {
        const label = r.label.replace(":", "").toUpperCase();
        const dots = ".".repeat(Math.max(2, 12 - label.length));
        return (
          <div key={r.label} className="flex min-w-0">
            <span className="shrink-0 whitespace-pre text-ink3">
              {label}
              {dots}{" "}
            </span>
            <b
              className={cn("min-w-0 truncate font-bold", r.notable && "text-stamp")}
              title={r.value}
            >
              {r.value}
            </b>
          </div>
        );
      })}
    </div>
  );
}

export function AudioInfoCard() {
  const track = usePlayerStore((s) => s.currentTrack());
  const outputFormat = usePlayerStore((s) => s.outputFormat);
  const fetchOnlineCover = usePlayerStore((s) => s.fetchOnlineCoverForCurrentTrack);
  const eqEnabled = useEqStore((s) => s.enabled);
  const crossfeedEnabled = useEqStore((s) => s.crossfeed.enabled);
  const applyToDsd = useEqStore((s) => s.applyToDsd);
  const [matching, setMatching] = useState(false);

  if (!track) return null;

  const parsedSampleRate = track.bitdepth.includes(" / ")
    ? track.bitdepth.split(" / ").slice(1).join(" / ")
    : undefined;
  const sampleRate = track.sampleRate ?? parsedSampleRate ?? "Unknown";
  // 本地曲目且无封面时提供在线匹配入口（B 站曲目封面来自视频，不提供）
  const canMatchCover = !track.cover && !isStreamingTrack(track);

  const handleMatchCover = async () => {
    if (matching) return;
    setMatching(true);
    try {
      await fetchOnlineCover();
    } finally {
      setMatching(false);
    }
  };

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

  // 输出链路：只显示属于当前曲目的那一次建流结果（切歌后、起播前不沿用上一首的）
  const chain =
    outputFormat && outputFormat.trackId === track.id
      ? describeOutputChain(outputFormat, {
          eqEnabled,
          crossfeedEnabled,
          dspBypassedForDsd: isDsdPath(track.path) && !applyToDsd,
        })
      : null;

  return (
    <div className="space-y-2">
      <div className="border-[1.5px] border-ink bg-card p-4 shadow-[3px_3px_0_rgba(43,39,34,0.12)]">
        <h3 className="flex items-center justify-between font-tw text-[9px] tracking-[3px] text-ink3 mb-2.5">
          <span>SPEC SHEET — 音频信息</span>
          <i className="not-italic font-bold text-stamp">● REC</i>
        </h3>
        <SpecRows rows={rows} />
        {chain ? (
          <div
            className="mt-2 border-t border-dashed border-line pt-2"
            data-testid="output-chain"
          >
            <p className="mb-1 font-tw text-[9px] tracking-[3px] text-ink3">
              SIGNAL PATH — 输出链路
            </p>
            <SpecRows
              rows={[
                { label: "Output:", value: chain.output },
                { label: "Process:", value: chain.processing, notable: chain.resampled },
              ]}
            />
          </div>
        ) : null}
        {canMatchCover ? (
          <button
            type="button"
            onClick={() => void handleMatchCover()}
            disabled={matching}
            className="stamp-btn mt-2.5 inline-flex h-7 items-center gap-1.5 px-2.5 font-tw text-[10px] font-bold disabled:cursor-not-allowed disabled:opacity-50"
            title="按标题与艺术家在线搜索专辑封面"
          >
            {matching ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <ImagePlus className="h-3 w-3" />
            )}
            {matching ? "匹配中…" : "在线匹配封面"}
          </button>
        ) : null}
      </div>
    </div>
  );
}
