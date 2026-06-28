import { beforeEach, describe, expect, it } from "vitest";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

function testTrack(overrides: Partial<Track>): Track {
  return {
    id: "track-a",
    title: "Track A",
    artist: "Artist",
    album: "Album",
    cover: "",
    format: "FLAC",
    bitdepth: "FLAC 24-bit / 96 kHz",
    sampleRate: "96 kHz",
    bitrate: "Unknown",
    channels: "Stereo",
    size: "1.0 MB",
    path: "C:/Music/a.flac",
    sourceUrl: null,
    sourceId: null,
    cacheMissing: false,
    duration: 180,
    glowColor: "#fff",
    glow1: "#fff",
    glow2: "#000",
    lyrics: [],
    ...overrides,
  };
}

describe("player store output driver", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      driverKind: "direct",
      notification: null,
    });
  });

  it("blocks the unfinished ASIO backend", () => {
    usePlayerStore.getState().setDriver("asio");

    const state = usePlayerStore.getState();
    expect(state.driverKind).toBe("direct");
    expect(state.notification?.text).toContain("ASIO 输出尚未开放");
  });

  it("allows implemented output drivers", () => {
    usePlayerStore.getState().setDriver("wasapi");

    expect(usePlayerStore.getState().driverKind).toBe("wasapi");
  });
});

describe("player store track deletion", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      playlist: [
        testTrack({ id: "track-a", title: "Alpha", path: "C:/Music/a.flac" }),
        testTrack({ id: "track-b", title: "Beta", path: "C:/Music/b.flac" }),
        testTrack({ id: "track-c", title: "Gamma", path: "C:/Music/c.flac" }),
      ],
      currentTrackIndex: 1,
      recentTrackIds: ["track-b", "track-a"],
      isPlaying: true,
      currentTime: 42,
      liked: { "track-a": true, "track-b": true },
      userPlaylists: [
        {
          id: "playlist-1",
          name: "Mix",
          trackIds: ["track-a", "track-b", "track-c"],
          createdAt: 1,
        },
      ],
      notification: null,
    });
  });

  it("removes the current track record and clears every local reference", async () => {
    await usePlayerStore.getState().deleteTrack("track-b");

    const state = usePlayerStore.getState();
    expect(state.playlist.map((track) => track.id)).toEqual([
      "track-a",
      "track-c",
    ]);
    expect(state.currentTrackIndex).toBe(1);
    expect(state.currentTrack()?.id).toBe("track-c");
    expect(state.isPlaying).toBe(false);
    expect(state.currentTime).toBe(0);
    expect(state.recentTrackIds).toEqual(["track-a"]);
    expect(state.liked).toEqual({ "track-a": true });
    expect(state.userPlaylists[0].trackIds).toEqual(["track-a", "track-c"]);
    expect(state.notification?.text).toContain("已从曲库移除：Beta");
  });

  it("keeps the same current song when deleting an earlier record", async () => {
    usePlayerStore.setState({
      currentTrackIndex: 2,
      isPlaying: true,
      currentTime: 25,
    });

    await usePlayerStore.getState().deleteTrack("track-a");

    const state = usePlayerStore.getState();
    expect(state.playlist.map((track) => track.id)).toEqual([
      "track-b",
      "track-c",
    ]);
    expect(state.currentTrackIndex).toBe(1);
    expect(state.currentTrack()?.id).toBe("track-c");
    expect(state.isPlaying).toBe(true);
    expect(state.currentTime).toBe(25);
  });
});
