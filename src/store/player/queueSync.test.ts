// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { resetPlaybackQueueSync, syncPlaybackModes, syncPlaybackQueue } from "./queueSync";
import type { PlaybackQueuePreview } from "./types";

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn(),
}));

const tracks: Track[] = ["a", "b", "c"].map((id) => ({
  id, title: id, path: `C:/Music/${id}.flac`, duration: 180, lyrics: [],
  artist: "Artist", album: "Album", cover: "", format: "FLAC",
  bitdepth: "16-bit", bitrate: "Unknown", channels: "Stereo", size: "1 MB", glowColor: "#fff",
}));
const get = usePlayerStore.getState;
const set = usePlayerStore.setState;
const invokeMock = vi.mocked(invoke);
const preview: PlaybackQueuePreview = {
  currentTrackId: "a", nextTrackId: "c", shuffleMode: true,
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

describe("后端下一首预览", () => {
  beforeEach(() => {
    resetPlaybackQueueSync();
    vi.restoreAllMocks();
    invokeMock.mockReset().mockResolvedValue(preview);
    set({
      ...usePlayerStore.getInitialState(),
      playlist: tracks,
      recentTrackIds: ["a"],
      shuffleMode: true,
    });
  });

  it("显示后端保留的曲目，前端不再独立随机", async () => {
    expect(get().nextTrackPreview()).toBeNull();
    await syncPlaybackQueue(get, set);
    const random = vi.spyOn(Math, "random");
    expect(get().nextTrackPreview()?.id).toBe("c");
    expect(get().nextTrackPreview()?.id).toBe("c");
    expect(random).not.toHaveBeenCalled();
  });

  it.each(["nextTrack", "playNextPreview"] as const)("%s 使用后端同一切歌入口", async (action) => {
    await syncPlaybackQueue(get, set);
    const loadTrack = vi.fn();
    set({ loadTrack });
    get()[action]();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith("next_track", undefined));
    expect(get().nextTrackPreview()?.id).toBe("c");
    expect(loadTrack).not.toHaveBeenCalled();
  });

  it("切歌后不显示上一首的预览，且忽略迟到的同步结果", async () => {
    const old = deferred<PlaybackQueuePreview>();
    invokeMock.mockReturnValueOnce(old.promise);
    const pending = syncPlaybackQueue(get, set);
    set({ currentTrackIndex: 1, recentTrackIds: ["b", "a"] });
    const current = { currentTrackId: "b", nextTrackId: "a", shuffleMode: true };
    invokeMock.mockResolvedValueOnce(current);
    await syncPlaybackQueue(get, set);
    old.resolve(preview);
    await pending;
    expect(get().playbackQueuePreview).toEqual(current);
    expect(get().nextTrackPreview()?.id).toBe("a");
  });

  it("请求期间切歌，即使尚未发起新同步，也丢弃旧结果", async () => {
    const old = deferred<PlaybackQueuePreview>();
    invokeMock.mockReturnValueOnce(old.promise);
    const pending = syncPlaybackQueue(get, set);
    set({ currentTrackIndex: 1 });
    old.resolve(preview);
    await pending;
    expect(get().nextTrackPreview()).toBeNull();
  });

  it("切换播放模式后只显示新模式对应的后端预览", async () => {
    await syncPlaybackQueue(get, set);
    set({ shuffleMode: false });
    expect(get().nextTrackPreview()).toBeNull();
    invokeMock.mockResolvedValueOnce({ ...preview, nextTrackId: "b", shuffleMode: false });
    await syncPlaybackModes(get, set);
    expect(get().nextTrackPreview()?.id).toBe("b");
  });

  it("同一快照合并在途请求，确认后不再重复同步", async () => {
    const response = deferred<PlaybackQueuePreview>();
    invokeMock.mockReturnValueOnce(response.promise);
    const first = syncPlaybackQueue(get, set);
    const second = syncPlaybackQueue(get, set);
    expect(second).toBe(first);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    response.resolve(preview);
    await first;
    await syncPlaybackQueue(get, set);
    expect(invokeMock).toHaveBeenCalledTimes(1);
  });

  it("切歌只同步选择，曲库内容变化才发送 tracks", async () => {
    await syncPlaybackQueue(get, set);
    const initial = invokeMock.mock.calls[0][1]!;
    expect(initial.tracks).toHaveLength(3);
    set({ currentTrackIndex: 1, recentTrackIds: ["b", "a"] });
    await syncPlaybackQueue(get, set);
    expect(invokeMock.mock.calls[1][1]).not.toHaveProperty("tracks");
    expect(invokeMock.mock.calls[1][1]).toMatchObject({ currentTrackIndex: 1 });
    set({ playlist: tracks.slice(1) });
    await syncPlaybackQueue(get, set);
    expect(invokeMock.mock.calls[2][1]?.tracks).toHaveLength(2);
  });

  it("后端版本失配时恢复完整快照，其他错误不盲目重试", async () => {
    await syncPlaybackQueue(get, set);
    set({ loopMode: true });
    invokeMock.mockRejectedValueOnce("queue_revision_mismatch");
    await syncPlaybackQueue(get, set);
    expect(invokeMock.mock.calls[1][1]).not.toHaveProperty("tracks");
    expect(invokeMock.mock.calls[2][1]?.tracks).toHaveLength(3);
    set({ loopMode: false });
    invokeMock.mockRejectedValueOnce("unavailable");
    await expect(syncPlaybackQueue(get, set)).rejects.toBe("unavailable");
    expect(invokeMock).toHaveBeenCalledTimes(4);
  });

  it("按需补入歌词不会触发整库重新同步，元数据修改仍会发送完整内容", async () => {
    await syncPlaybackQueue(get, set);
    const initial = invokeMock.mock.calls[0][1]!;
    set({ playlist: tracks.map((track, index) => index === 0
      ? { ...track, lyrics: [{ time: 1, text: "已加载歌词" }], lyricsLoaded: true } : track) });
    await syncPlaybackQueue(get, set);
    expect(invokeMock.mock.calls[1][1]).not.toHaveProperty("tracks");
    expect(invokeMock.mock.calls[1][1]?.sync).toMatchObject({ revision: (initial.sync as { revision: string }).revision });
    set({ playlist: get().playlist.map((track, index) => index === 0 ? { ...track, title: "新标题" } : track) });
    await syncPlaybackQueue(get, set);
    expect(invokeMock.mock.calls[2][1]?.tracks).toHaveLength(3);
  });

  it("A→B→A 的新请求不能复用已经过期的 A 请求", async () => {
    const a = deferred<PlaybackQueuePreview>();
    const b = deferred<PlaybackQueuePreview>();
    invokeMock.mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
    const first = syncPlaybackQueue(get, set);
    set({ currentTrackIndex: 1 });
    const second = syncPlaybackQueue(get, set);
    set({ currentTrackIndex: 0 });
    await syncPlaybackQueue(get, set);
    expect(invokeMock).toHaveBeenCalledTimes(3);
    b.resolve({ ...preview, currentTrackId: "b" });
    a.resolve({ ...preview, nextTrackId: "b" });
    await Promise.all([first, second]);
    expect(get().playbackQueuePreview).toEqual(preview);
  });
});
