// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { seekGuard } from "@/store/player/playbackActions";
import type { Track } from "@/types/track";
import { usePlayerEvents } from "./usePlayerEvents";
import { usePlayback } from "./usePlayback";

vi.mock("./usePlayerEvents", () => ({ usePlayerEvents: vi.fn() }));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn(async () => undefined),
}));

function emit(event: { type: string; [key: string]: unknown }) {
  const handler = vi.mocked(usePlayerEvents).mock.lastCall?.[0];
  act(() => handler?.(event));
}

describe("切歌时的歌词时间轴", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seekGuard.until = 0;
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: ["a", "b"].map((id): Track => ({
        id, title: id, path: `C:/Music/${id}.flac`, duration: 180, lyrics: [],
        artist: "Artist", album: "Album", cover: "", format: "FLAC",
        bitdepth: "16-bit", bitrate: "Unknown", channels: "Stereo", size: "1 MB", glowColor: "#fff",
      })),
      currentTime: 90,
      recentTrackIds: ["a"],
    });
  });

  afterEach(cleanup);

  it("跳转失败保留播放态并立即回滚进度", () => {
    usePlayerStore.setState({ isPlaying: true, currentTime: 40 });
    renderHook(usePlayback);
    seekGuard.until = Date.now() + 400;
    seekGuard.target = 40;
    emit({ type: "seek_failed", track_id: "a", seconds: 8, message: "不可跳转" });
    expect(usePlayerStore.getState().isPlaying).toBe(true);
    expect(usePlayerStore.getState().currentTime).toBe(8);
    expect(usePlayerStore.getState().notification?.text).toBe("不可跳转");
    expect(seekGuard.until).toBe(0);
  });

  it("上一首的跳转失败不会回滚当前曲目", () => {
    usePlayerStore.setState({ isPlaying: true, currentTrackIndex: 1, currentTime: 12 });
    renderHook(usePlayback);
    emit({ type: "seek_failed", track_id: "a", seconds: 8, message: "旧错误" });
    expect(usePlayerStore.getState().currentTime).toBe(12);
    expect(usePlayerStore.getState().isPlaying).toBe(true);
    expect(usePlayerStore.getState().notification).toBeNull();
  });

  it("致命播放错误仍会停止界面播放态", () => {
    usePlayerStore.setState({ isPlaying: true });
    renderHook(usePlayback);
    emit({ type: "error", message: "解码失败" });
    expect(usePlayerStore.getState().isPlaying).toBe(false);
  });

  it("切歌立即归零并清除上一首的 seek 保护", () => {
    renderHook(usePlayback);
    seekGuard.until = Date.now() + 400;
    seekGuard.target = 90;
    emit({ type: "track_changed", track_id: "b" });
    expect(usePlayerStore.getState().currentTrack()?.id).toBe("b");
    expect(usePlayerStore.getState().currentTime).toBe(0);
    expect(seekGuard.until).toBe(0);
    emit({ type: "progress", track_id: "b", seconds: 0.2 });
    expect(usePlayerStore.getState().currentTime).toBe(0.2);
  });

  it("上一首迟到的进度不会让新歌词跳回开头", () => {
    renderHook(usePlayback);
    emit({ type: "track_changed", track_id: "b" });
    emit({ type: "progress", track_id: "b", seconds: 8 });
    emit({ type: "progress", track_id: "a", seconds: 179 });
    expect(usePlayerStore.getState().currentTime).toBe(8);
  });

  it("收到起播事件即可同步曲目，不依赖额外的切歌事件", () => {
    renderHook(usePlayback);
    emit({ type: "playback_started", track_id: "b" });
    const state = usePlayerStore.getState();
    expect(state.currentTrack()?.id).toBe("b");
    expect(state.currentTime).toBe(0);
    expect(state.isPlaying).toBe(true);
    expect(state.recentTrackIds).toEqual(["b", "a"]);
  });

  it("同一次切歌的起播事件不覆盖已经收到的新进度", () => {
    renderHook(usePlayback);
    emit({ type: "track_changed", track_id: "b" });
    emit({ type: "progress", track_id: "b", seconds: 0.3 });
    emit({ type: "playback_started", track_id: "b" });
    expect(usePlayerStore.getState().currentTime).toBe(0.3);
  });
});
