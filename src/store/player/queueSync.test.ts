// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { syncPlaybackModes, syncPlaybackQueue } from "./queueSync";
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
});
