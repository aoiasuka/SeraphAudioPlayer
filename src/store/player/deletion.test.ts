// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import type { DeleteTracksResult, Track } from "@/types/track";

vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn(async () => undefined),
}));

const invokeMock = vi.mocked(invoke);
const initialState = usePlayerStore.getInitialState();
const tracks: Track[] = ["a", "b", "c"].map((id) => ({
  id, title: id.toUpperCase(), artist: "测试", album: "测试", cover: "", format: "FLAC",
  bitdepth: "16-bit", bitrate: "", channels: "Stereo", size: "1 MB",
  path: `C:/test/${id}.flac`, duration: 180, glowColor: "#fff", lyrics: lyricDocument([]),
}));

function deleted(ids: string[]): DeleteTracksResult {
  return { deletedIds: ids, deletedFiles: 0, failures: [] };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function flush() {
  for (let index = 0; index < 20; index += 1) await Promise.resolve();
}

beforeEach(() => {
  vi.useFakeTimers();
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (command, args) =>
    command === "delete_tracks" ? deleted(args!.trackIds as string[]) : undefined
  );
  usePlayerStore.setState(initialState, true);
  usePlayerStore.setState({
    playlist: tracks, currentTrackIndex: 1, isPlaying: true, currentTime: 42,
    liked: { a: true, b: true }, recentTrackIds: ["b", "a"],
    userPlaylists: [{ id: "mix", name: "Mix", trackIds: ["a", "b", "c"], createdAt: 1 }],
  });
});

afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("批量删除状态同步", () => {
  it("单次 IPC 只传去重后的 ID，部分失败保留曲目及所有关联", async () => {
    usePlayerStore.setState({ currentTrackIndex: 2 });
    invokeMock.mockResolvedValue({
      deletedIds: ["a"], deletedFiles: 1,
      failures: [{ id: "b", title: "B", message: "文件被占用" }],
    });
    const result = await usePlayerStore.getState().deleteTracks(["a", "b", "a", "missing"]);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("delete_tracks", { trackIds: ["a", "b"] });
    expect(result.failures[0].id).toBe("b");
    const state = usePlayerStore.getState();
    expect(state.playlist.map((track) => track.id)).toEqual(["b", "c"]);
    expect(state.currentTrack()?.id).toBe("c");
    expect(state.currentTime).toBe(42);
    expect(state.isPlaying).toBe(true);
    expect(state.liked).toEqual({ b: true });
    expect(state.recentTrackIds).toEqual(["b"]);
    expect(state.userPlaylists[0].trackIds).toEqual(["b", "c"]);
    expect(state.notification?.text).toContain("1 首未删除");
  });

  it("删除当前曲目必须等待停止回执，全部删除后队列和引用清空", async () => {
    const stop = deferred<void>();
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "stop") return stop.promise;
      if (command === "delete_tracks") return deleted(args!.trackIds as string[]);
      return undefined;
    });
    const pending = usePlayerStore.getState().deleteTracks(["a", "b", "c"]);
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(["stop"]);
    stop.resolve(undefined);
    await pending;
    expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(["stop", "delete_tracks"]);
    const state = usePlayerStore.getState();
    expect(state.playlist).toEqual([]);
    expect(state.currentTrackIndex).toBe(0);
    expect(state.currentTrack()).toBeNull();
    expect(state.persistedCurrentTrackId).toBeNull();
    expect(state.currentTime).toBe(0);
    expect(state.isPlaying).toBe(false);
    expect(state.liked).toEqual({});
    expect(state.recentTrackIds).toEqual([]);
    expect(state.userPlaylists[0].trackIds).toEqual([]);
  });

  it("停止失败时不执行删除，返回可重试的失败原因", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    invokeMock.mockRejectedValue(new Error("无法释放音频文件"));
    const result = await usePlayerStore.getState().deleteTracks(["a", "b"]);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("stop");
    expect(result.deletedIds).toEqual([]);
    expect(result.failures.map((failure) => failure.id)).toEqual(["a", "b"]);
    expect(result.failures[0].message).toContain("无法释放音频文件");
    expect(usePlayerStore.getState().playlist).toEqual(tracks);
    expect(usePlayerStore.getState().liked).toEqual({ a: true, b: true });
  });

  it("桌面 IPC 缺失回执不能冒充删除成功", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    invokeMock.mockResolvedValue(undefined);
    const result = await usePlayerStore.getState().deleteTracks(["a"]);
    expect(result.deletedIds).toEqual([]);
    expect(result.failures[0].message).toContain("未收到有效的删除结果");
    expect(usePlayerStore.getState().playlist).toEqual(tracks);
  });

  it("等待删除期间切到未选曲目，不停止或重置新的播放", async () => {
    const deletion = deferred<DeleteTracksResult>();
    invokeMock.mockImplementation(async (command) => command === "delete_tracks" ? deletion.promise : undefined);
    const pending = usePlayerStore.getState().deleteTracks(["b"]);
    await flush();
    usePlayerStore.setState({ currentTrackIndex: 2, currentTime: 59, isPlaying: true });
    deletion.resolve(deleted(["b"]));
    await pending;
    const state = usePlayerStore.getState();
    expect(state.currentTrack()?.id).toBe("c");
    expect(state.currentTrackIndex).toBe(1);
    expect(state.currentTime).toBe(59);
    expect(state.isPlaying).toBe(true);
    expect(invokeMock.mock.calls.filter(([command]) => command === "stop")).toHaveLength(1);
  });

  it.each(["删除前", "删除期间"])("%s开始的曲库读取迟到时不能重新插入已删除曲目", async (when) => {
    const deletion = deferred<DeleteTracksResult>();
    const snapshot = deferred<Track[]>();
    invokeMock.mockImplementation(async (command) => {
      if (command === "delete_tracks") return deletion.promise;
      if (command === "get_playlist") return snapshot.promise;
      return undefined;
    });
    let load: Promise<void> | undefined;
    if (when === "删除前") load = usePlayerStore.getState().loadBackendLibrary();
    const pending = usePlayerStore.getState().deleteTracks(["a"]);
    if (when === "删除期间") load = usePlayerStore.getState().loadBackendLibrary();
    deletion.resolve(deleted(["a"]));
    await pending;
    snapshot.resolve(tracks);
    await load;
    expect(usePlayerStore.getState().playlist.map((track) => track.id)).toEqual(["b", "c"]);
  });

  it("空选择不调用后端", async () => {
    expect(await usePlayerStore.getState().deleteTracks([])).toEqual(deleted([]));
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
