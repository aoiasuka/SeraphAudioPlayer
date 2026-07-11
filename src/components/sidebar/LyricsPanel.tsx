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
  // 审2-R12：与 formatSeconds 同修——Infinity 会绕过 <=0 判断产生 "Infinity:NaN"
  if (!duration || !Number.isFinite(duration) || duration <= 0) return "";
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

function TypewriterText({ text }: { text: string }) {
  const [displayedLength, setDisplayedLength] = useState(0);

  useEffect(() => {
    setDisplayedLength(0);
    if (!text) return;
    
    const speed = Math.max(30, Math.min(80, 800 / text.length));
    
    const interval = setInterval(() => {
      setDisplayedLength((prev) => {
        if (prev >= text.length) {
          clearInterval(interval);
          return text.length;
        }
        return prev + 1;
      });
    }, speed);

    return () => clearInterval(interval);
  }, [text]);

  return <span className="type-caret">{text.slice(0, displayedLength)}</span>;
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
  const showNotification = usePlayerStore((s) => s.showNotification);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const lineRefs = useRef<Array<HTMLDivElement | null>>([]);
  // 发现3：打开弹窗/文件选择器那一刻锁定的曲目 id，应用前校验曲目未被切换
  const pinnedTrackIdRef = useRef<string | null>(null);
  // 发现16：用户手动滚动后 3 秒内暂停歌词自动跟随
  const lastUserScrollAtRef = useRef(0);
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
    // L-10: 节流到 200ms，避免连续切换 group 时 smooth-scroll 互相打断
    const handle = window.setTimeout(() => {
      // 发现16：用户刚手动滚动过，暂停自动跟随
      if (Date.now() - lastUserScrollAtRef.current < 3000) return;
      const top =
        active.offsetTop - container.clientHeight / 2 + active.clientHeight / 2;
      container.scrollTo({
        top: Math.max(0, top),
        behavior: "smooth",
      });
    }, 200);
    return () => window.clearTimeout(handle);
  }, [activeIdx, centerPadding, trackId]);

  useEffect(() => {
    lineRefs.current = [];
  }, [trackId]);

  // 发现16：监听用户手动滚动（wheel / pointerdown），记录时间戳
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const markUserScroll = () => {
      lastUserScrollAtRef.current = Date.now();
    };
    container.addEventListener("wheel", markUserScroll, { passive: true });
    container.addEventListener("pointerdown", markUserScroll);
    return () => {
      container.removeEventListener("wheel", markUserScroll);
      container.removeEventListener("pointerdown", markUserScroll);
    };
  }, [trackId]);

  const handleImportClick = () => {
    if (isImporting) return;
    pinnedTrackIdRef.current = track?.id ?? null;
    fileInputRef.current?.click();
  };

  const runOnlineLyricsSearch = async (query?: string) => {
    if (isFetchingOnline) return null;

    setOnlineCandidates([]);
    setSelectedCandidateId("");
    setOnlineDialogOpen(true);
    setIsFetchingOnline(true);
    try {
      const candidates = await fetchOnlineLyricsForCurrentTrack(query);
      setOnlineCandidates(candidates);
      setSelectedCandidateId(candidates[0]?.id ?? "");
      return candidates;
    } finally {
      setIsFetchingOnline(false);
    }
  };

  const handleOnlineLyricsClick = async () => {
    pinnedTrackIdRef.current = track?.id ?? null;
    setManualSearchQuery(track?.title ?? "");
    await runOnlineLyricsSearch();
  };

  const handleManualLyricsSearch = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    // 审2-R9：手动搜索是对“发起搜索时的当前曲目”的新意图。弹窗打开期间自动切歌后，
    // pinned 若仍停留在旧曲目，应用歌词会被发现3的校验误拒；搜索成功后把 pinned
    // 更新为搜索发起时快照的曲目 id。
    const searchTrackId = track?.id ?? null;
    const candidates = await runOnlineLyricsSearch(manualSearchQuery);
    if (candidates && searchTrackId) {
      pinnedTrackIdRef.current = searchTrackId;
    }
  };

  const handleApplyOnlineLyrics = async () => {
    if (!selectedCandidate || isApplyingOnline) return;

    // 发现3：弹窗打开期间曲目被切换（如自动切歌）时，拒绝把歌词写进错误的曲目
    const pinnedTrackId = pinnedTrackIdRef.current;
    if (pinnedTrackId && track?.id !== pinnedTrackId) {
      showNotification("曲目已切换，歌词未应用");
      setOnlineDialogOpen(false);
      return;
    }

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

    // 发现3：文件选择器打开期间曲目被切换时，拒绝把歌词写进错误的曲目
    const pinnedTrackId = pinnedTrackIdRef.current;
    if (pinnedTrackId && track?.id !== pinnedTrackId) {
      showNotification("曲目已切换，歌词未应用");
      return;
    }

    setIsImporting(true);
    try {
      await importLyricsForCurrentTrack(file);
    } finally {
      setIsImporting(false);
    }
  };

  if (!track) return null;

  // 审2-R7：与 WaveformProgress 的 canSeek 一致——duration 未知(<=0)时点击歌词行不触发 seek，
  // 否则 seek 会被钳制成 0 导致进度直接回开头。
  const canSeek = track.duration > 0;

  return (
    <>
      <div className="flex-1 min-h-0 flex flex-col overflow-hidden space-y-2">
        <div className="flex items-center justify-between gap-2">
          <h3 className="font-tw text-[10px] font-bold text-ink3 tracking-[3px] uppercase truncate shrink-0">
            歌词稿
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
              className="inline-flex h-7 shrink-0 items-center gap-1.5 border-[1.5px] border-ink bg-card px-2.5 font-tw text-[11px] font-bold text-ink transition-all hover:bg-paper2 disabled:cursor-wait disabled:opacity-70"
              aria-label="在线匹配"
              title="在线匹配"
            >
              {isFetchingOnline ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <CloudDownload className="h-3.5 w-3.5" />
              )}
              <span>在线匹配</span>
            </button>
            <button
              type="button"
              onClick={handleImportClick}
              disabled={isImporting || isFetchingOnline}
              className="inline-flex h-7 shrink-0 items-center gap-1.5 border-[1.5px] border-line bg-card px-2.5 font-tw text-[11px] font-bold text-ink2 transition-all hover:border-ink hover:text-ink disabled:cursor-wait disabled:opacity-70"
              aria-label="导入"
              title="导入歌词"
            >
              {isImporting ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Upload className="h-3.5 w-3.5" />
              )}
              <span>导入</span>
            </button>
          </div>
        </div>
        <div
          className="relative flex-1 min-h-0 overflow-hidden flex flex-col border-[1.5px] border-line bg-card p-5"
          style={{
            backgroundImage:
              "repeating-linear-gradient(0deg, transparent 0 27px, rgba(122,92,62,0.07) 27px 28px)",
          }}
        >
          <div
            ref={containerRef}
            className="relative flex-1 min-h-0 overflow-y-auto overflow-x-hidden pr-1 text-left no-scrollbar"
          >
            {lyricGroups.length === 0 ? (
              <div className="flex h-full min-h-[180px] items-center justify-center text-center">
                <p className="font-tw text-xs font-medium text-ink3">暂无歌词稿</p>
              </div>
            ) : (
              <div className="flex min-h-full flex-col gap-2">
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
                      onClick={() => {
                        if (canSeek) seek(group.time);
                      }}
                      className={cn(
                        "flex items-start gap-2 px-1 transition-all duration-300 ease-out origin-left",
                        canSeek ? "cursor-pointer" : "cursor-default",
                        active ? "opacity-100" : "opacity-40 hover:opacity-70"
                      )}
                    >
                      <div className="min-w-0 space-y-0.5">
                        {group.lines.map((line, lineIdx) => (
                          <p
                            key={`${track.id}-${idx}-${lineIdx}`}
                            className={cn(
                              "break-words font-serif leading-[28px] transition-all duration-300 ease-out",
                              active
                                ? lineIdx === 0
                                  ? "text-[16.5px] font-semibold text-ink"
                                  : "text-[13px] font-medium text-ink2"
                                : "text-[14px] text-ink3"
                            )}
                          >
                            {active && lineIdx === 0 ? (
                              <TypewriterText text={line.text} />
                            ) : (
                              line.text
                            )}
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
        <div className="grid max-h-[78vh] min-h-[520px] grid-cols-[280px_minmax(0,1fr)] bg-card">
          <aside className="min-h-0 border-r-2 border-ink bg-paper2 p-4">
            <div className="mb-3">
              <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
                Online Lyrics
              </p>
              <h2 className="mt-1 font-serif text-lg font-bold text-ink">
                {isFetchingOnline ? "正在搜索在线歌词" : "选择在线歌词"}
              </h2>
              <p className="mt-1 font-tw text-[11px] leading-relaxed text-ink2">
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
                className="mb-1.5 block font-tw text-[10px] font-bold uppercase tracking-[0.14em] text-ink3"
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
                  className="h-8 min-w-0 flex-1 border-[1.5px] border-ink bg-card px-2.5 font-tw text-xs font-medium text-ink outline-none transition-colors placeholder:text-ink3 focus:border-stamp disabled:cursor-wait disabled:bg-paper2"
                />
                <button
                  type="submit"
                  disabled={isFetchingOnline}
                  className="stamp-btn inline-flex h-8 w-8 shrink-0 items-center justify-center disabled:cursor-wait disabled:opacity-60"
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
                <div className="flex h-56 flex-col items-center justify-center border-[1.5px] border-dashed border-line bg-card text-center">
                  <Loader2 className="h-5 w-5 animate-spin text-brown" />
                  <p className="mt-3 font-tw text-xs font-semibold text-ink">
                    正在搜索，请稍等
                  </p>
                  <p className="mt-1 font-tw text-[11px] text-ink3">
                    正在从 QQ 音乐、网易云音乐、酷狗音乐获取歌词
                  </p>
                </div>
              ) : onlineCandidates.length === 0 ? (
                <div className="flex h-56 items-center justify-center border-[1.5px] border-dashed border-line bg-card px-4 text-center font-tw text-xs font-medium text-ink3">
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
                        "w-full border-[1.5px] p-3 text-left transition-all",
                        active
                          ? "border-ink bg-card shadow-[3px_3px_0_var(--stamp)]"
                          : "border-line bg-card hover:border-ink"
                      )}
                    >
                      <span className="flex items-center justify-between gap-2">
                        <span className="border border-brown bg-paper2 px-1.5 py-0.5 font-tw text-[10px] font-bold text-brown">
                          {candidate.source}
                        </span>
                        <span className="font-tw text-[10px] text-ink3">
                          {duration || `${candidate.lyrics.length} 行`}
                        </span>
                      </span>
                      <span className="mt-2 block truncate font-serif text-xs font-bold text-ink">
                        {candidate.title}
                      </span>
                      <span className="mt-0.5 block truncate font-tw text-[11px] text-ink2">
                        {candidate.artist || "Unknown Artist"}
                      </span>
                      <span className="mt-2 line-clamp-2 font-tw text-[11px] leading-relaxed text-ink3">
                        {lyricPreview(candidate.lyrics)}
                      </span>
                    </button>
                  );
                })
              )}
            </div>
          </aside>

          <section className="flex min-h-0 flex-col">
            <header className="border-b-2 border-ink px-5 py-4">
              <div className="flex items-start justify-between gap-4">
                <div className="min-w-0">
                  <p className="font-tw text-[10px] font-bold uppercase tracking-[0.16em] text-ink3">
                    {selectedCandidate?.source ?? "Preview"}
                  </p>
                  <h3 className="mt-1 truncate font-serif text-base font-bold text-ink">
                    {selectedCandidate?.title ??
                      (isFetchingOnline ? "正在搜索歌词" : "未选择歌词")}
                  </h3>
                  <p className="mt-1 truncate font-tw text-[11px] text-ink2">
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
                  className="inline-flex h-9 shrink-0 items-center gap-2 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:cursor-wait disabled:bg-line disabled:border-line disabled:text-ink2"
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
                  <Loader2 className="h-6 w-6 animate-spin text-brown" />
                  <p className="mt-3 font-tw text-sm font-semibold text-ink">
                    正在搜索，请稍等
                  </p>
                </div>
              ) : selectedCandidate ? (
                <div className="space-y-1">
                  {selectedCandidate.lyrics.map((line, index) => (
                    <div
                      key={`${selectedCandidate.id}-${index}`}
                      className="grid grid-cols-[52px_minmax(0,1fr)] gap-3 px-2 py-1.5 text-left hover:bg-paper2"
                    >
                      <span className="font-tw text-[11px] text-ink3">
                        {formatCandidateDuration(line.time)}
                      </span>
                      <span className="font-serif text-xs leading-relaxed text-ink">
                        {line.text}
                      </span>
                    </div>
                  ))}
                </div>
              ) : (
                <div className="flex h-full min-h-[260px] items-center justify-center font-tw text-xs font-medium text-ink3">
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
