import { ArrowLeft, Disc3 } from "lucide-react";
import { useEffect, useMemo } from "react";
import { groupAlbums } from "@/lib/albums";
import { coverSrc } from "@/lib/tauri";
import { buildTrackGroupMenuEntries } from "@/lib/trackMenu";
import { useRememberedState } from "@/lib/viewMemory";
import { showContextMenu } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import { TrackRows } from "./TrackRows";
import { VirtualGrid } from "./VirtualGrid";
import { isLocalTrack } from "./trackFilters";

export function AlbumsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  // 记住打开的专辑：切到别的页面再回来仍停在这张专辑的详情
  const [selectedAlbumKey, setSelectedAlbumKey] = useRememberedState<string | null>("albums:selected", null);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  // LIB5-01：按专辑艺术家分组，组内按碟号、音轨号排序
  const albums = useMemo(() => groupAlbums(localTracks), [localTracks]);
  const activeAlbum = useMemo(
    () => albums.find((album) => album.key === selectedAlbumKey) ?? null,
    [albums, selectedAlbumKey]
  );

  useEffect(() => {
    // 曲库还没载入（启动水合）时不清：否则记住的选择会在数据到来前被抹掉
    if (selectedAlbumKey && !activeAlbum && playlist.length > 0) setSelectedAlbumKey(null);
  }, [activeAlbum, selectedAlbumKey, playlist.length, setSelectedAlbumKey]);

  if (activeAlbum) {
    return (
      <div className="flex min-h-0 flex-1 flex-col gap-3">
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
          <button
            type="button"
            onClick={() => setSelectedAlbumKey(null)}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
          >
            <ArrowLeft className="h-4 w-4" />
            返回专辑
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate font-serif text-sm font-bold text-ink">
              {activeAlbum.title}
            </p>
            <p className="font-tw text-[11px] text-ink2">
              {activeAlbum.artist} · {activeAlbum.tracks.length} 首曲目
            </p>
          </div>
        </div>
        <TrackRows
          key={activeAlbum.key}
          tracks={activeAlbum.tracks}
          empty={`${activeAlbum.title} 暂无曲目`}
          scopeName={activeAlbum.title}
          stateKey={`tracks:album:${activeAlbum.key}`}
        />
      </div>
    );
  }

  return (
    <VirtualGrid items={albums} itemKey={(album) => album.key} label="专辑，方向键浏览" stateKey="albums:grid"
      renderItem={(album) => {
        return (
          <button
            key={album.key}
            type="button"
            onClick={() => setSelectedAlbumKey(album.key)}
            onContextMenu={(event) =>
              showContextMenu(
                event,
                buildTrackGroupMenuEntries(album.tracks, () =>
                  setSelectedAlbumKey(album.key)
                , album.title)
              )
            }
            className="archive-card group flex flex-col p-4 text-left"
          >
            {album.cover ? (
              <span className="mb-4 block w-full aspect-square overflow-hidden border-[1.5px] border-ink shrink-0">
                <img
                  src={coverSrc(album.cover)}
                  loading="lazy"
                  decoding="async"
                  alt=""
                  className="h-full w-full object-cover grayscale-[0.2] transition-transform duration-500 group-hover:scale-110"
                />
              </span>
            ) : (
              <span className="mb-4 flex w-full aspect-square items-center justify-center border-[1.5px] border-ink bg-paper2 text-brown shrink-0">
                <Disc3 className="h-8 w-8" />
              </span>
            )}
            <span className="min-w-0 self-start w-full">
              <span className="block truncate font-serif text-sm font-bold text-ink">
                {album.title}
              </span>
              <span className="mt-1 block truncate font-tw text-[11px] text-ink2">
                {album.artist} · {album.year ?? "Unknown"}
              </span>
              <span className="mt-1 block truncate font-tw text-[11px] text-ink3">
                {album.tracks.length} 首曲目
              </span>
              <span className="mt-3 inline-flex items-center gap-1 border border-brown bg-paper2 px-1.5 py-0.5 font-tw text-[9px] font-bold text-brown">
                <Disc3 className="h-2.5 w-2.5" />
                {album.format}
              </span>
            </span>
          </button>
        );
      }}
    />
  );
}

