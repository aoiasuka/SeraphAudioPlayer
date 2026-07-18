import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { invoke } from "@/lib/tauri";
import { migratePersistedPlayerState, usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    invoke: vi.fn(async () => undefined) as unknown as typeof actual.invoke,
  };
});

const invokeMock = invoke as unknown as Mock;

async function flushAsyncQueue() {
  for (let i = 0; i < 5; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

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

describe("player store startup and persistence", () => {
  it("starts with an empty production playlist", () => {
    expect(usePlayerStore.getState().playlist).toEqual([]);
  });

  it("migrates old persisted state into bounded current values", () => {
    const migrated = migratePersistedPlayerState({
      currentTrackIndex: -4.8,
      recentTrackIds: ["a", 1, "b"],
      volume: 2,
      previousVolume: Number.NaN,
      liked: { a: true, b: "yes" },
      userPlaylists: "bad",
      currentDeviceId: "",
      driverKind: "usb",
      activeView: "missing",
    });

    expect(migrated.currentTrackIndex).toBe(0);
    expect(migrated.persistedCurrentTrackId).toBe(null);
    expect(migrated.recentTrackIds).toEqual(["a", "b"]);
    expect(migrated.volume).toBe(1);
    expect(migrated.previousVolume).toBe(0.7);
    expect(migrated.liked).toEqual({ a: true });
    expect(migrated.userPlaylists).toEqual([]);
    expect(migrated.currentDeviceId).toBe("wasapi:hd-dac1");
    expect(migrated.driverKind).toBe("wasapi");
    expect(migrated.activeView).toBe("local");
    // v3：旧状态无 rememberPlayback 字段时默认开启（保持既有恢复行为）
    expect(migrated.rememberPlayback).toBe(true);
  });

  it("honors explicit rememberPlayback=false in persisted state", () => {
    const migrated = migratePersistedPlayerState({ rememberPlayback: false });
    expect(migrated.rememberPlayback).toBe(false);
  });

  it("clears persisted playback position when memory playback is turned off", () => {
    usePlayerStore.setState({
      rememberPlayback: true,
      persistedCurrentTrackId: "track-x",
      persistedCurrentTime: 123,
    });
    usePlayerStore.getState().setRememberPlayback(false);
    const state = usePlayerStore.getState();
    expect(state.rememberPlayback).toBe(false);
    expect(state.persistedCurrentTrackId).toBe(null);
    expect(state.persistedCurrentTime).toBe(0);
  });
});

describe("player store user playlists (v0.4.3)", () => {
  beforeEach(() => {
    usePlayerStore.setState({ userPlaylists: [], notification: null });
  });

  it("createUserPlaylist returns the new playlist id", () => {
    const id = usePlayerStore.getState().createUserPlaylist("新建测试");

    const playlists = usePlayerStore.getState().userPlaylists;
    expect(id).toBeTruthy();
    expect(playlists).toHaveLength(1);
    expect(playlists[0].id).toBe(id);
    expect(playlists[0].name).toBe("新建测试");
  });

  it("rejects blank playlist names and returns null", () => {
    const id = usePlayerStore.getState().createUserPlaylist("   ");

    expect(id).toBe(null);
    expect(usePlayerStore.getState().userPlaylists).toHaveLength(0);
  });

  it("renames an existing playlist with trimmed name", () => {
    const id = usePlayerStore.getState().createUserPlaylist("旧名字");
    usePlayerStore.getState().renameUserPlaylist(id!, "  新名字  ");

    expect(usePlayerStore.getState().userPlaylists[0].name).toBe("新名字");
  });

  it("keeps the old name when renaming to blank", () => {
    const id = usePlayerStore.getState().createUserPlaylist("保持原名");
    usePlayerStore.getState().renameUserPlaylist(id!, "   ");

    expect(usePlayerStore.getState().userPlaylists[0].name).toBe("保持原名");
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

describe("player store library normalization (发现1/9)", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async () => undefined);
  });

  it("keeps the persisted index untouched when the playlist is still empty", () => {
    usePlayerStore.setState({
      playlist: [],
      currentTrackIndex: 5,
      persistedCurrentTrackId: "track-b",
      liked: {},
    });

    usePlayerStore.getState().normalizeLibrary();

    const state = usePlayerStore.getState();
    expect(state.currentTrackIndex).toBe(5);
    expect(state.persistedCurrentTrackId).toBe("track-b");
  });

  it("remaps currentTrackIndex by id after dedupe removes an earlier duplicate", () => {
    usePlayerStore.setState({
      playlist: [
        testTrack({ id: "track-a", title: "Alpha", path: "C:/Music/a.flac" }),
        testTrack({ id: "track-a-dup", title: "Alpha Copy", path: "C:/Music/a.flac" }),
        testTrack({ id: "track-b", title: "Beta", path: "C:/Music/b.flac" }),
      ],
      currentTrackIndex: 2,
      liked: {},
    });

    usePlayerStore.getState().normalizeLibrary();

    const state = usePlayerStore.getState();
    expect(state.playlist).toHaveLength(2);
    expect(state.currentTrackIndex).toBe(1);
    expect(state.currentTrack()?.id).toBe("track-b");
  });

  it("restores the last played track by persisted id when hydrating an empty playlist", async () => {
    const backendTracks = [
      testTrack({ id: "track-a", title: "Alpha", path: "C:/Music/a.flac" }),
      testTrack({ id: "track-b", title: "Beta", path: "C:/Music/b.flac" }),
      testTrack({ id: "track-c", title: "Gamma", path: "C:/Music/c.flac" }),
    ];
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "get_playlist" ? backendTracks : undefined
    );
    usePlayerStore.setState({
      playlist: [],
      currentTrackIndex: 0,
      persistedCurrentTrackId: "track-b",
      liked: {},
      recentTrackIds: [],
    });

    await usePlayerStore.getState().loadBackendLibrary();

    const state = usePlayerStore.getState();
    expect(state.playlist).toHaveLength(3);
    expect(state.currentTrackIndex).toBe(1);
    expect(state.currentTrack()?.id).toBe("track-b");
    expect(state.persistedCurrentTrackId).toBe("track-b");
  });

  it("falls back to the first track when the persisted id no longer exists", async () => {
    const backendTracks = [
      testTrack({ id: "track-a", title: "Alpha", path: "C:/Music/a.flac" }),
      testTrack({ id: "track-b", title: "Beta", path: "C:/Music/b.flac" }),
    ];
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "get_playlist" ? backendTracks : undefined
    );
    usePlayerStore.setState({
      playlist: [],
      currentTrackIndex: 0,
      persistedCurrentTrackId: "track-gone",
      liked: {},
      recentTrackIds: [],
    });

    await usePlayerStore.getState().loadBackendLibrary();

    const state = usePlayerStore.getState();
    expect(state.currentTrackIndex).toBe(0);
    expect(state.currentTrack()?.id).toBe("track-a");
    expect(state.persistedCurrentTrackId).toBe("track-a");
  });
});

