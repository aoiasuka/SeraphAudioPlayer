// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { sortLikedTracks } from "@/components/pages/main-pages/trackFilters";
import { emptyLyricDocument } from "@/lib/lyrics/document";
import { migratePersistedPlayerState, usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

function track(id: string, overrides: Partial<Track> = {}): Track {
  return {
    id,
    title: id,
    artist: "artist",
    album: "album",
    format: "FLAC",
    bitdepth: "16-bit / 44.1 kHz",
    bitrate: "900 kbps",
    channels: "Stereo",
    size: "1 MB",
    path: `C:/music/${id}.flac`,
    cover: "",
    duration: 100,
    glowColor: "#000000",
    glow1: "#000000",
    glow2: "#000000",
    lyrics: emptyLyricDocument(),
    ...overrides,
  } as Track;
}

beforeEach(() => {
  vi.useRealTimers();
  usePlayerStore.setState({
    playlist: [track("a"), track("b"), track("c")],
    currentTrackIndex: 0,
    liked: {},
    likedAt: {},
    userPlaylists: [],
    notification: null,
  });
});

describe("收藏时间", () => {
  it("收藏时记录时间，取消收藏时清除", () => {
    vi.useFakeTimers();
    vi.setSystemTime(1_000);
    usePlayerStore.getState().toggleLike("a");
    expect(usePlayerStore.getState().liked).toEqual({ a: true });
    expect(usePlayerStore.getState().likedAt).toEqual({ a: 1_000 });

    usePlayerStore.getState().toggleLike("a");
    expect(usePlayerStore.getState().liked.a).toBe(false);
    expect(usePlayerStore.getState().likedAt).toEqual({});
  });

  it("「我喜欢」按收藏时间倒序，没有时间的旧收藏保持曲库顺序排在后面", () => {
    const playlist = [track("a"), track("b"), track("c"), track("d")];
    const sorted = sortLikedTracks(
      playlist,
      { a: true, b: true, c: false, d: true },
      { b: 200, d: 500 }
    );
    expect(sorted.map((item) => item.id)).toEqual(["d", "b", "a"]);
  });

  it("水合时丢弃非数字的收藏时间，旧存档没有该字段时得到空表", () => {
    expect(migratePersistedPlayerState({ likedAt: { a: 5, b: "x", c: Number.NaN } }).likedAt)
      .toEqual({ a: 5 });
    expect(migratePersistedPlayerState({}).likedAt).toEqual({});
  });

  it("重复曲目合并后，存活条目继承收藏时间", () => {
    usePlayerStore.setState({
      playlist: [
        track("old", { path: "C:/music/same.flac" }),
        track("new", { path: "c:\\music\\same.flac" }),
      ],
      liked: { new: true },
      likedAt: { new: 900 },
    });
    usePlayerStore.getState().normalizeLibrary();
    const state = usePlayerStore.getState();
    expect(state.playlist).toHaveLength(1);
    const survivor = state.playlist[0].id;
    expect(state.liked[survivor]).toBe(true);
    expect(state.likedAt[survivor]).toBe(900);
  });
});

describe("批量加入歌单", () => {
  it("一次写入、跳过重复与不存在的曲目，并返回实际加入数量", () => {
    const playlistId = usePlayerStore.getState().createUserPlaylist("测试")!;
    usePlayerStore.getState().addTrackToUserPlaylist(playlistId, "a");

    let updates = 0;
    const unsubscribe = usePlayerStore.subscribe((state, previous) => {
      if (state.userPlaylists !== previous.userPlaylists) updates += 1;
    });
    const added = usePlayerStore
      .getState()
      .addTracksToUserPlaylist(playlistId, ["a", "b", "ghost", "c", "b"]);
    unsubscribe();

    expect(added).toBe(2);
    expect(updates).toBe(1);
    expect(usePlayerStore.getState().userPlaylists[0].trackIds).toEqual(["a", "b", "c"]);
    expect(usePlayerStore.getState().notification?.text).toContain("2 首");
  });

  it("silent 选项不发通知；全部已在歌单时不写 store", () => {
    const playlistId = usePlayerStore.getState().createUserPlaylist("测试")!;
    usePlayerStore.setState({ notification: null });
    expect(
      usePlayerStore.getState().addTracksToUserPlaylist(playlistId, ["a"], { silent: true })
    ).toBe(1);
    expect(usePlayerStore.getState().notification).toBeNull();

    const before = usePlayerStore.getState().userPlaylists;
    expect(usePlayerStore.getState().addTracksToUserPlaylist(playlistId, ["a"])).toBe(0);
    expect(usePlayerStore.getState().userPlaylists).toBe(before);
  });
});
