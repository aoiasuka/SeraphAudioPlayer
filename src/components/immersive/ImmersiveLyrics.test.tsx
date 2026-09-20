// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LyricsPanel } from "@/components/sidebar/LyricsPanel";
import { useImmersiveStore } from "@/store/immersive";
import { usePlayerStore } from "@/store/player";
import type { OnlineLyricsCandidate, Track } from "@/types/track";
import { ImmersivePlayer } from "./ImmersivePlayer";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => false,
  invoke: bridge.invoke,
}));

const track: Track = {
  id: "typing-a", title: "同一首旋律", artist: "演奏者", album: "歌词档案", path: "C:/Music/a.flac",
  cover: "", format: "FLAC", bitdepth: "24-bit", bitrate: "", channels: "Stereo", size: "",
  duration: 180, glowColor: "", lyricsLoaded: true,
  lyrics: lyricDocument([{ startMs: 10000, text: "First line" }, { startMs: 10000, text: "第一句译文" }, { startMs: 20000, text: "Second line" }]),
};
const candidate: OnlineLyricsCandidate = {
  id: "matched", source: "QQ 音乐", title: "搜索匹配结果", artist: "演奏者", duration: 180,
  lyrics: lyricDocument([{ startMs: 10000, text: "Matched line" }, { startMs: 10000, text: "匹配后的译文" }]),
};
// store 默认歌词设置 → fetch_online_lyrics 的 options
const lyricsOptions = expect.objectContaining({ sourcePriority: "auto", preferTraditional: false, ttmlEnabled: true });

