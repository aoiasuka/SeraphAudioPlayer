import {
  type ChangeEvent,
  type FormEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { CloudDownload, Loader2, Search, Upload } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import type { LyricLine, OnlineLyricsCandidate } from "@/types/track";

interface LyricGroup {
  time: number;
  lines: LyricLine[];
}

const SAME_TIMESTAMP_EPSILON = 0.01;

function formatCandidateDuration(duration?: number | null) {
  if (!duration || duration <= 0) return "";
  const minutes = Math.floor(duration / 60);
  const seconds = Math.floor(duration % 60)
    .toString()
    .padStart(2, "0");
  return `${minutes}:${seconds}`;
}

function lyricPreview(lyrics: LyricLine[]) {
  return lyrics
    .slice(0, 3)
    .map((line) => line.text)
    .join(" / ");
}

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
  const fetchOnlineLyricsForCurrentTrack = usePlayerStore(
    (s) => s.fetchOnlineLyricsForCurrentTrack
  );
  const applyOnlineLyricsForCurrentTrack = usePlayerStore(
    (s) => s.applyOnlineLyricsForCurrentTrack
  );
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const lineRefs = useRef<Array<HTMLDivElement | null>>([]);
  const [isImporting, setIsImporting] = useState(false);
  const [isFetchingOnline, setIsFetchingOnline] = useState(false);
  const [isApplyingOnline, setIsApplyingOnline] = useState(false);
  const [onlineCandidates, setOnlineCandidates] = useState<
    OnlineLyricsCandidate[]
  >([]);
  const [selectedCandidateId, setSelectedCandidateId] = useState("");
  const [onlineDialogOpen, setOnlineDialogOpen] = useState(false);
  const [manualSearchQuery, setManualSearchQuery] = useState("");
  const [centerPadding, setCenterPadding] = useState(0);
  const lyrics = track?.lyrics ?? [];
  const lyricGroups = useMemo(() => groupLyricsByTime(lyrics), [lyrics]);
  const trackId = track?.id ?? "empty";
  const selectedCandidate =
    onlineCandidates.find((candidate) => candidate.id === selectedCandidateId) ??
    onlineCandidates[0] ??
    null;

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

  useEffect(() => {
    lineRefs.current = [];
  }, [trackId]);

  const handleImportClick = () => {
    if (isImporting) return;
    fileInputRef.current?.click();
  };

  const runOnlineLyricsSearch = async (query?: string) => {
    if (isFetchingOnline) return;

    setOnlineCandidates([]);
    setSelectedCandidateId("");
    setOnlineDialogOpen(true);
    setIsFetchingOnline(true);
    try {
      const candidates = await fetchOnlineLyricsForCurrentTrack(query);
      setOnlineCandidates(candidates);
      setSelectedCandidateId(candidates[0]?.id ?? "");
    } finally {
      setIsFetchingOnline(false);
    }
  };

  const handleOnlineLyricsClick = async () => {
    setManualSearchQuery(track?.title ?? "");
    await runOnlineLyricsSearch();
  };

  const handleManualLyricsSearch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    await runOnlineLyricsSearch(manualSearchQuery);
  };

  const handleApplyOnlineLyrics = async () => {
    if (!selectedCandidate || isApplyingOnline) return;

    setIsApplyingOnline(true);
    try {
      const applied = await applyOnlineLyricsForCurrentTrack(
        selectedCandidate.lyrics
      );
      if (applied) setOnlineDialogOpen(false);
    } finally {
      setIsApplyingOnline(false);
    }
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
    <>
      <div className="flex-1 min-h-0 flex flex-col overflow-hidden space-y-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-[10px] font-bold text-slate-400 tracking-wider uppercase truncate shrink-0">
            Lyrics 歌词区域
          </h3>
          <input
            ref={fileInputRef}
            type="file"
            accept=".lrc,.qrc,.krc,.yrc,.txt,text/plain"
            className="hidden"
            onChange={handleFileChange}
          />
          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              onClick={handleOnlineLyricsClick}
              disabled={isFetchingOnline || isImporting}
              className="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-md border border-cyan-200/70 bg-cyan-50/70 px-2.5 text-[11px] font-semibold text-cyan-700 shadow-[0_3px_10px_rgba(8,145,178,0.08)] transition-all hover:border-cyan-300 hover:bg-white hover:text-cyan-800 disabled:cursor-wait disabled:opacity-70"
              aria-label="在线歌词"
              title="在线歌词"
            >
              {isFetchingOnline ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <CloudDownload className="h-3.5 w-3.5" />
              )}
              <span>在线歌词</span>
            </button>
            <button
              type="button"
              onClick={handleImportClick}
              disabled={isImporting || isFetchingOnline}
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
        </div>
        <div className="acrylic-card rounded-lg p-3 flex-1 min-h-0 overflow-hidden flex flex-col">
          <div
            ref={containerRef}
            className="relative flex-1 min-h-0 overflow-y-auto pr-1 text-left"
          >
            {lyricGroups.length === 0 ? (
              <div className="flex h-full min-h-[180px] items-center justify-center text-center">
                <p className="text-xs font-medium text-slate-400">暂无歌词</p>
              </div>
            ) : (
              <div className="flex min-h-full flex-col gap-3.5">
                <div
                  aria-hidden="true"
                  style={{ height: `${centerPadding}px` }}
                />
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
                        "flex items-start gap-2 pl-1.5 pr-2 rounded border py-1.5 cursor-pointer transition-all duration-500 ease-out origin-left",
                        active
                          ? "bg-white border-black/[0.04] py-2.5 shadow-[0_4px_12px_rgba(0,0,0,0.03)] scale-[1.02] z-10 opacity-100"
                          : "border-transparent hover:bg-black/[0.02] opacity-40 hover:opacity-80 scale-[0.98]"
                      )}
                    >
                      <div
                        className={cn(
                          "mt-0.5 rounded transition-all duration-500",
                          active
                            ? "w-[3px] h-5 bg-gradient-to-b from-cyan-500 to-blue-600 shadow-[0_0_8px_rgba(8,145,178,0.5)]"
                            : "w-[2px] h-3 bg-transparent"
                        )}
                      />
                      <div className="min-w-0 space-y-1">
                        {group.lines.map((line, lineIdx) => (
                          <p
                            key={`${track.id}-${idx}-${lineIdx}`}
                            className={cn(
                              "break-words tracking-wide leading-relaxed transition-all duration-500 ease-out",
                              active
                                ? lineIdx === 0
                                  ? "text-xs font-bold text-slate-800"
                                  : "text-[11px] font-semibold text-slate-500"
                                : "text-[11px] font-medium text-slate-500"
                            )}
                          >
                            {line.text}
                          </p>
                        ))}
                      </div>
                    </div>
                  );
                })}
                <div
                  aria-hidden="true"
                  style={{ height: `${centerPadding}px` }}
                />
              </div>
            )}
          </div>
        </div>
      </div>

      <Dialog
        open={onlineDialogOpen}
        onClose={() => {
          if (!isApplyingOnline) setOnlineDialogOpen(false);
        }}
        className="max-w-4xl p-0 overflow-hidden rounded-lg"
      >
        <div className="grid max-h-[78vh] min-h-[520px] grid-cols-[280px_minmax(0,1fr)] bg-white">
          <aside className="min-h-0 border-r border-slate-200/70 bg-slate-50/80 p-4">
            <div className="mb-3">
              <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-cyan-700">
                Online Lyrics
              </p>
              <h2 className="mt-1 text-lg font-bold text-slate-800">
                {isFetchingOnline ? "正在搜索在线歌词" : "选择在线歌词"}
              </h2>
              <p className="mt-1 text-[11px] leading-relaxed text-slate-500">
                {isFetchingOnline
                  ? "正在搜索，请稍等。"
                  : onlineCandidates.length > 0
                    ? `已抓取到 ${onlineCandidates.length} 份结果，选择一份预览后应用。`
                    : "未找到匹配歌词，可以关闭弹窗后再试。"}
              </p>
            </div>
            <form onSubmit={handleManualLyricsSearch} className="mb-3">
              <label
                htmlFor="online-lyrics-search"
                className="mb-1.5 block text-[10px] font-bold uppercase tracking-[0.14em] text-slate-400"
              >
                手动搜索
              </label>
              <div className="flex gap-1.5">
                <input
                  id="online-lyrics-search"
                  value={manualSearchQuery}
                  onChange={(event) => setManualSearchQuery(event.target.value)}
                  disabled={isFetchingOnline}
                  placeholder="输入曲名或歌手"
                  className="h-8 min-w-0 flex-1 rounded-md border border-slate-200 bg-white px-2.5 text-xs font-medium text-slate-700 outline-none transition-colors placeholder:text-slate-400 focus:border-cyan-300 focus:ring-2 focus:ring-cyan-100 disabled:cursor-wait disabled:bg-slate-100"
                />
                <button
                  type="submit"
                  disabled={isFetchingOnline}
                  className="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-md border border-cyan-200 bg-cyan-50 text-cyan-700 transition-colors hover:border-cyan-300 hover:bg-white disabled:cursor-wait disabled:opacity-60"
                  aria-label="搜索在线歌词"
                  title="搜索在线歌词"
                >
                  {isFetchingOnline ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <Search className="h-3.5 w-3.5" />
                  )}
                </button>
              </div>
            </form>
            <div className="min-h-0 max-h-[calc(78vh-190px)] space-y-2 overflow-y-auto pr-1">
              {isFetchingOnline ? (
                <div className="flex h-56 flex-col items-center justify-center rounded-md border border-dashed border-cyan-200 bg-white/70 text-center">
                  <Loader2 className="h-5 w-5 animate-spin text-cyan-700" />
                  <p className="mt-3 text-xs font-semibold text-slate-700">
                    正在搜索，请稍等
                  </p>
                  <p className="mt-1 text-[11px] text-slate-400">
                    正在从 QQ 音乐、网易云音乐、酷狗音乐获取歌词
                  </p>
                </div>
              ) : onlineCandidates.length === 0 ? (
                <div className="flex h-56 items-center justify-center rounded-md border border-dashed border-slate-200 bg-white/70 px-4 text-center text-xs font-medium text-slate-400">
                  未找到匹配歌词
                </div>
              ) : (
                onlineCandidates.map((candidate) => {
                  const active = candidate.id === selectedCandidate?.id;
                  const duration = formatCandidateDuration(candidate.duration);
                  return (
                    <button
                      key={candidate.id}
                      type="button"
                      onClick={() => setSelectedCandidateId(candidate.id)}
                      className={cn(
                        "w-full rounded-md border p-3 text-left transition-all",
                        active
                          ? "border-cyan-300 bg-white shadow-[0_8px_24px_rgba(8,145,178,0.10)]"
                          : "border-transparent bg-white/55 hover:border-slate-200 hover:bg-white"
                      )}
                    >
                      <span className="flex items-center justify-between gap-2">
                        <span className="rounded border border-cyan-200 bg-cyan-50 px-1.5 py-0.5 text-[10px] font-bold text-cyan-700">
                          {candidate.source}
                        </span>
                        <span className="text-[10px] font-mono text-slate-400">
                          {duration || `${candidate.lyrics.length} 行`}
                        </span>
                      </span>
                      <span className="mt-2 block truncate text-xs font-bold text-slate-800">
                        {candidate.title}
                      </span>
                      <span className="mt-0.5 block truncate text-[11px] text-slate-500">
                        {candidate.artist || "Unknown Artist"}
                      </span>
                      <span className="mt-2 line-clamp-2 text-[11px] leading-relaxed text-slate-400">
                        {lyricPreview(candidate.lyrics)}
                      </span>
                    </button>
                  );
                })
              )}
            </div>
          </aside>

          <section className="flex min-h-0 flex-col">
            <header className="border-b border-slate-200/70 px-5 py-4">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <p className="text-[10px] font-bold uppercase tracking-[0.16em] text-slate-400">
                    {selectedCandidate?.source ?? "Preview"}
                  </p>
                  <h3 className="mt-1 truncate text-base font-bold text-slate-800">
                    {selectedCandidate?.title ??
                      (isFetchingOnline ? "正在搜索歌词" : "未选择歌词")}
                  </h3>
                  <p className="mt-1 truncate text-[11px] text-slate-500">
                    {selectedCandidate
                      ? `${selectedCandidate.artist || "Unknown Artist"}${
                          selectedCandidate.album
                            ? ` / ${selectedCandidate.album}`
                            : ""
                        }`
                      : isFetchingOnline
                        ? "搜索结束后将在左侧显示候选结果"
                        : "请在左侧选择候选歌词"}
                  </p>
                </div>
                <button
                  type="button"
                  onClick={handleApplyOnlineLyrics}
                  disabled={
                    isFetchingOnline || !selectedCandidate || isApplyingOnline
                  }
                  className="inline-flex h-9 shrink-0 items-center gap-2 rounded-md bg-cyan-700 px-3 text-xs font-bold text-white shadow-[0_8px_22px_rgba(8,145,178,0.20)] transition-colors hover:bg-cyan-800 disabled:cursor-wait disabled:bg-slate-300 disabled:shadow-none"
                >
                  {isApplyingOnline ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <CloudDownload className="h-4 w-4" />
                  )}
                  使用这份歌词
                </button>
              </div>
            </header>
            <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
              {isFetchingOnline ? (
                <div className="flex h-full min-h-[260px] flex-col items-center justify-center text-center">
                  <Loader2 className="h-6 w-6 animate-spin text-cyan-700" />
                  <p className="mt-3 text-sm font-semibold text-slate-700">
                    正在搜索，请稍等
                  </p>
                </div>
              ) : selectedCandidate ? (
                <div className="space-y-2">
                  {selectedCandidate.lyrics.map((line, index) => (
                    <div
                      key={`${selectedCandidate.id}-${index}`}
                      className="grid grid-cols-[52px_minmax(0,1fr)] gap-3 rounded-md px-2 py-1.5 text-left hover:bg-slate-50"
                    >
                      <span className="font-mono text-[11px] text-slate-400">
                        {formatCandidateDuration(line.time)}
                      </span>
                      <span className="text-xs leading-relaxed text-slate-700">
                        {line.text}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="flex h-full min-h-[260px] items-center justify-center text-xs font-medium text-slate-400">
                  暂无可预览歌词
                </div>
              )}
            </div>
          </section>
        </div>
      </Dialog>
    </>
  );
}