describe("player store playback epoch (发现2)", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async () => undefined);
    usePlayerStore.setState({
      playlist: [
        testTrack({ id: "track-a", title: "Track A", path: "C:/Music/a.flac" }),
        testTrack({ id: "track-b", title: "Track B", path: "C:/Music/b.flac" }),
      ],
      currentTrackIndex: 0,
      recentTrackIds: [],
      isPlaying: false,
      currentTime: 0,
      notification: null,
    });
  });

  it("starts playback when no newer intent arrives (stub mode)", async () => {
    usePlayerStore.getState().togglePlayback();
    await flushAsyncQueue();

    const state = usePlayerStore.getState();
    expect(state.isPlaying).toBe(true);
    expect(state.notification?.text).toContain("正在播放: Track A");
  });

  it("drops the stale play continuation when a newer track is selected", async () => {
    usePlayerStore.getState().togglePlayback(); // 异步开始播放 track-a
    usePlayerStore.getState().loadTrack(1); // 立即切到 track-b，使上面的续体过期
    await flushAsyncQueue();

    const state = usePlayerStore.getState();
    expect(state.currentTrack()?.id).toBe("track-b");
    expect(state.isPlaying).toBe(false);
    expect(state.notification?.text ?? "").not.toContain("正在播放: Track A");
  });

  it("drops the stale resume continuation when the user pauses again", async () => {
    usePlayerStore.getState().togglePlayback(); // 异步开始播放
    // 用户随即又按了一次暂停（此时 UI 仍是未播放态，模拟为直接置为播放后暂停）
    usePlayerStore.setState({ isPlaying: true });
    usePlayerStore.getState().togglePlayback(); // 暂停，代际递增
    await flushAsyncQueue();

    const state = usePlayerStore.getState();
    expect(state.isPlaying).toBe(false);
    expect(state.notification?.text ?? "").not.toContain("正在播放");
  });
});

