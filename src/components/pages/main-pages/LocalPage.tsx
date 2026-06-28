import { useMemo } from "react";
import { usePlayerStore } from "@/store/player";
import { TrackRows } from "./TrackRows";
import { isLocalTrack } from "./trackFilters";

export function LocalPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  return <TrackRows tracks={localTracks} empty="暂无本地曲目" />;
}

