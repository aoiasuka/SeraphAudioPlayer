import { useMemo } from "react";
import { trackById } from "@/lib/trackIndex";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { TrackRows } from "./TrackRows";

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

  return <TrackRows tracks={tracks} empty="播放过的曲目会显示在这里" scopeName="最近播放" />;
}

export function LikedPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const tracks = useMemo(
    () => playlist.filter((track) => liked[track.id]),
    [playlist, liked]
  );

  return <TrackRows tracks={tracks} empty="还没有收藏曲目" scopeName="我喜欢" />;
}

