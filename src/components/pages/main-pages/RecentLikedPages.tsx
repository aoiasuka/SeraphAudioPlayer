import { useMemo } from "react";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { TrackRows } from "./TrackRows";

function isTrack(track: Track | undefined): track is Track {
  return Boolean(track);
}

export function RecentPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const trackById = useMemo(() => {
    const tracks = new Map<string, Track>();
    playlist.forEach((track) => tracks.set(track.id, track));
    return tracks;
  }, [playlist]);
  const tracks = useMemo(
    () => recentTrackIds.map((id) => trackById.get(id)).filter(isTrack),
    [recentTrackIds, trackById]
  );

  return <TrackRows tracks={tracks} empty="播放过的曲目会显示在这里" />;
}

export function LikedPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const tracks = useMemo(
    () => playlist.filter((track) => liked[track.id]),
    [playlist, liked]
  );

  return <TrackRows tracks={tracks} empty="还没有收藏曲目" />;
}

