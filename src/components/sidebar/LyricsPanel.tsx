import {
  type ChangeEvent,
  type FormEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { CloudDownload, Copy, Download, Loader2, Search, Upload } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { KaraokeLine } from "@/components/lyrics/KaraokeLine";
import { TypewriterText } from "@/components/ui/TypewriterText";
import { useSmoothTime } from "@/hooks/useSmoothTime";
import { copyText } from "@/lib/clipboard";
import { candidateBadges } from "@/lib/lyrics/candidate";
import {
  activeVisibleIndex,
  hasWordTiming,
  isInIntermission,
  lyricsPositionMs,
  resolveVisibleGroups,
} from "@/lib/lyrics/activeLine";
import { lyricLines } from "@/lib/lyrics/document";
import { cn } from "@/lib/utils";
import { showContextMenu, type ContextMenuEntry } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { LyricGroup } from "@/lib/lyrics/activeLine";
import type { LyricLine, OnlineLyricsCandidate } from "@/types/track";

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

export function LyricsPanel() {
  const track = usePlayerStore((s) => s.currentTrack());
  // 歌词定位按可听位置（毫秒）：引擎进度减去输出延迟（进度条等仍用原始 currentTime）
  const currentMs = usePlayerStore((s) => lyricsPositionMs(s.currentTime, s.outputLatency));
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
  const exportLyricsForCurrentTrack = usePlayerStore(
    (s) => s.exportLyricsForCurrentTrack
  );
  const showNotification = usePlayerStore((s) => s.showNotification);
  const toggleSettings = usePlayerStore((s) => s.toggleSettings);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const onlineTriggerRef = useRef<HTMLElement | null>(null);
  const lineRefs = useRef<Array<HTMLDivElement | null>>([]);
  // 发现3：打开弹窗/文件选择器那一刻锁定的曲目 id，应用前校验曲目未被切换
  const pinnedTrackIdRef = useRef<string | null>(null);
  // 发现16：用户手动滚动后 3 秒内暂停歌词自动跟随
  const lastUserScrollAtRef = useRef(0);
  const lastScrollContextRef = useRef<{
    trackId: string;
    groups: LyricGroup[];
    padding: number;
  } | null>(null);
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
  const rawLyrics = useMemo(() => lyricLines(track), [track]);
  const showTranslation = usePlayerStore((s) => s.showLyricsTranslation);
  const showRoman = usePlayerStore((s) => s.showLyricsRoman);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const ttmlLyricsEnabled = usePlayerStore((s) => s.ttmlLyricsEnabled);
  // 排除规则由后端打 hidden 标记：全量分组用于定位、可见分组用于渲染，
  // 被隐藏句的时间区间不并入上一句（该区间内不高亮任何行）。
  const resolvedGroups = useMemo(() => resolveVisibleGroups(rawLyrics), [rawLyrics]);
  const lyricGroups = resolvedGroups.visible;
  const lyrics = useMemo(
    () => lyricGroups.flatMap((group) => group.lines),
    [lyricGroups]
  );
  // 原始歌词非空但全部被排除规则隐藏
  const allHiddenByRules = rawLyrics.length > 0 && lyricGroups.length === 0;
  const trackId = track?.id ?? "empty";
  const selectedCandidate =
    onlineCandidates.find((candidate) => candidate.id === selectedCandidateId) ??
    onlineCandidates[0] ??
    null;

  const activeIdx = useMemo(
    () => activeVisibleIndex(resolvedGroups, currentMs),
    [resolvedGroups, currentMs]
  );
  const activeLine = activeIdx >= 0 ? lyricGroups[activeIdx]?.lines[0] : undefined;
  const activeHasWords = hasWordTiming(activeLine);
  const smoothMs = useSmoothTime(currentMs, isPlaying, activeHasWords);
  // 逐字来源带行结束时间：一句唱完且距下一句尚远时，当前句淡出（间奏）
  const intermission = useMemo(
    () => isInIntermission(resolvedGroups, activeIdx, currentMs),
    [resolvedGroups, activeIdx, currentMs]
  );

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
    if (!container) return;
    const previous = lastScrollContextRef.current;
    const contentChanged = previous?.trackId !== trackId || previous.groups !== lyricGroups;
    const resized = previous?.padding !== centerPadding;
    lastScrollContextRef.current = { trackId, groups: lyricGroups, padding: centerPadding };

    // 切歌/替换歌词立即定位，不能沿用上一首的滚动位置或手动浏览暂停窗口。
    if (contentChanged) lastUserScrollAtRef.current = 0;
    if (!contentChanged && Date.now() - lastUserScrollAtRef.current < 3000) return;
    // 前奏尚未到第一句时也展示开头，避免停留在上一首末尾的滚动位置。
    const active = lineRefs.current[Math.max(0, activeIdx)];
    const top = active
      ? active.offsetTop - container.clientHeight / 2 + active.clientHeight / 2
      : 0;
    container.scrollTo({
      top: Math.max(0, top),
      behavior: contentChanged || resized ? "instant" : "smooth",
    });
  }, [activeIdx, centerPadding, trackId, lyricGroups]);

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
    onlineTriggerRef.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    pinnedTrackIdRef.current = track?.id ?? null;
    setManualSearchQuery(track?.title ?? "");
    await runOnlineLyricsSearch();
  };

  // 播放条右键菜单与沉浸歌词工具栏共用本面板的匹配流程。
  // handler 每次渲染变化，经 ref 转发保持监听器只挂一次。
  const openLyricsSearchRef = useRef(handleOnlineLyricsClick);
  openLyricsSearchRef.current = handleOnlineLyricsClick;
  useEffect(() => {
    const onOpenSearch = () => void openLyricsSearchRef.current();
    window.addEventListener("seraph:open-lyrics-search", onOpenSearch);
    return () =>
      window.removeEventListener("seraph:open-lyrics-search", onOpenSearch);
  }, []);

  useEffect(() => {
    if (!onlineDialogOpen) return;
    return () => {
      const trigger = onlineTriggerRef.current;
      if (trigger?.isConnected) trigger.focus({ preventScroll: true });
    };
  }, [onlineDialogOpen]);

  const copyLyricsText = async (text: string) => {
    const copied = await copyText(text);
    showNotification(copied ? "已复制歌词" : "复制失败");
  };

  /** 歌词稿右键菜单；在具体歌词行上触发时附带「复制这句」。 */
  const buildLyricsMenuEntries = (line?: string): ContextMenuEntry[] => {
    const entries: ContextMenuEntry[] = [];
    if (line) {
      entries.push({
        key: "copy-line",
        label: "复制这句",
        icon: Copy,
        onSelect: () => void copyLyricsText(line),
      });
    }
    entries.push(
      {
        key: "copy-all",
        label: "复制整篇歌词",
        icon: Copy,
        disabled: lyrics.length === 0,
        onSelect: () =>
          void copyLyricsText(lyrics.map((item) => item.text).join("\n")),
      },
      { type: "separator", key: "sep-actions" },
      {
        key: "search-online",
        label: "在线匹配歌词",
        icon: CloudDownload,
        onSelect: () => void handleOnlineLyricsClick(),
      },
      {
        key: "import-local",
        label: "导入本地歌词…",
        icon: Upload,
        onSelect: handleImportClick,
      },
      {
        key: "export-lrc",
        label: "导出歌词…",
        icon: Download,
        disabled: rawLyrics.length === 0,
        children: [
          {
            key: "export-enhanced",
            label: "增强型 LRC（ESLyric）",
            hint: "<t> 逐字",
            onSelect: () => void exportLyricsForCurrentTrack("enhanced"),
          },
          {
            key: "export-verbatim",
            label: "逐字 LRC",
            hint: "[t] 逐字",
            onSelect: () => void exportLyricsForCurrentTrack("verbatim"),
          },
          {
            key: "export-line",
            label: "逐行 LRC",
            onSelect: () => void exportLyricsForCurrentTrack("line"),
          },
        ],
      }
    );
    return entries;
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
        selectedCandidate.lyrics,
        selectedCandidate.lookupKeys
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
          onContextMenu={(event) =>
            showContextMenu(event, buildLyricsMenuEntries())
          }
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
              <div className="flex h-full min-h-[180px] flex-col items-center justify-center gap-3 text-center">
                {allHiddenByRules ? (
                  <>
                    <p className="font-tw text-xs font-medium text-ink3">
                      歌词已被排除规则全部隐藏
                    </p>
                    <button
                      type="button"
                      onClick={() => {
                        toggleSettings();
                        // 设置弹窗若监听该事件可直接切到「歌词设置」标签
                        window.dispatchEvent(
                          new CustomEvent("seraph:open-settings-tab", { detail: "lyrics" })
                        );
                      }}
                      className="inline-flex h-7 items-center border-[1.5px] border-line bg-card px-2.5 font-tw text-[11px] font-bold text-ink2 transition-all hover:border-ink hover:text-ink"
                    >
                      打开歌词设置
                    </button>
                  </>
                ) : (
                  <p className="font-tw text-xs font-medium text-ink3">暂无歌词稿</p>
                )}
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
                        if (canSeek) seek(group.startMs / 1000);
                      }}
                      onContextMenu={(event) => {
                        // 阻断冒泡：外层歌词稿容器也挂了菜单，避免二次打开覆盖行级条目
                        event.stopPropagation();
                        showContextMenu(
                          event,
                          buildLyricsMenuEntries(
                            group.lines.map((item) => item.text).join("\n")
                          )
                        );
                      }}
                      className={cn(
                        "flex items-start gap-2 px-1 transition-all duration-300 ease-out origin-left",
                        canSeek ? "cursor-pointer" : "cursor-default",
                        active
                          ? intermission
                            ? "opacity-55"
                            : "opacity-100"
                          : "opacity-40 hover:opacity-70"
                      )}
                      data-intermission={active && intermission ? "true" : undefined}
                    >
                      <div className="min-w-0 space-y-0.5">
                        {group.lines.map((line, lineIdx) => (
                          <div key={`${track.id}-${idx}-${lineIdx}`}>
                            <p
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
                                hasWordTiming(line) ? (
                                  <KaraokeLine
                                    words={line.words}
                                    currentMs={smoothMs}
                                    lineEndMs={line.endMs}
                                  />
                                ) : (
                                  <TypewriterText text={line.text} />
                                )
                              ) : (
                                line.text
                              )}
                            </p>
                            {showTranslation
                              ? (line.translations ?? []).map((translation, translationIndex) => (
                                  <p
                                    key={`t-${translationIndex}`}
                                    className={cn(
                                      "break-words font-tw leading-[22px]",
                                      active ? "text-[12px] text-ink2" : "text-[11px] text-ink3"
                                    )}
                                  >
                                    {translation.text}
                                  </p>
                                ))
                              : null}
                            {showRoman && line.roman ? (
                              <p
                                className={cn(
                                  "break-words font-tw leading-[20px] italic",
                                  active ? "text-[11px] text-ink3" : "text-[10px] text-ink3/80"
                                )}
                              >
                                {line.roman.text}
                              </p>
                            ) : null}
                          </div>
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

      {createPortal(<Dialog
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
              {/* N-09:向第三方外发曲目元数据属用户数据外发,即便是主动触发也应明示去向。 */}
              <p className="mt-1.5 font-tw text-[10px] leading-relaxed text-ink3">
                匹配时会将曲名与艺术家发送至网易云音乐、酷狗、QQ 音乐进行搜索；不发送音频文件本身。
                {ttmlLyricsEnabled ? "已启用 AMLL TTML DB 逐字歌词查找，命中的逐字歌词会排在最前。" : ""}
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
                        <span className="flex min-w-0 items-center gap-1">
                          <span
                            className={cn(
                              "truncate border px-1.5 py-0.5 font-tw text-[10px] font-bold",
                              candidate.id.startsWith("ttml-")
                                ? "border-stamp bg-stamp-soft text-stamp"
                                : "border-brown bg-paper2 text-brown"
                            )}
                          >
                            {candidate.source}
                          </span>
                          {candidateBadges(candidate.lyrics.lines).map((badge) => (
                            <span
                              key={badge}
                              data-testid="candidate-badge"
                              className={cn(
                                "shrink-0 border px-1 py-0.5 font-tw text-[10px]",
                                badge === "逐字"
                                  ? "border-ink bg-ink text-paper"
                                  : "border-line text-ink3"
                              )}
                            >
                              {badge}
                            </span>
                          ))}
                        </span>
                        <span className="shrink-0 font-tw text-[10px] text-ink3">
                          {duration || `${candidate.lyrics.lines.length} 行`}
                        </span>
                      </span>
                      <span className="mt-2 block truncate font-serif text-xs font-bold text-ink">
                        {candidate.title}
                      </span>
                      <span className="mt-0.5 block truncate font-tw text-[11px] text-ink2">
                        {candidate.artist || "Unknown Artist"}
                      </span>
                      <span className="mt-2 line-clamp-2 font-tw text-[11px] leading-relaxed text-ink3">
                        {lyricPreview(candidate.lyrics.lines)}
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
                  {selectedCandidate.lyrics.lines.map((line, index) => (
                    <div
                      key={`${selectedCandidate.id}-${index}`}
                      className="grid grid-cols-[52px_minmax(0,1fr)] gap-3 px-2 py-1.5 text-left hover:bg-paper2"
                    >
                      <span className="font-tw text-[11px] text-ink3">
                        {formatCandidateDuration(line.startMs / 1000)}
                      </span>
                      <span className="font-serif text-xs leading-relaxed text-ink">
                        {line.text}
                        {(line.translations ?? []).map((translation, translationIndex) => (
                          <span
                            key={`t-${translationIndex}`}
                            className="mt-0.5 block font-tw text-[11px] text-ink2"
                          >
                            {translation.text}
                          </span>
                        ))}
                        {line.roman ? (
                          <span className="block font-tw text-[10px] italic text-ink3">
                            {line.roman.text}
                          </span>
                        ) : null}
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
      </Dialog>, document.body)}
    </>
  );
}
