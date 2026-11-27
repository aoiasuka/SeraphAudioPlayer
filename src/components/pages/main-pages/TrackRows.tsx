import { ListPlus, ListChecks, Heart, Search, Trash2, X } from "lucide-react";
import { useDeferredValue, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { useVirtualRows } from "@/hooks/useVirtualRows";
import { formatSeconds } from "@/lib/format";
import { trackIndexById as sharedTrackIndexById } from "@/lib/trackIndex";
import { buildTrackMenuEntries } from "@/lib/trackMenu";
import { cn } from "@/lib/utils";
import { useRememberedState } from "@/lib/viewMemory";
import { showContextMenu, useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { PlayScope } from "@/store/player/types";
import type { Track } from "@/types/track";
import {
  filterAndSortTracks,
  isStreamingTrack,
  TRACK_SORT_OPTIONS,
  type TrackSortKey,
} from "./trackFilters";

const TRACK_ROW_HEIGHT = 59;

function compactQualityLabel(track: Track) {
  const prefix = `${track.format} `;
  return track.bitdepth.startsWith(prefix)
    ? track.bitdepth.slice(prefix.length)
    : track.bitdepth;
}

/**
 * `stateKey`：给出后，搜索词、排序方式和滚动位置按这个键记在内存里，切换页面再回来
 * 时恢复（见 `lib/viewMemory.ts`）。不同列表用不同的键；不传则每次从头开始。
 */
export function TrackRows({ tracks, empty, scopeName = "当前列表", stateKey }: { tracks: Track[]; empty: string; scopeName?: string; stateKey?: string }) {
  const playlist = usePlayerStore((s) => s.playlist);
  const currentTrack = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const liked = usePlayerStore((s) => s.liked);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const toggleLike = usePlayerStore((s) => s.toggleLike);
  const userPlaylists = usePlayerStore((s) => s.userPlaylists);
  const addTrackToUserPlaylist = usePlayerStore((s) => s.addTrackToUserPlaylist);
  // v0.4.3：删除确认弹窗全局化（右键菜单与行内按钮共用），此处只发起请求
  const requestDeleteTrack = useContextMenuStore((s) => s.requestDeleteTrack);
  const requestDeleteTracks = useContextMenuStore((s) => s.requestDeleteTracks);
  const [selectionMode, setSelectionMode] = useState(false);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
  const selectAllRef = useRef<HTMLInputElement | null>(null);
  const [trackToAddId, setTrackToAddId] = useState<string | null>(null);
  const [query, setQuery] = useRememberedState(stateKey ? `${stateKey}:query` : null, "");
  const deferredQuery = useDeferredValue(query);
  const [sortKey, setSortKey] = useRememberedState<TrackSortKey>(stateKey ? `${stateKey}:sort` : null, "default");
  // 搜索 + 排序后的视图列表。点击播放时经 trackIndexById 映射回曲库下标。
  const displayTracks = useMemo(
    () => filterAndSortTracks(tracks, deferredQuery, sortKey),
    [tracks, deferredQuery, sortKey]
  );
  // PLAY6-01：从这个列表点歌，队列就是列表当前显示的内容（含筛选与排序）。
  const playScope = useMemo<PlayScope>(
    () => ({
      key: stateKey ?? `tracks:${scopeName}`,
      label: scopeName,
      trackIds: displayTracks.map((track) => track.id),
    }),
    [displayTracks, scopeName, stateKey]
  );
  const displayIds = useMemo(() => new Set(displayTracks.map((track) => track.id)), [displayTracks]);
  const selectedTracks = useMemo(() => displayTracks.filter((track) => selectedIds.has(track.id)), [displayTracks, selectedIds]);
  const allSelected = displayTracks.length > 0 && selectedTracks.length === displayTracks.length;
  const isFiltering = query !== deferredQuery;
  const deleteScope = query.trim() ? `${scopeName} · 搜索「${query.trim()}」的结果` : scopeName;
  useEffect(() => {
    // 删除、取消收藏或切换筛选后，不保留不可见曲目的选择。
    setSelectedIds((previous) => {
      const next = new Set([...previous].filter((id) => displayIds.has(id)));
      return next.size === previous.size ? previous : next;
    });
  }, [displayIds]);
  useLayoutEffect(() => {
    if (selectAllRef.current) {
      selectAllRef.current.indeterminate = selectedTracks.length > 0 && !allSelected;
    }
  }, [selectedTracks.length, allSelected, selectionMode]);
  const toggleSelection = (trackId: string) => {
    setSelectedIds((previous) => {
      const next = new Set(previous);
      if (next.has(trackId)) next.delete(trackId);
      else next.add(trackId);
      return next;
    });
  };
  const trackIndexById = useMemo(() => sharedTrackIndexById(playlist), [playlist]);
  const trackToAdd = useMemo(
    () =>
      tracks.find((track) => track.id === trackToAddId) ??
      playlist.find((track) => track.id === trackToAddId) ??
      null,
    [playlist, trackToAddId, tracks]
  );
  // UI3-02：虚拟滚动走共享的 `useVirtualRows`（此前这里另有一套手写实现，要与它分别维护）。
  // M-17：滚动容器的挂载条件是「过滤后非空」——搜索无结果会卸载容器、清词后重挂，
  // `mountKey` 跟着这个信号变，hook 才会重新观察新容器的高度。
  const rows = useVirtualRows(displayTracks.length, TRACK_ROW_HEIGHT, {
    mountKey: displayTracks.length > 0,
    stateKey,
  });
  const visibleTracks = useMemo(
    () => displayTracks.slice(rows.start, rows.end),
    [displayTracks, rows.start, rows.end]
  );
  // 搜索词、排序变了：列表内容整体换了，回到顶部
  const resetScroll = rows.scrollToTop;

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
      <div className="mb-3 flex flex-wrap items-center gap-2">
        <label className="flex h-8 min-w-[180px] flex-1 items-center gap-2 border-[1.5px] border-line bg-card px-2.5 transition-colors focus-within:border-ink">
          <Search className="h-3.5 w-3.5 shrink-0 text-ink3" />
          <input
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setSelectedIds(new Set());
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
                setSelectedIds(new Set());
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
        <button
          type="button"
          aria-label={selectionMode ? "退出多选" : "多选曲目"}
          aria-pressed={selectionMode}
          onClick={() => { setSelectionMode(!selectionMode); setSelectedIds(new Set()); }}
          className="stamp-btn inline-flex h-8 shrink-0 items-center gap-1.5 px-2.5 font-tw text-xs font-bold"
        >
          {selectionMode ? <X className="h-3.5 w-3.5" /> : <ListChecks className="h-3.5 w-3.5" />}
          {selectionMode ? "退出多选" : "多选"}
        </button>
        <button
          type="button"
          disabled={displayTracks.length === 0 || isFiltering}
          onClick={() => requestDeleteTracks(displayTracks.map((track) => track.id), { scope: deleteScope, all: true })}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 border-[1.5px] border-stamp/40 px-2.5 font-tw text-xs font-bold text-stamp transition-colors hover:border-stamp hover:bg-stamp/5 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <Trash2 className="h-3.5 w-3.5" />
          全部删除
        </button>
      </div>
      {selectionMode && (
        <div className="mb-3 flex flex-wrap items-center gap-3 border-[1.5px] border-line bg-card px-3 py-2 font-tw text-xs">
          <label className="flex cursor-pointer items-center gap-2 font-bold text-ink">
            <input
              ref={selectAllRef}
              type="checkbox"
              checked={allSelected}
              disabled={displayTracks.length === 0 || isFiltering}
              onChange={() => setSelectedIds(allSelected ? new Set() : new Set(displayIds))}
              className="h-4 w-4 accent-stamp"
              aria-label={query.trim() ? "全选搜索结果" : "全选当前列表"}
            />
            {query.trim() ? "全选搜索结果" : "全选"}
          </label>
          <span className="text-ink3" aria-live="polite">已选择 {selectedTracks.length} / {displayTracks.length} 首</span>
          <button
            type="button"
            disabled={selectedTracks.length === 0 || isFiltering}
            onClick={() => requestDeleteTracks(selectedTracks.map((track) => track.id), { scope: deleteScope })}
            className="ml-auto inline-flex h-7 items-center gap-1.5 border border-stamp bg-stamp px-2.5 font-bold text-paper transition-colors hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Trash2 className="h-3.5 w-3.5" />删除所选
          </button>
        </div>
      )}
      {displayTracks.length === 0 ? (
        <div className="flex min-h-[160px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
          没有匹配「{query.trim()}」的曲目
        </div>
      ) : (
      <div
        ref={rows.scrollRef}
        className="min-h-0 flex-1 overflow-y-auto pr-1"
        onScroll={rows.onScroll}
      >
        <div style={{ paddingTop: rows.paddingTop, paddingBottom: rows.paddingBottom }}>
        {visibleTracks.map((track, visibleIndex) => {
          const index = trackIndexById.get(track.id) ?? -1;
          const active = currentTrack?.id === track.id;
          const favorite = !!liked[track.id];
          const displayIndex = rows.start + visibleIndex + 1;
          const recNo = displayIndex
            .toString()
            .padStart(3, "0");
          const playing = active && isPlaying;
          const deleteLabel = isStreamingTrack(track) ? "删除曲目及缓存文件" : "删除曲库记录";

          return (
            <div
              key={track.id}
              onDoubleClick={() => {
                // 双击行内任意处即从头强制播放本曲目（含停止态）
                if (!selectionMode && index >= 0) loadTrack(index, { forcePlay: true, scope: playScope });
              }}
              onContextMenu={(event) =>
                showContextMenu(event, buildTrackMenuEntries(track, { scope: playScope }))
              }
              className={cn(
                "archive-card group relative grid h-[49px] select-none grid-cols-[50px_minmax(0,1fr)_64px_30px_30px_30px] xl:grid-cols-[58px_minmax(0,1fr)_118px_64px_40px_34px_34px] items-center gap-2 xl:gap-3 px-3 xl:px-4 mb-2.5",
                active && "is-playing",
                selectionMode && selectedIds.has(track.id) && "border-stamp bg-stamp/5"
              )}
            >
              {playing && (
                <div className="absolute -top-[7px] left-1/2 -translate-x-1/2 -rotate-2 w-[72px] h-[16px] bg-stamp/[0.18] border-x border-dashed border-stamp/30" />
              )}
              {selectionMode ? (
                <input
                  type="checkbox"
                  checked={selectedIds.has(track.id)}
                  onChange={() => toggleSelection(track.id)}
                  aria-label={`选择 ${track.title}`}
                  className="h-4 w-4 cursor-pointer accent-stamp"
                />
              ) : <div className="font-tw text-[11px] text-ink3 leading-tight">
                REC.
                <b className="block text-[15px] text-ink font-bold">
                  <span className={active ? "text-stamp" : undefined}>
                    {recNo}
                  </span>
                </b>
              </div>}
              <button
                type="button"
                onClick={() => {
                  if (selectionMode) toggleSelection(track.id);
                  else if (index >= 0) loadTrack(index, { scope: playScope });
                }}
                disabled={!selectionMode && index < 0}
                className="min-w-0 text-left disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={`${selectionMode ? "切换选择" : "播放"} ${track.title}`}
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
                  "font-tw text-[10px] leading-tight text-ink2 hidden xl:block",
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
                onClick={() => requestDeleteTrack(track.id)}
                className="flex h-8 w-8 items-center justify-center text-ink3 opacity-0 transition-all hover:text-stamp focus:opacity-100 focus:text-stamp group-hover:opacity-100"
                aria-label={`${deleteLabel} ${track.title}`}
                title={deleteLabel}
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

