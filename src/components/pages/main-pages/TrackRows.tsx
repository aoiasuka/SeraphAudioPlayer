import { ListPlus, Loader2, Heart, Search, Trash2, X } from "lucide-react";
import { useLayoutEffect, useMemo, useRef, useState, type UIEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { formatSeconds } from "@/lib/format";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import {
  filterAndSortTracks,
  TRACK_SORT_OPTIONS,
  type TrackSortKey,
} from "./trackFilters";

const TRACK_ROW_HEIGHT = 59;
const TRACK_ROW_OVERSCAN = 6;

function compactQualityLabel(track: Track) {
  const prefix = `${track.format} `;
  return track.bitdepth.startsWith(prefix)
    ? track.bitdepth.slice(prefix.length)
    : track.bitdepth;
}

export function TrackRows({ tracks, empty }: { tracks: Track[]; empty: string }) {
  const playlist = usePlayerStore((s) => s.playlist);
  const currentTrack = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const liked = usePlayerStore((s) => s.liked);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const toggleLike = usePlayerStore((s) => s.toggleLike);
  const deleteTrack = usePlayerStore((s) => s.deleteTrack);
  const userPlaylists = usePlayerStore((s) => s.userPlaylists);
  const addTrackToUserPlaylist = usePlayerStore((s) => s.addTrackToUserPlaylist);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(420);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [trackToDeleteId, setTrackToDeleteId] = useState<string | null>(null);
  const [isDeletingTrack, setIsDeletingTrack] = useState(false);
  const [trackToAddId, setTrackToAddId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [sortKey, setSortKey] = useState<TrackSortKey>("default");
  // 搜索 + 排序后的视图列表。点击播放仍经 trackIndexById 映射回全局队列索引，
  // 因此过滤/排序只改变展示，不影响播放队列与切歌逻辑。
  const displayTracks = useMemo(
    () => filterAndSortTracks(tracks, query, sortKey),
    [tracks, query, sortKey]
  );
  const trackIndexById = useMemo(() => {
    const indexById = new Map<string, number>();
    playlist.forEach((track, index) => indexById.set(track.id, index));
    return indexById;
  }, [playlist]);
  const trackToDelete = useMemo(
    () =>
      tracks.find((track) => track.id === trackToDeleteId) ??
      playlist.find((track) => track.id === trackToDeleteId) ??
      null,
    [playlist, trackToDeleteId, tracks]
  );
  const trackToAdd = useMemo(
    () =>
      tracks.find((track) => track.id === trackToAddId) ??
      playlist.find((track) => track.id === trackToAddId) ??
      null,
    [playlist, trackToAddId, tracks]
  );
  const visibleRows = useMemo(() => {
    const start = Math.max(
      0,
      Math.floor(scrollTop / TRACK_ROW_HEIGHT) - TRACK_ROW_OVERSCAN
    );
    const end = Math.min(
      displayTracks.length,
      Math.ceil((scrollTop + viewportHeight) / TRACK_ROW_HEIGHT) +
        TRACK_ROW_OVERSCAN
    );

    return {
      tracks: displayTracks.slice(start, end),
      start,
      paddingTop: start * TRACK_ROW_HEIGHT,
      paddingBottom: (displayTracks.length - end) * TRACK_ROW_HEIGHT,
    };
  }, [scrollTop, displayTracks, viewportHeight]);
  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    setScrollTop(element.scrollTop);
  };
  const hasTracks = tracks.length > 0;
  // 发现6：用 ResizeObserver 测量容器实际高度，修复首屏高窗口/resize 时底部曲目空白
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;

    const updateViewport = () => setViewportHeight(element.clientHeight);
    updateViewport();
    const resizeObserver = new ResizeObserver(updateViewport);
    resizeObserver.observe(element);
    return () => resizeObserver.disconnect();
  }, [hasTracks]);
  const closeDeleteDialog = () => {
    if (!isDeletingTrack) setTrackToDeleteId(null);
  };
  const handleDeleteTrack = async () => {
    if (!trackToDelete || isDeletingTrack) return;

    setIsDeletingTrack(true);
    try {
      await deleteTrack(trackToDelete.id);
      setTrackToDeleteId(null);
    } finally {
      setIsDeletingTrack(false);
    }
  };

  const resetScroll = () => {
    setScrollTop(0);
    scrollRef.current?.scrollTo({ top: 0 });
  };

  if (tracks.length === 0) {
    return (
      <div className="flex min-h-[260px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
        {empty}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="font-tw text-[10px] tracking-[3px] text-ink3 mb-2 flex justify-between">
        <span>INDEX — 播放队列</span>
        <span>
          {query.trim() || sortKey !== "default"
            ? `${displayTracks.length} / ${tracks.length} RECORDS`
            : `${tracks.length} RECORDS`}
        </span>
      </div>
      {/* 检索 + 排序工具行 */}
      <div className="mb-3 flex items-center gap-2">
        <label className="flex h-8 min-w-0 flex-1 items-center gap-2 border-[1.5px] border-line bg-card px-2.5 transition-colors focus-within:border-ink">
          <Search className="h-3.5 w-3.5 shrink-0 text-ink3" />
          <input
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              resetScroll();
            }}
            placeholder="检索标题 / 艺术家 / 专辑…"
            className="min-w-0 flex-1 bg-transparent font-tw text-xs text-ink outline-none placeholder:text-ink3"
            aria-label="检索曲目"
          />
          {query ? (
            <button
              type="button"
              onClick={() => {
                setQuery("");
                resetScroll();
              }}
              className="text-ink3 transition-colors hover:text-stamp"
              aria-label="清除检索"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </label>
        <select
          value={sortKey}
          onChange={(event) => {
            setSortKey(event.target.value as TrackSortKey);
            resetScroll();
          }}
          className="h-8 shrink-0 cursor-pointer border-[1.5px] border-line bg-card px-2 font-tw text-xs font-bold text-ink2 outline-none transition-colors hover:border-ink focus:border-ink"
          aria-label="排序方式"
        >
          {TRACK_SORT_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </div>
      {displayTracks.length === 0 ? (
        <div className="flex min-h-[160px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
          没有匹配「{query.trim()}」的曲目
        </div>
      ) : (
      <div
        ref={scrollRef}
        className="min-h-0 flex-1 overflow-y-auto pr-1"
        onScroll={handleScroll}
      >
        <div style={{ paddingTop: visibleRows.paddingTop, paddingBottom: visibleRows.paddingBottom }}>
        {visibleRows.tracks.map((track, visibleIndex) => {
          const index = trackIndexById.get(track.id) ?? -1;
          const active = currentTrack?.id === track.id;
          const favorite = !!liked[track.id];
          const displayIndex = visibleRows.start + visibleIndex + 1;
          const recNo = displayIndex
            .toString()
            .padStart(3, "0");
          const playing = active && isPlaying;

          return (
            <div
              key={track.id}
              className={cn(
                "archive-card group relative grid h-[49px] grid-cols-[58px_minmax(0,1fr)_118px_64px_40px_34px_34px] items-center gap-3 px-4 mb-2.5",
                active && "is-playing"
              )}
            >
              {playing && (
                <div className="absolute -top-[7px] left-1/2 -translate-x-1/2 -rotate-2 w-[72px] h-[16px] bg-stamp/[0.18] border-x border-dashed border-stamp/30" />
              )}
              <div className="font-tw text-[11px] text-ink3 leading-tight">
                REC.
                <b className="block text-[15px] text-ink font-bold">
                  <span className={active ? "text-stamp" : undefined}>
                    {recNo}
                  </span>
                </b>
              </div>
              <button
                type="button"
                onClick={() => {
                  if (index >= 0) loadTrack(index);
                }}
                disabled={index < 0}
                className="min-w-0 text-left disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={`播放 ${track.title}`}
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  {playing && (
                    <span className="bars shrink-0">
                      <i />
                      <i />
                      <i />
                    </span>
                  )}
                  <span className="block min-w-0 truncate font-serif text-[15px] font-semibold text-ink leading-tight">
                    {track.title}
                  </span>
                </span>
                <span className="block truncate font-tw text-[11px] text-ink2 mt-0.5">
                  {track.artist} — {track.album}
                </span>
              </button>

              <div
                className={cn(
                  "font-tw text-[10px] leading-tight text-ink2",
                  track.cacheMissing && "text-stamp"
                )}
                title={track.bitdepth}
              >
                {track.cacheMissing ? (
                  <>
                    <b className="block text-stamp underline underline-offset-2">
                      ↻ 需重缓存
                    </b>
                    CACHE EXPIRED
                  </>
                ) : (
                  <>
                    <b className="block text-brown">
                      {compactQualityLabel(track)}
                    </b>
                    {track.format.toUpperCase()}
                  </>
                )}
              </div>
              <span className="text-right font-tw text-[13px] font-bold text-ink2">
                {formatSeconds(track.duration)}
              </span>
              <button
                type="button"
                onClick={() => toggleLike(track.id)}
                className={cn(
                  "w-8 h-8 flex items-center justify-center transition-colors",
                  favorite
                    ? "text-stamp"
                    : "text-ink3 hover:text-stamp"
                )}
                aria-label={favorite ? "取消收藏" : "收藏"}
              >
                <Heart className="w-3.5 h-3.5" fill={favorite ? "currentColor" : "none"} />
              </button>
              <button
                type="button"
                onClick={() => setTrackToAddId(track.id)}
                className="flex h-8 w-8 items-center justify-center text-ink3 opacity-0 transition-all hover:text-ink focus:opacity-100 focus:text-ink group-hover:opacity-100"
                aria-label={`把 ${track.title} 加入歌单`}
                title="加入歌单"
              >
                <ListPlus className="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                onClick={() => setTrackToDeleteId(track.id)}
                className="flex h-8 w-8 items-center justify-center text-ink3 opacity-0 transition-all hover:text-stamp focus:opacity-100 focus:text-stamp group-hover:opacity-100"
                aria-label={`删除曲库记录 ${track.title}`}
                title="删除曲库记录"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          );
        })}
        </div>
      </div>
      )}
      <Dialog
        open={Boolean(trackToDelete)}
        onClose={closeDeleteDialog}
        className="max-w-sm"
      >
        <div className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              Delete Track
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">
              删除曲库记录
            </h2>
            <p className="mt-2 font-tw text-xs leading-relaxed text-ink2">
              确定从曲库中移除「{trackToDelete?.title}」吗？这只会删除软件内记录，不会删除磁盘上的音频文件。
            </p>
          </div>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={closeDeleteDialog}
              disabled={isDeletingTrack}
              className="stamp-btn h-9 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-60"
            >
              取消
            </button>
            <button
              type="button"
              onClick={handleDeleteTrack}
              disabled={isDeletingTrack}
              className="inline-flex h-9 items-center gap-2 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper transition-colors hover:brightness-110 disabled:cursor-wait disabled:opacity-70"
            >
              {isDeletingTrack ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Trash2 className="h-3.5 w-3.5" />
              )}
              <span>{isDeletingTrack ? "删除中" : "删除记录"}</span>
            </button>
          </div>
        </div>
      </Dialog>
      <Dialog
        open={Boolean(trackToAdd)}
        onClose={() => setTrackToAddId(null)}
        className="max-w-sm"
      >
        <div className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              Add To Playlist
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">
              加入歌单
            </h2>
            <p className="mt-2 truncate font-tw text-xs text-ink2">
              「{trackToAdd?.title}」
            </p>
          </div>
          {userPlaylists.length === 0 ? (
            <p className="border-[1.5px] border-dashed border-line bg-card p-3 font-tw text-xs leading-relaxed text-ink3">
              还没有歌单——先到「歌单」页新建一个。
            </p>
          ) : (
            <div className="max-h-[240px] space-y-1.5 overflow-y-auto pr-1">
              {userPlaylists.map((item) => {
                const included = trackToAdd
                  ? item.trackIds.includes(trackToAdd.id)
                  : false;
                return (
                  <button
                    key={item.id}
                    type="button"
                    disabled={included}
                    onClick={() => {
                      if (trackToAdd) {
                        addTrackToUserPlaylist(item.id, trackToAdd.id);
                      }
                      setTrackToAddId(null);
                    }}
                    className="flex w-full items-center justify-between border-[1.5px] border-line bg-card px-3 py-2 text-left font-tw text-xs font-bold text-ink transition-colors hover:border-ink disabled:cursor-not-allowed disabled:opacity-50"
                  >
                    <span className="min-w-0 truncate">{item.name}</span>
                    <span className="shrink-0 text-[10px] text-ink3">
                      {included ? "已加入" : `${item.trackIds.length} 首`}
                    </span>
                  </button>
                );
              })}
            </div>
          )}
          <div className="flex justify-end">
            <button
              type="button"
              onClick={() => setTrackToAddId(null)}
              className="stamp-btn h-9 px-3 font-tw text-xs font-bold"
            >
              关闭
            </button>
          </div>
        </div>
      </Dialog>
    </div>
  );
}

