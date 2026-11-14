import { useMemo } from "react";
import { trackById } from "@/lib/trackIndex";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { TrackRows } from "./TrackRows";
import { sortLikedTracks } from "./trackFilters";

function isTrack(track: Track | undefined): track is Track {
  return Boolean(track);
}

export function RecentPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const tracks = useMemo(
    () => recentTrackIds.map((id) => trackById(playlist, id)).filter(isTrack),
    [recentTrackIds, playlist]
  );

  return <TrackRows tracks={tracks} empty="播放过的曲目会显示在这里" scopeName="最近播放" stateKey="tracks:recent" />;
}

export function LikedPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const likedAt = usePlayerStore((s) => s.likedAt);
  const tracks = useMemo(
    () => sortLikedTracks(playlist, liked, likedAt),
    [playlist, liked, likedAt]
  );

  return <TrackRows tracks={tracks} empty="还没有收藏曲目" scopeName="我喜欢" stateKey="tracks:liked" />;
}

