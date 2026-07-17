import { ArrowLeft, Disc3 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { coverSrc } from "@/lib/tauri";
import { buildTrackGroupMenuEntries } from "@/lib/trackMenu";
import { showContextMenu } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { TrackRows } from "./TrackRows";
import { isLocalTrack } from "./trackFilters";

export function AlbumsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const [selectedAlbumKey, setSelectedAlbumKey] = useState<string | null>(null);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  const albums = useMemo(() => {
    const groups = new Map<
      string,
      {
        key: string;
        title: string;
        artist: string;
        year?: string;
        format: string;
        cover: string;
        tracks: Track[];
      }
    >();

    localTracks.forEach((track) => {
      const key = `${track.artist}::${track.album}`;
      const group = groups.get(key);
      if (group) {
        group.tracks.push(track);
        // 专辑内部分曲目缺封面时，用组内第一张可用封面兜底
        if (!group.cover && track.cover) group.cover = track.cover;
        return;
      }

      groups.set(key, {
        key,
        title: track.album,
        artist: track.artist,
        year: track.albumYear,
        format: track.format,
        cover: track.cover,
        tracks: [track],
      });
    });

    return Array.from(groups.values());
  }, [localTracks]);
  const activeAlbum = useMemo(
    () => albums.find((album) => album.key === selectedAlbumKey) ?? null,
    [albums, selectedAlbumKey]
  );

  useEffect(() => {
    if (selectedAlbumKey && !activeAlbum) setSelectedAlbumKey(null);
  }, [activeAlbum, selectedAlbumKey]);

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
          tracks={activeAlbum.tracks}
          empty={`${activeAlbum.title} 暂无曲目`}
        />
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5 gap-4 overflow-y-auto pr-1">
      {albums.map((album) => {
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
                )
              )
            }
            className="archive-card group flex flex-col p-4 text-left"
          >
            {album.cover ? (
              <span className="mb-4 block w-full aspect-square overflow-hidden border-[1.5px] border-ink shrink-0">
                <img
                  src={coverSrc(album.cover)}
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
      })}
    </div>
  );
}

