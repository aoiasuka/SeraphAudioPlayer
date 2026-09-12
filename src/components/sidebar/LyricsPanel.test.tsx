// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { LyricsPanel } from "./LyricsPanel";

vi.mock("@/components/ui/TypewriterText", () => ({
  TypewriterText: ({ text }: { text: string }) => <span>{text}</span>,
}));

const tracks = [
  { id: "a", duration: 180, lyrics: [{ time: 0, text: "上一首开头" }, { time: 60, text: "上一首结尾" }] },
  { id: "b", duration: 180, lyrics: [{ time: 10, text: "新歌第一句" }, { time: 20, text: "新歌第二句" }] },
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
      playlist: [{ ...tracks[0], lyrics: [{ time: 0, text: "替换后的开头" }, { time: 60, text: "替换后的当前行" }] }, tracks[1]],
    }));
    expect(scrollTo).toHaveBeenLastCalledWith({ top: 0, behavior: "instant" });
  });
});