describe("player store double-click force play (v0.4.4)", () => {
  beforeEach(() => {
    invokeMock.mockImplementation(async () => undefined);
    usePlayerStore.setState({
      playlist: [
        testTrack({ id: "track-a", title: "Track A", path: "C:/Music/a.flac" }),
        testTrack({ id: "track-b", title: "Track B", path: "C:/Music/b.flac" }),
      ],
      currentTrackIndex: 0,
      recentTrackIds: [],
      isPlaying: false,
      currentTime: 0,
      notification: null,
    });
  });

  it("starts playback from a stopped state when forcePlay is set (stub mode)", async () => {
    // 双击语义：即使当前处于停止态，也从头强制起播选中的曲目
    usePlayerStore.getState().loadTrack(1, { forcePlay: true });
    await flushAsyncQueue();

    const state = usePlayerStore.getState();
    expect(state.currentTrack()?.id).toBe("track-b");
    expect(state.isPlaying).toBe(true);
  });

  it("only selects the track without playing when forcePlay is absent", async () => {
    // 单击语义：停止态下只切换选中曲目，不自动起播
    usePlayerStore.getState().loadTrack(1);
    await flushAsyncQueue();

    const state = usePlayerStore.getState();
    expect(state.currentTrack()?.id).toBe("track-b");
    expect(state.isPlaying).toBe(false);
  });
});

describe("player store reload streaming track (v0.4.4)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    usePlayerStore.setState({
      playlist: [
        testTrack({
          id: "bilibili-BV1xx411c7mD",
          title: "老流媒体",
          artist: "UP",
          album: "Bilibili",
          format: "M4A",
          path: "C:/cache/BV1xx411c7mD-1.m4a",
          sourceUrl: "https://www.bilibili.com/video/BV1xx411c7mD",
          sourceId: "BV1xx411c7mD",
        }),
      ],
      liked: { "bilibili-BV1xx411c7mD": true },
      userPlaylists: [
        {
          id: "pl-1",
          name: "收藏夹",
          trackIds: ["bilibili-BV1xx411c7mD"],
          createdAt: 1,
        },
      ],
      notification: null,
    });
  });

  it("passes the user-picked options through and replaces the record in place", async () => {
    const reloaded = testTrack({
      id: "bilibili-BV1xx411c7mD",
      title: "老流媒体（无损）",
      artist: "UP",
      album: "Bilibili",
      format: "FLAC",
      path: "C:/cache/BV1xx411c7mD-1.flac",
      sourceUrl: "https://www.bilibili.com/video/BV1xx411c7mD",
      sourceId: "BV1xx411c7mD",
    });
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "import_bilibili_audio_with_options" ? reloaded : undefined
    );

    const ok = await usePlayerStore.getState().reloadStreamingTrack(
      "bilibili-BV1xx411c7mD",
      { preferFlac: true, preferDolbyAtmos: false, remuxWithFfmpeg: true }
    );

    expect(ok).toBe(true);
    // 用当次勾选的选项调用后端（此处：FLAC + 混流，杜比关）
    const call = invokeMock.mock.calls.find(
      ([cmd]) => cmd === "import_bilibili_audio_with_options"
    );
    expect(call?.[1]).toMatchObject({
      input: "https://www.bilibili.com/video/BV1xx411c7mD",
      options: { preferFlac: true, preferDolbyAtmos: false, remuxWithFfmpeg: true },
    });

    const state = usePlayerStore.getState();
    // 原位替换：id 不变，收藏与歌单归属保留，内容已更新为无损
    expect(state.playlist).toHaveLength(1);
    expect(state.playlist[0].id).toBe("bilibili-BV1xx411c7mD");
    expect(state.playlist[0].format).toBe("FLAC");
    expect(state.liked["bilibili-BV1xx411c7mD"]).toBe(true);
    expect(state.userPlaylists[0].trackIds).toEqual(["bilibili-BV1xx411c7mD"]);
  });

  it("defaults every quality option to false when none are passed", async () => {
    const reloaded = testTrack({
      id: "bilibili-BV1xx411c7mD",
      title: "老流媒体",
      album: "Bilibili",
      path: "C:/cache/BV1xx411c7mD-1.m4a",
      sourceId: "BV1xx411c7mD",
      sourceUrl: "https://www.bilibili.com/video/BV1xx411c7mD",
    });
    invokeMock.mockImplementation(async (cmd: string) =>
      cmd === "import_bilibili_audio_with_options" ? reloaded : undefined
    );

    await usePlayerStore.getState().reloadStreamingTrack("bilibili-BV1xx411c7mD");

    const call = invokeMock.mock.calls.find(
      ([cmd]) => cmd === "import_bilibili_audio_with_options"
    );
    expect(call?.[1]).toMatchObject({
      options: { preferFlac: false, preferDolbyAtmos: false, remuxWithFfmpeg: false },
    });
  });

  it("reports failure without touching the playlist when reimport throws", async () => {
    invokeMock.mockImplementation(async () => {
      throw new Error("403 forbidden");
    });

    const ok = await usePlayerStore.getState().reloadStreamingTrack(
      "bilibili-BV1xx411c7mD"
    );

    expect(ok).toBe(false);
    const state = usePlayerStore.getState();
    expect(state.playlist[0].format).toBe("M4A");
    expect(state.notification?.text ?? "").toContain("B 站");
  });
});
