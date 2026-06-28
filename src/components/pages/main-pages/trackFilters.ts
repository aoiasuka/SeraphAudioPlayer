import type { Track } from "@/types/track";

export function isStreamingTrack(track: Track) {
  return (
    track.id.startsWith("bilibili-") ||
    track.sourceId?.trim().toLowerCase().startsWith("bv") ||
    track.sourceUrl?.trim().toLowerCase().includes("bilibili.com") ||
    track.album === "Bilibili"
  );
}

export function isLocalTrack(track: Track) {
  return !isStreamingTrack(track);
}

