// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MiniPlayer } from "@/components/pages/main-pages/MiniPlayer";
import { useSpacePlayback } from "@/hooks/useSpacePlayback";
import { useImmersiveStore } from "@/store/immersive";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { ImmersivePlayer } from "./ImmersivePlayer";

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => false,
  invoke: vi.fn(async () => undefined),
}));

const track: Track = {
  id: "immersive-a", title: "档案中的旋律", artist: "演奏者", album: "纸上的回声",
  path: "C:/Music/a.flac", cover: "data:image/png;base64,AAAA", format: "FLAC",
  bitdepth: "FLAC 24-bit / 48 kHz", bitrate: "", channels: "Stereo", size: "", duration: 180,
  glowColor: "", lyricsLoaded: true,
  lyrics: [{ time: 10, text: "First line" }, { time: 10, text: "第一句译文" }, { time: 20, text: "Second line" }, { time: 20, text: "第二句译文" }],
};

function PlayerHarness() {
  const open = useImmersiveStore((s) => s.isOpen);
  useSpacePlayback();
  return <><div style={{ display: open ? "none" : undefined }}><MiniPlayer /></div>{open && <ImmersivePlayer />}</>;
}

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => window.setTimeout(() => callback(performance.now()), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: true, addEventListener() {}, removeEventListener() {} }));
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  HTMLElement.prototype.scrollTo = vi.fn();
  HTMLElement.prototype.setPointerCapture = vi.fn();
  useImmersiveStore.setState({ isOpen: false, mode: "lyrics" });
  usePlayerStore.setState({ playlist: [track, { ...track, id: "immersive-b", title: "下一张唱片" }], currentTrackIndex: 0, currentTime: 15, isPlaying: true, liked: {}, shuffleMode: false, loopMode: false, showLyricsTranslation: true });
});

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("沉浸播放的会话与交互", () => {
  it("从底部封面进入，Esc 返回封面焦点，保持歌曲、播放状态与进度", async () => {
    const user = userEvent.setup();
    render(<PlayerHarness />);
    const trigger = screen.getByRole("button", { name: "打开沉浸播放" });
    await user.click(trigger);
    expect(screen.getByRole("region", { name: "沉浸播放" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "收起沉浸播放" })).toHaveFocus();
    expect(usePlayerStore.getState()).toMatchObject({ currentTrackIndex: 0, currentTime: 15, isPlaying: true });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("region", { name: "沉浸播放" })).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
    expect(usePlayerStore.getState()).toMatchObject({ currentTrackIndex: 0, currentTime: 15, isPlaying: true });
  });

  it("模式支持键盘切换；按钮空格激活不会误触发播放暂停", async () => {
    const user = userEvent.setup();
    render(<PlayerHarness />);
    screen.getByRole("button", { name: "打开沉浸播放" }).focus();
    await user.keyboard(" ");
    const lyricsTab = screen.getByRole("tab", { name: "封面 · 歌词" });
    lyricsTab.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "声学分析" })).toHaveFocus();
    expect(await screen.findByRole("button", { name: "冻结画面" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "冻结画面" }));
    expect(screen.getByRole("button", { name: "继续分析" })).toHaveAttribute("aria-pressed", "true");
    expect(usePlayerStore.getState()).toMatchObject({ currentTime: 15, isPlaying: true });
    lyricsTab.focus();
    await user.keyboard(" ");
    expect(lyricsTab).toHaveAttribute("aria-selected", "true");
    expect(usePlayerStore.getState().isPlaying).toBe(true);
  });

  it("按时间分组原文和译文，点击歌词定位；切换字号与译文不改变播放", async () => {
    const user = userEvent.setup();
    useImmersiveStore.getState().open();
    render(<PlayerHarness />);
    expect(screen.getByRole("button", { name: /First line/ })).toHaveAttribute("aria-current", "true");
    await user.click(screen.getByRole("button", { name: "显示译文" }));
    expect(screen.queryByText("第一句译文")).not.toBeInTheDocument();
    // 译文开关写入 store（与设置页「显示译文」共用）
    expect(usePlayerStore.getState().showLyricsTranslation).toBe(false);
    await user.click(screen.getByRole("button", { name: "放大歌词字号" }));
    expect(screen.getByRole("button", { name: "放大歌词字号" })).toHaveAttribute("aria-pressed", "true");
    expect(usePlayerStore.getState().currentTime).toBe(15);
    await user.click(screen.getByRole("button", { name: /Second line/ }));
    expect(usePlayerStore.getState().currentTime).toBe(20);
    expect(screen.getByRole("button", { name: /Second line/ })).toHaveAttribute("aria-current", "true");
  });

  it("译文开关读写 store：设置页关掉译文后沉浸页同步隐藏，沉浸页再开回写 store", async () => {
    const user = userEvent.setup();
    useImmersiveStore.getState().open();
    render(<PlayerHarness />);
    expect(screen.getByText("第一句译文")).toBeInTheDocument();
    act(() => usePlayerStore.getState().setShowLyricsTranslation(false));
    expect(screen.queryByText("第一句译文")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "显示译文" })).toHaveAttribute("aria-pressed", "false");
    await user.click(screen.getByRole("button", { name: "显示译文" }));
    expect(usePlayerStore.getState().showLyricsTranslation).toBe(true);
    expect(screen.getByText("第一句译文")).toBeInTheDocument();
  });

  it("进度拖动只在松开时定位；取消拖动保持原进度，键盘输入直接定位", () => {
    useImmersiveStore.getState().open();
    render(<PlayerHarness />);
    const slider = screen.getByRole("slider", { name: "播放进度" });
    fireEvent.pointerDown(slider, { pointerId: 1 });
    fireEvent.change(slider, { target: { value: "60" } });
    expect(usePlayerStore.getState().currentTime).toBe(15);
    fireEvent.pointerCancel(slider, { pointerId: 1 });
    expect(slider).toHaveValue("15");
    fireEvent.pointerDown(slider, { pointerId: 2 });
    fireEvent.change(slider, { target: { value: "80" } });
    fireEvent.pointerUp(slider, { pointerId: 2 });
    expect(usePlayerStore.getState().currentTime).toBe(80);
    fireEvent.change(slider, { target: { value: "90" } });
    expect(usePlayerStore.getState().currentTime).toBe(90);
  });

  it("声学分析侧栏显示完整同步歌词，支持定位并共享译文与字号偏好", async () => {
    const user = userEvent.setup();
    useImmersiveStore.getState().open();
    render(<PlayerHarness />);
    await user.click(screen.getByRole("button", { name: "显示译文" }));
    await user.click(screen.getByRole("button", { name: "放大歌词字号" }));
    await user.click(screen.getByRole("tab", { name: "声学分析" }));

    const sidebar = within(screen.getByRole("complementary", { name: "当前曲目" }));
    const lyrics = within(sidebar.getByRole("region", { name: "同步歌词" }));
    expect(lyrics.getByRole("button", { name: /First line/ })).toHaveAttribute("aria-current", "true");
    expect(lyrics.getByRole("button", { name: /Second line/ })).toBeInTheDocument();
    expect(lyrics.queryByText("第一句译文")).not.toBeInTheDocument();
    expect(lyrics.getByRole("button", { name: "放大歌词字号" })).toHaveAttribute("aria-pressed", "true");

    await user.click(lyrics.getByRole("button", { name: /Second line/ }));
    expect(usePlayerStore.getState()).toMatchObject({ currentTime: 20, isPlaying: true });
    expect(lyrics.getByRole("button", { name: /Second line/ })).toHaveAttribute("aria-current", "true");
    act(() => usePlayerStore.setState({ currentTime: 12 }));
    expect(lyrics.getByRole("button", { name: /First line/ })).toHaveAttribute("aria-current", "true");

    await user.click(lyrics.getByRole("button", { name: "显示译文" }));
    await user.click(screen.getByRole("tab", { name: "封面 · 歌词" }));
    expect(screen.getByText("第一句译文")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "放大歌词字号" })).toHaveAttribute("aria-pressed", "true");
    expect(usePlayerStore.getState()).toMatchObject({ currentTime: 12, isPlaying: true });
  });

  it("队列检索后仍按原队列定位，Esc 先关闭队列并恢复焦点", async () => {
    const user = userEvent.setup();
    useImmersiveStore.getState().open();
    usePlayerStore.setState({ isPlaying: false });
    render(<PlayerHarness />);
    const trigger = screen.getByRole("button", { name: "打开播放队列" });
    await user.click(trigger);
    const dialog = await screen.findByRole("dialog");
    await user.type(within(dialog).getByRole("textbox", { name: "检索播放队列" }), "下一张");
    await user.click(within(dialog).getByRole("button", { name: /下一张唱片/ }));
    expect(usePlayerStore.getState().currentTrackIndex).toBe(1);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(useImmersiveStore.getState().isOpen).toBe(true);
    expect(trigger).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(useImmersiveStore.getState().isOpen).toBe(false);
  });

  it("封面失败、歌词加载及无歌词各有回退；未知时长禁用定位", () => {
    useImmersiveStore.getState().open();
    usePlayerStore.setState({ playlist: [{ ...track, lyrics: [], lyricsLoaded: false, duration: 0 }] });
    render(<PlayerHarness />);
    fireEvent.error(screen.getByRole("img", { name: /专辑封面/ }));
    expect(screen.getByRole("img", { name: "暂无专辑封面" })).toBeInTheDocument();
    expect(screen.getByText("正在读取歌词…")).toBeInTheDocument();
    expect(screen.getByRole("slider", { name: "播放进度" })).toBeDisabled();
    expect(within(screen.getByRole("region", { name: "沉浸播放" })).getByText("00:15")).toBeInTheDocument();
    act(() => usePlayerStore.setState({ playlist: [{ ...track, lyrics: [] }] }));
    expect(screen.getByText("暂无歌词")).toBeInTheDocument();
  });
});
