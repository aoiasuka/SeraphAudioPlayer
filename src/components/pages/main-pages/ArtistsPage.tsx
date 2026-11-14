import { ArrowLeft, Search, User, X } from "lucide-react";
import { useEffect, useMemo } from "react";
import { filterArtistGroups, groupTracksByArtist } from "@/lib/artists";
import { coverSrc } from "@/lib/tauri";
import { buildTrackGroupMenuEntries } from "@/lib/trackMenu";
import { useRememberedState } from "@/lib/viewMemory";
import { showContextMenu } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import { TrackRows } from "./TrackRows";
import { VirtualGrid } from "./VirtualGrid";
import { isLocalTrack } from "./trackFilters";

const UNKNOWN_ARTIST = "未知艺术家";

export function ArtistsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  // 记住打开的艺术家与筛选词：切到别的页面再回来不必重新找
  const [selectedArtist, setSelectedArtist] = useRememberedState<string | null>(
    "artists:selected",
    null
  );
  const [query, setQuery] = useRememberedState("artists:query", "");
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  // 合作曲目按参与者拆开归组，并按名称排序（规则见 lib/artists.ts）
  const artists = useMemo(() => groupTracksByArtist(localTracks), [localTracks]);
  const visibleArtists = useMemo(() => filterArtistGroups(artists, query), [artists, query]);
  const activeArtist = useMemo(
    () => artists.find((artist) => artist.key === selectedArtist) ?? null,
    [artists, selectedArtist]
  );

  useEffect(() => {
    // 曲库还没载入（启动水合）时不清：否则记住的选择会在数据到来前被抹掉
    if (selectedArtist !== null && !activeArtist && playlist.length > 0) setSelectedArtist(null);
  }, [activeArtist, selectedArtist, playlist.length, setSelectedArtist]);

  if (activeArtist) {
    const displayName = activeArtist.name || UNKNOWN_ARTIST;
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-3">
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
          <button
            type="button"
            onClick={() => setSelectedArtist(null)}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
          >
            <ArrowLeft className="h-4 w-4" />
            返回艺术家
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate font-serif text-sm font-bold text-ink">
              {displayName}
            </p>
            <p className="font-tw text-[11px] text-ink2">
              {activeArtist.tracks.length} 首曲目
            </p>
          </div>
        </div>
        <TrackRows
          key={activeArtist.key}
          tracks={activeArtist.tracks}
          empty={`${displayName} 暂无曲目`}
          scopeName={displayName}
          stateKey={`tracks:artist:${activeArtist.key}`}
        />
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-3 flex items-center gap-3">
        <label className="flex h-8 min-w-[180px] flex-1 items-center gap-2 border-[1.5px] border-line bg-card px-2.5 transition-colors focus-within:border-ink">
          <Search className="h-3.5 w-3.5 shrink-0 text-ink3" />
          <input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="筛选艺术家…"
            className="min-w-0 flex-1 bg-transparent font-tw text-xs text-ink outline-none placeholder:text-ink3"
            aria-label="筛选艺术家"
          />
          {query ? (
            <button
              type="button"
              onClick={() => setQuery("")}
              className="text-ink3 transition-colors hover:text-stamp"
              aria-label="清除筛选"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </label>
        <span className="shrink-0 font-tw text-[10px] tracking-[3px] text-ink3" aria-live="polite">
          {query.trim()
            ? `${visibleArtists.length} / ${artists.length} ARTISTS`
            : `${artists.length} ARTISTS`}
        </span>
      </div>
      {visibleArtists.length === 0 ? (
        <div className="flex min-h-[160px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
          {artists.length === 0 ? "暂无本地曲目" : `没有匹配「${query.trim()}」的艺术家`}
        </div>
      ) : (
        <VirtualGrid
          items={visibleArtists}
          itemKey={(artist) => artist.key}
          label="艺术家，方向键浏览"
          // 筛选时不沿用、也不覆盖完整列表的滚动位置
          stateKey={query.trim() ? undefined : "artists:grid"}
          renderItem={(artist) => {
            const first = artist.tracks[0];
            const displayName = artist.name || UNKNOWN_ARTIST;
            // 艺术家封面：取该艺术家第一首带封面的曲目
            const cover = coverSrc(artist.tracks.find((track) => track.cover)?.cover);
            return (
              <button
                key={artist.key}
                type="button"
                onClick={() => setSelectedArtist(artist.key)}
                onContextMenu={(event) =>
                  showContextMenu(
                    event,
                    buildTrackGroupMenuEntries(artist.tracks, () =>
                      setSelectedArtist(artist.key)
                    , displayName)
                  )
                }
                className="archive-card p-4 text-left"
              >
                {cover ? (
                  <span className="mb-4 block h-10 w-10 overflow-hidden border-[1.5px] border-ink">
                    <img src={cover} alt="" loading="lazy" decoding="async" className="h-full w-full object-cover" />
                  </span>
                ) : (
                  <span className="mb-4 flex h-10 w-10 items-center justify-center border-[1.5px] border-ink bg-paper2 text-brown">
                    <User className="h-4 w-4" />
                  </span>
                )}
                <span className="block truncate font-serif text-sm font-bold text-ink" title={displayName}>{displayName}</span>
                <span className="mt-1 block truncate font-tw text-[11px] text-ink2">
                  {artist.tracks.length} 首曲目 · {first.album}
                </span>
              </button>
            );
          }}
        />
      )}
    </div>
  );
}
