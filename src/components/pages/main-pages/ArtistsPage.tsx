import { ArrowLeft, User } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { TrackRows } from "./TrackRows";
import { isLocalTrack } from "./trackFilters";

export function ArtistsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const [selectedArtist, setSelectedArtist] = useState<string | null>(null);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  const artists = useMemo(() => {
    const groups = new Map<
      string,
      { name: string; tracks: Track[] }
    >();

    localTracks.forEach((track) => {
      const group = groups.get(track.artist);
      if (group) {
        group.tracks.push(track);
        return;
      }

      groups.set(track.artist, {
        name: track.artist,
        tracks: [track],
      });
    });

    return Array.from(groups.values());
  }, [localTracks]);
  const activeArtist = useMemo(
    () => artists.find((artist) => artist.name === selectedArtist) ?? null,
    [artists, selectedArtist]
  );

  useEffect(() => {
    if (selectedArtist && !activeArtist) setSelectedArtist(null);
  }, [activeArtist, selectedArtist]);

  if (activeArtist) {
    return (
      <div className="flex min-h-0 flex-col gap-3">
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
              {activeArtist.name}
            </p>
            <p className="font-tw text-[11px] text-ink2">
              {activeArtist.tracks.length} 首曲目
            </p>
          </div>
        </div>
        <TrackRows
          tracks={activeArtist.tracks}
          empty={`${activeArtist.name} 暂无曲目`}
        />
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-4 overflow-y-auto pr-1">
      {artists.map((artist) => {
        const first = artist.tracks[0];
        return (
          <button
            key={artist.name}
            type="button"
            onClick={() => setSelectedArtist(artist.name)}
            className="archive-card p-4 text-left"
          >
            <span className="mb-4 flex h-10 w-10 items-center justify-center border-[1.5px] border-ink bg-paper2 text-brown">
              <User className="h-4 w-4" />
            </span>
            <span className="block font-serif text-sm font-bold text-ink">{artist.name}</span>
            <span className="mt-1 block font-tw text-[11px] text-ink2">
              {artist.tracks.length} 首曲目 · {first.album}
            </span>
          </button>
        );
      })}
    </div>
  );
}

