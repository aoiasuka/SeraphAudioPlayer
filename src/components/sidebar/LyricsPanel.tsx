import {
  type ChangeEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Loader2, Upload } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import type { LyricLine } from "@/types/track";

interface LyricGroup {
  time: number;
  lines: LyricLine[];
}

const SAME_TIMESTAMP_EPSILON = 0.01;

function groupLyricsByTime(lyrics: LyricLine[]) {
  const groups: LyricGroup[] = [];

  for (const line of lyrics) {
    const previous = groups[groups.length - 1];
    if (
      previous &&
      Math.abs(previous.time - line.time) <= SAME_TIMESTAMP_EPSILON
    ) {
      previous.lines.push(line);
      continue;
    }

    groups.push({ time: line.time, lines: [line] });
  }

  return groups;
}

export function LyricsPanel() {
  const track = usePlayerStore((s) => s.currentTrack());
  const currentTime = usePlayerStore((s) => s.currentTime);
  const seek = usePlayerStore((s) => s.seek);
  const importLyricsForCurrentTrack = usePlayerStore(
    (s) => s.importLyricsForCurrentTrack
  );
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const lineRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [isImporting, setIsImporting] = useState(false);
  const [centerPadding, setCenterPadding] = useState(0);
  const lyrics = track?.lyrics ?? [];
  const lyricGroups = useMemo(() => groupLyricsByTime(lyrics), [lyrics]);
  const trackId = track?.id ?? "empty";

  const activeIdx = useMemo(() => {
    let low = 0;
    let high = lyricGroups.length - 1;
    let match = -1;

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      if (currentTime + SAME_TIMESTAMP_EPSILON >= lyricGroups[mid].time) {
        match = mid;
        low = mid + 1;
      } else {
        high = mid - 1;
      }
    }

    return match;
  }, [lyricGroups, currentTime]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const updatePadding = () => {
      setCenterPadding(Math.max(0, container.clientHeight / 2));
    };

    updatePadding();
    const resizeObserver = new ResizeObserver(updatePadding);
    resizeObserver.observe(container);
    return () => resizeObserver.disconnect();
  }, [trackId]);

  useLayoutEffect(() => {
    const container = containerRef.current;
    if (activeIdx < 0) return;
    const active = lineRefs.current[activeIdx];
    if (!container || !active) return;
    const top =
      active.offsetTop - container.clientHeight / 2 + active.clientHeight / 2;
    container.scrollTo({
      top: Math.max(0, top),
      behavior: "smooth",
    });
  }, [activeIdx, centerPadding, trackId]);

  // 切歌时清空 refs
  useEffect(() => {
    lineRefs.current = [];
  }, [trackId]);

  const handleImportClick = () => {
    if (isImporting) return;
    fileInputRef.current?.click();
  };

  const handleFileChange = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.currentTarget.files?.[0];
    event.currentTarget.value = "";
    if (!file || isImporting) return;

    setIsImporting(true);
    try {
      await importLyricsForCurrentTrack(file);
    } finally {
      setIsImporting(false);
    }
  };

  if (!track) return null;

  return (
    <div className="flex-1 min-h-0 flex flex-col overflow-hidden space-y-2">
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-[10px] font-bold text-slate-400 tracking-wider uppercase truncate shrink-0">
          Lyrics 歌词区域
        </h3>
        <input
          ref={fileInputRef}
          type="file"
          accept=".lrc,.txt,text/plain"
          className="hidden"
          onChange={handleFileChange}
        />
        <button
          type="button"
          onClick={handleImportClick}
          disabled={isImporting}
          className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-slate-200/80 bg-white/70 px-2.5 text-[11px] font-semibold text-slate-600 shadow-[0_3px_10px_rgba(15,23,42,0.05)] transition-all hover:border-cyan-200 hover:bg-white hover:text-cyan-700 disabled:cursor-wait disabled:opacity-70"
          aria-label="导入歌词"
          title="导入歌词"
        >
          {isImporting ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Upload className="h-3.5 w-3.5" />
          )}
          <span>导入歌词</span>
        </button>
      </div>
      <div className="acrylic-card rounded-lg p-3 flex-1 min-h-0 overflow-hidden flex flex-col">
        <div
          ref={containerRef}
          className="relative flex-1 min-h-0 overflow-y-auto pr-1 text-left"
        >
          {lyricGroups.length === 0 ? (
            <div className="flex h-full min-h-[180px] items-center justify-center text-center">
              <p className="text-xs font-medium text-slate-400">
                暂无歌词
              </p>
            </div>
          ) : (
            <div className="flex min-h-full flex-col gap-3.5">
              <div aria-hidden="true" style={{ height: `${centerPadding}px` }} />
              {lyricGroups.map((group, idx) => {
                const active = idx === activeIdx;
                return (
                  <div
                    key={`${track.id}-${idx}`}
                    ref={(el) => {
                      lineRefs.current[idx] = el;
                    }}
                    onClick={() => seek(group.time)}
                    className={cn(
                      "flex items-start gap-2 pl-1.5 pr-2 rounded border py-1 cursor-pointer transition-all duration-300",
                      active
                        ? "bg-white border-black/[0.04] py-1.5 shadow-[0_2px_10px_rgba(0,0,0,0.02)]"
                        : "border-transparent hover:bg-black/[0.02]"
                    )}
                  >
                    <div
                      className={cn(
                        "mt-0.5 rounded transition-colors duration-300",
                        active
                          ? "w-[2.5px] h-5 bg-cyan-600 shadow-[0_0_6px_rgba(8,145,178,0.4)]"
                          : "w-[2px] h-3 bg-transparent"
                      )}
                    />
                    <div className="min-w-0 space-y-1">
                      {group.lines.map((line, lineIdx) => (
                        <p
                          key={`${track.id}-${idx}-${lineIdx}`}
                          className={cn(
                            "break-words tracking-wide leading-normal transition-all duration-300",
                            active
                              ? lineIdx === 0
                                ? "text-[12px] font-semibold text-slate-800"
                                : "text-[11px] font-medium text-slate-500"
                              : "text-[11px] text-slate-400"
                          )}
                        >
                          {line.text}
                        </p>
                      ))}
                    </div>
                  </div>
                );
              })}
              <div aria-hidden="true" style={{ height: `${centerPadding}px` }} />
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