function Harness() {
  return <><div data-testid="normal-mode" style={{ display: "none" }}><LyricsPanel /></div><ImmersivePlayer /></>;
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => window.setTimeout(() => callback(performance.now()), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener() {}, removeEventListener() {} }));
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  HTMLElement.prototype.scrollTo = vi.fn();
  bridge.invoke.mockReset();
  bridge.invoke.mockImplementation(async (command: string) => {
    if (command === "fetch_online_lyrics") return [candidate];
    if (command === "apply_online_lyrics") return candidate.lyrics;
    return undefined;
  });
  usePlayerStore.setState({ ...usePlayerStore.getInitialState(), playlist: [track, { ...track, id: "typing-b" }], currentTrackIndex: 0, currentTime: 15, isPlaying: true });
});

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe.each(["lyrics", "analysis"] as const)("沉浸 %s 模式的歌词", (mode) => {
  beforeEach(() => useImmersiveStore.setState({ isOpen: true, mode }));

  it("当前句与正常模式逐字同步，换句和切歌重新打字，字号与译文切换不中断", async () => {
    vi.useFakeTimers();
    await act(async () => { render(<Harness />); });
    const immersive = screen.getByRole("region", { name: "沉浸播放" });
    const normal = screen.getByTestId("normal-mode");
    const typed = () => immersive.querySelector(".immersive-lyric-typing .type-caret")?.textContent;
    const normalTyped = () => normal.querySelector(".type-caret")?.textContent;
    expect(typed()).toBe("");
    expect(typed()).toBe(normalTyped());
    expect(within(immersive).getByText("第一句译文")).toBeVisible();

    act(() => vi.advanceTimersByTime(160));
    expect(typed()).toBe("Fi");
    expect(typed()).toBe(normalTyped());
    fireEvent.click(within(immersive).getByRole("button", { name: "放大歌词字号" }));
    fireEvent.click(within(immersive).getByRole("button", { name: "显示译文" }));
    act(() => usePlayerStore.setState({ currentTime: 16 }));
    expect(typed()).toBe("Fi");

    act(() => usePlayerStore.setState({ currentTime: 21 }));
    expect(typed()).toBe("");
    expect(within(immersive).getByRole("button", { name: /First line/ })).toHaveTextContent("First line");
    act(() => vi.advanceTimersByTime(160));
    expect(typed()).toBe("Se");
    expect(typed()).toBe(normalTyped());
    act(() => vi.advanceTimersByTime(1000));
    expect(typed()).toBe("Second line");

    // 同时间戳、同文本的新曲目也必须重新开始动画。
    act(() => usePlayerStore.setState({ currentTrackIndex: 1 }));
    expect(typed()).toBe("");
    expect(typed()).toBe(normalTyped());
    act(() => vi.advanceTimersByTime(80));
    expect(typed()).toBe("S");
  });

  it("侧栏隐藏时匹配弹窗仍可见，复用自动搜索、手动搜索和应用歌词流程", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    const immersive = within(screen.getByRole("region", { name: "沉浸播放" }));
    const trigger = immersive.getByRole("button", { name: "在线匹配歌词" });
    await user.click(trigger);
    await screen.findByRole("heading", { name: "选择在线歌词" });
    const dialog = screen.getByRole("dialog");
    expect(dialog).toBeVisible();
    expect(screen.getByTestId("normal-mode")).not.toContainElement(dialog);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(bridge.invoke).toHaveBeenCalledWith("fetch_online_lyrics", { trackId: track.id, title: track.title, artist: track.artist, duration: 180, options: lyricsOptions });
    expect(usePlayerStore.getState().currentTrack()?.lyrics).toEqual(track.lyrics);

    const search = within(dialog).getByLabelText("手动搜索");
    await user.clear(search);
    await user.type(search, "  another song  ");
    await user.click(within(dialog).getByRole("button", { name: "搜索在线歌词" }));
    await screen.findByRole("heading", { name: "选择在线歌词" });
    expect(bridge.invoke).toHaveBeenLastCalledWith("fetch_online_lyrics", { trackId: track.id, title: "another song", artist: "", duration: 180, options: lyricsOptions });
    await user.click(within(dialog).getByRole("button", { name: "使用这份歌词" }));
    // 候选未携带 lookupKeys 时仍以空数组随请求发出（查找键回写由后端处理）
    expect(bridge.invoke).toHaveBeenLastCalledWith("apply_online_lyrics", { trackId: track.id, trackPath: track.path, lyrics: candidate.lyrics, lookupKeys: [] });
    expect(immersive.getByRole("button", { name: /Matched line/ })).toHaveAttribute("aria-current", "true");
    expect(usePlayerStore.getState()).toMatchObject({ currentTime: 15, isPlaying: true });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    await screen.findByRole("heading", { name: "选择在线歌词" });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(useImmersiveStore.getState().isOpen).toBe(true);
    expect(trigger).toHaveFocus();
  });

  it("匹配过程中切歌后仍阻止把旧候选歌词应用到新曲目", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await user.click(screen.getByRole("button", { name: "在线匹配歌词" }));
    await screen.findByRole("heading", { name: "选择在线歌词" });
    act(() => usePlayerStore.setState({ currentTrackIndex: 1 }));
    await user.click(screen.getByRole("button", { name: "使用这份歌词" }));
    expect(bridge.invoke.mock.calls.some(([command]) => command === "apply_online_lyrics")).toBe(false);
    expect(usePlayerStore.getState().currentTrack()?.lyrics).toEqual(track.lyrics);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(useImmersiveStore.getState().isOpen).toBe(true);
  });

  it("隐藏句区间不高亮任何行；歌词全部被规则隐藏时显示专用空状态", () => {
    usePlayerStore.setState({
      playlist: [{ ...track, lyrics: lyricDocument([{ startMs: 10000, text: "First line" }, { startMs: 20000, text: "作词：某人", hidden: true }, { startMs: 30000, text: "Third line" }]) }],
      currentTime: 25,
    });
    render(<Harness />);
    const immersive = within(screen.getByRole("region", { name: "沉浸播放" }));
    expect(immersive.queryByRole("button", { name: /作词/ })).not.toBeInTheDocument();
    expect(immersive.getByRole("button", { name: /First line/ })).not.toHaveAttribute("aria-current");
    expect(immersive.getByRole("button", { name: /Third line/ })).not.toHaveAttribute("aria-current");
    act(() => usePlayerStore.setState({ currentTime: 30 }));
    expect(immersive.getByRole("button", { name: /Third line/ })).toHaveAttribute("aria-current", "true");

    act(() => usePlayerStore.setState({ playlist: [{ ...track, lyrics: lyricDocument([{ startMs: 0, text: "作曲：某人", hidden: true }]) }] }));
    expect(immersive.getByText("歌词已被排除规则全部隐藏")).toBeInTheDocument();
    expect(immersive.queryByText("暂无歌词")).not.toBeInTheDocument();
    act(() => usePlayerStore.setState({ playlist: [{ ...track, lyrics: lyricDocument([]) }] }));
    expect(immersive.getByText("暂无歌词")).toBeInTheDocument();
  });
});
