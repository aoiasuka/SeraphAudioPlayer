// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { lyricDocument } from "@/lib/lyrics/document";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { LyricsPanel } from "./LyricsPanel";

vi.mock("@/components/ui/TypewriterText", () => ({
  TypewriterText: ({ text }: { text: string }) => <span>{text}</span>,
}));

const tracks = [
  { id: "a", duration: 180, lyrics: lyricDocument([{ startMs: 0, text: "上一首开头" }, { startMs: 60000, text: "上一首结尾" }]) },
  { id: "b", duration: 180, lyrics: lyricDocument([{ startMs: 10000, text: "新歌第一句" }, { startMs: 20000, text: "新歌第二句" }]) },
] as Track[];

describe("歌词滚动跟随", () => {
  const scrollTo = vi.fn(function (this: HTMLElement, options?: ScrollToOptions | number, y?: number) {
    this.scrollTop = typeof options === "number" ? y ?? 0 : options?.top ?? 0;
  });

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("ResizeObserver", class {
      observe() {}
      disconnect() {}
    });
    Element.prototype.scrollTo = scrollTo;
    scrollTo.mockClear();
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: tracks,
      currentTime: 70,
    });
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("手动滚动后切到有前奏的新歌，立即回到开头", () => {
    const { container } = render(<LyricsPanel />);
    const scroller = container.querySelector<HTMLElement>(".no-scrollbar")!;
    scroller.scrollTop = 600;
    fireEvent.wheel(scroller);
    scrollTo.mockClear();
    act(() => usePlayerStore.setState({ currentTrackIndex: 1, currentTime: 0 }));
    expect(scroller.scrollTop).toBe(0);
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, behavior: "instant" });
    expect(screen.queryByText("上一首结尾")).toBeNull();
    expect(screen.getByText("新歌第一句")).toBeTruthy();
  });

  it("切歌时当前行索引相同也立即定位，不等待滚动节流", () => {
    const { container } = render(<LyricsPanel />);
    fireEvent.wheel(container.querySelector(".no-scrollbar")!);
    scrollTo.mockClear();
    act(() => usePlayerStore.setState({ currentTrackIndex: 1, currentTime: 25 }));
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, behavior: "instant" });
  });

  it("同一首歌仍尊重手动浏览，恢复跟随时不额外延迟 200ms", () => {
    usePlayerStore.setState({ currentTime: 0 });
    const { container } = render(<LyricsPanel />);
    fireEvent.wheel(container.querySelector(".no-scrollbar")!);
    scrollTo.mockClear();
    act(() => usePlayerStore.setState({ currentTime: 70 }));
    expect(scrollTo).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(3001));
    act(() => usePlayerStore.setState({ currentTime: 0 }));
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, behavior: "smooth" });
  });

  it("给当前歌曲替换歌词后立即重新定位", () => {
    render(<LyricsPanel />);
    scrollTo.mockClear();
    act(() => usePlayerStore.setState({
      playlist: [{ ...tracks[0], lyrics: lyricDocument([{ startMs: 0, text: "替换后的开头" }, { startMs: 60000, text: "替换后的当前行" }]) }, tracks[1]],
    }));
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, behavior: "instant" });
  });
});

describe("排除规则隐藏行", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class {
      observe() {}
      disconnect() {}
    });
    Element.prototype.scrollTo = vi.fn();
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("隐藏句的时间区间不并入上一句：该区间内不高亮任何行，下一句到点正常高亮", () => {
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: [{
        id: "h", duration: 180,
        lyrics: lyricDocument([
          { startMs: 0, text: "第一句" },
          { startMs: 10000, text: "作词：某人", hidden: true },
          { startMs: 20000, text: "第三句" },
        ]),
      }] as Track[],
      currentTime: 5,
    });
    render(<LyricsPanel />);
    expect(screen.queryByText("作词：某人")).toBeNull();
    const first = () => screen.getByText("第一句").closest(".origin-left")!;
    const third = () => screen.getByText("第三句").closest(".origin-left")!;
    expect(first().className).toContain("opacity-100");

    act(() => usePlayerStore.setState({ currentTime: 15 }));
    expect(first().className).toContain("opacity-40");
    expect(third().className).toContain("opacity-40");

    act(() => usePlayerStore.setState({ currentTime: 20 }));
    expect(third().className).toContain("opacity-100");
  });

  it("歌词全部被隐藏时显示专用空状态，按钮打开设置并派发切标签事件", () => {
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: [{
        id: "all-hidden", duration: 180,
        lyrics: lyricDocument([{ startMs: 0, text: "作词：某人", hidden: true }, { startMs: 5000, text: "作曲：某人", hidden: true }]),
      }] as Track[],
      settingsOpen: false,
    });
    const onOpenTab = vi.fn();
    window.addEventListener("seraph:open-settings-tab", onOpenTab);
    render(<LyricsPanel />);
    expect(screen.getByText("歌词已被排除规则全部隐藏")).toBeTruthy();
    expect(screen.queryByText("暂无歌词稿")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "打开歌词设置" }));
    expect(usePlayerStore.getState().settingsOpen).toBe(true);
    expect(onOpenTab).toHaveBeenCalledTimes(1);
    expect((onOpenTab.mock.calls[0][0] as CustomEvent).detail).toBe("lyrics");
    window.removeEventListener("seraph:open-settings-tab", onOpenTab);

    // 真正没有歌词时仍是原文案
    act(() => usePlayerStore.setState({ playlist: [{ id: "none", duration: 180, lyrics: lyricDocument([]) }] as Track[] }));
    expect(screen.getByText("暂无歌词稿")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "打开歌词设置" })).toBeNull();
  });
});
