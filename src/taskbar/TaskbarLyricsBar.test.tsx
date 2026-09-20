// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { lyricDocument } from "@/lib/lyrics/document";
import { afterEach, beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { FRONTEND_EVENT, invoke } from "@/lib/tauri";
import { TaskbarLyricsBar } from "./TaskbarLyricsBar";

const { handlers } = vi.hoisted(() => ({
  handlers: new Map<string, (event: unknown) => void>(),
}));

vi.mock("@/components/ui/TypewriterText", () => ({
  TypewriterText: ({ text }: { text: string }) => <span>{text}</span>,
}));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  invoke: vi.fn(),
  listen: vi.fn(async (name: string, callback: (event: unknown) => void) => {
    handlers.set(name, callback);
    return () => { handlers.delete(name); };
  }),
}));

const invokeMock = invoke as Mock;
const trackA = { id: "a", title: "A", artist: "", cover: "", duration: 180, lyrics: lyricDocument([{ startMs: 0, text: "旧歌歌词" }]) };
const trackB = { id: "b", title: "B", artist: "", cover: "", duration: 180, lyrics: lyricDocument([{ startMs: 0, text: "新歌歌词" }]) };
const snapshotA = { trackId: "a", playing: true, seconds: 40, total: 180, darkTaskbar: false };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function emit(event: { type: string; [key: string]: unknown }) {
  act(() => handlers.get(FRONTEND_EVENT)?.(event));
}

describe("任务栏歌词切歌", () => {
  beforeEach(() => {
    handlers.clear();
    invokeMock.mockReset();
    localStorage.clear();
  });

  afterEach(cleanup);

  it("新歌数据加载期间立即隐藏旧歌词，旧进度也不会切回上一首", async () => {
    const nextInfo = deferred<typeof trackB>();
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "get_playback_snapshot") return snapshotA;
      if (command === "get_track_info") return args.trackId === "a" ? trackA : nextInfo.promise;
    });
    render(<TaskbarLyricsBar />);
    await screen.findByText("旧歌歌词");
    emit({ type: "track_changed", track_id: "b" });
    expect(screen.queryByText("旧歌歌词")).toBeNull();
    emit({ type: "progress", track_id: "a", seconds: 179, total: 180 });
    await act(async () => { nextInfo.resolve(trackB); });
    expect(screen.getByText("新歌歌词")).toBeTruthy();
    expect(screen.queryByText("旧歌歌词")).toBeNull();
  });

  it("连续切歌时不接收上一首迟到的歌词数据", async () => {
    const oldInfo = deferred<typeof trackA>();
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "get_playback_snapshot") return snapshotA;
      if (command === "get_track_info") return args.trackId === "a" ? oldInfo.promise : trackB;
    });
    render(<TaskbarLyricsBar />);
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_track_info", { trackId: "a" }));
    emit({ type: "track_changed", track_id: "b" });
    await screen.findByText("新歌歌词");
    await act(async () => { oldInfo.resolve(trackA); });
    expect(screen.getByText("新歌歌词")).toBeTruthy();
    expect(screen.queryByText("旧歌歌词")).toBeNull();
  });

  it("迟到的启动快照不会覆盖实时切歌事件", async () => {
    const oldSnapshot = deferred<typeof snapshotA>();
    invokeMock.mockImplementation(async (command) => {
      if (command === "get_playback_snapshot") return oldSnapshot.promise;
      if (command === "get_track_info") return trackB;
    });
    render(<TaskbarLyricsBar />);
    emit({ type: "playback_started", track_id: "b" });
    await screen.findByText("新歌歌词");
    await act(async () => { oldSnapshot.resolve(snapshotA); });
    expect(screen.getByText("新歌歌词")).toBeTruthy();
    expect(invokeMock).not.toHaveBeenCalledWith("get_track_info", { trackId: "a" });
  });

  it("进度先于启动快照到达时，仍从同曲目快照补齐播放状态", async () => {
    const snapshot = deferred<typeof snapshotA>();
    invokeMock.mockImplementation(async (command) => {
      if (command === "get_playback_snapshot") return snapshot.promise;
      if (command === "get_track_info") return trackA;
    });
    const { container } = render(<TaskbarLyricsBar />);
    emit({ type: "progress", track_id: "a", seconds: 42, total: 180 });
    await screen.findByText("旧歌歌词");
    await act(async () => { snapshot.resolve(snapshotA); });
    fireEvent.mouseEnter(container.firstElementChild!);
    expect(screen.getByRole("button", { name: "暂停" })).toBeTruthy();
  });

  it("暂停事件先到时，快照仍能补齐歌词而不覆盖暂停状态", async () => {
    const snapshot = deferred<typeof snapshotA>();
    invokeMock.mockImplementation(async (command) => {
      if (command === "get_playback_snapshot") return snapshot.promise;
      if (command === "get_track_info") return trackA;
    });
    const { container } = render(<TaskbarLyricsBar />);
    emit({ type: "playback_paused" });
    await act(async () => { snapshot.resolve(snapshotA); });
    await screen.findByText("旧歌歌词");
    fireEvent.mouseEnter(container.firstElementChild!);
    expect(screen.getByRole("button", { name: "播放" })).toBeTruthy();
  });
});

describe("任务栏歌词排除规则", () => {
  beforeEach(() => {
    handlers.clear();
    invokeMock.mockReset();
    localStorage.clear();
  });

  afterEach(cleanup);

  it("隐藏句区间不延长上一句，显示占位；全部隐藏时显示专用文案", async () => {
    const withHidden = {
      ...trackA,
      lyrics: lyricDocument([
        { startMs: 0, text: "第一句" },
        { startMs: 10000, text: "作词：某人", hidden: true },
        { startMs: 20000, text: "第三句" },
      ]),
    };
    const allHidden = { ...trackB, lyrics: lyricDocument([{ startMs: 0, text: "作曲：某人", hidden: true }]) };
    invokeMock.mockImplementation(async (command, args) => {
      if (command === "get_playback_snapshot") return { ...snapshotA, seconds: 5 };
      if (command === "get_track_info") return args.trackId === "a" ? withHidden : allHidden;
    });
    render(<TaskbarLyricsBar />);
    await screen.findByText("第一句");

    emit({ type: "progress", track_id: "a", seconds: 15, total: 180 });
    expect(screen.queryByText("第一句")).toBeNull();
    expect(screen.queryByText("作词：某人")).toBeNull();
    expect(screen.getByText("— 暂无歌词稿 —")).toBeTruthy();

    emit({ type: "progress", track_id: "a", seconds: 20, total: 180 });
    expect(screen.getByText("第三句")).toBeTruthy();

    emit({ type: "track_changed", track_id: "b" });
    expect(await screen.findByText("— 歌词已被排除规则隐藏 —")).toBeTruthy();
    expect(screen.queryByText("— 暂无歌词稿 —")).toBeNull();
  });
});
