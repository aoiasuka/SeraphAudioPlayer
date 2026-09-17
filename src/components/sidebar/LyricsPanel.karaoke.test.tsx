// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { LyricsPanel } from "./LyricsPanel";

vi.mock("@/components/ui/TypewriterText", () => ({
  TypewriterText: ({ text }: { text: string }) => <span>{text}</span>,
}));

const track = {
  id: "w",
  duration: 100,
  lyrics: [
    { time: 0, text: "作词：某人" },
    {
      time: 10,
      end: 14,
      text: "Hello world",
      translation: "你好世界",
      roman: "ha-ro",
      words: [
        { start: 10, end: 12, text: "Hello " },
        { start: 12, end: 14, text: "world" },
      ],
    },
    { time: 20, text: "Plain line" },
  ],
} as Track;

describe("歌词稿：排除规则与逐字/译文/音译", () => {
  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    vi.stubGlobal("requestAnimationFrame", () => 0);
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    Element.prototype.scrollTo = vi.fn();
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: [track],
      currentTime: 11,
      isPlaying: false,
      lyricsExcludeRules: [],
      showLyricsTranslation: true,
      showLyricsRoman: true,
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("当前行带 words 时逐字渲染，并显示译文与音译；排除规则即时生效", () => {
    render(<LyricsPanel />);
    const karaoke = screen.getByTestId("karaoke-line");
    const words = karaoke.querySelectorAll(".karaoke-word");
    expect(words).toHaveLength(2);
    // currentTime=11：第一个音节唱到一半，第二个未开始
    expect(words[0]).toHaveAttribute("data-progress", "0.50");
    expect(words[1]).toHaveAttribute("data-progress", "0.00");
    expect(screen.getByText("你好世界")).toBeInTheDocument();
    expect(screen.getByText("ha-ro")).toBeInTheDocument();
    expect(screen.getByText("作词：某人")).toBeInTheDocument();

    act(() =>
      usePlayerStore.setState({
        lyricsExcludeRules: [{ id: "k", kind: "keyword", pattern: "作词" }],
        showLyricsRoman: false,
      })
    );
    expect(screen.queryByText("作词：某人")).toBeNull();
    expect(screen.queryByText("ha-ro")).toBeNull();
    expect(screen.getByText("Plain line")).toBeInTheDocument();
  });
});
