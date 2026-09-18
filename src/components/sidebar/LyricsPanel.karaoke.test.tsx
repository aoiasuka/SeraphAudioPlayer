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

  it("当前行带 words 时逐字渲染，并显示译文与音译；后端 hidden 标记即时生效", () => {
    render(<LyricsPanel />);
    const karaoke = screen.getByTestId("karaoke-line");
    const words = karaoke.querySelectorAll(".karaoke-word");
    expect(words).toHaveLength(2);
    // currentTime=11：第一个音节唱到一半，第二个未开始
    expect(words[0]).toHaveAttribute("data-progress", "0.50");
    expect(words[1]).toHaveAttribute("data-progress", "0.00");
    // 回归：文字色必须是具体颜色，不能靠 currentColor（音节自身透明会连带整行不可见）
    expect((words[0] as HTMLElement).style.color).toBe("var(--ink)");
    expect((words[0] as HTMLElement).style.backgroundImage).not.toContain("currentColor");
    expect(screen.getByText("你好世界")).toBeInTheDocument();
    expect(screen.getByText("ha-ro")).toBeInTheDocument();
    expect(screen.getByText("作词：某人")).toBeInTheDocument();

    // 排除规则由后端打 hidden 标记：模拟后端回传后的形态
    act(() =>
      usePlayerStore.setState((state) => ({
        playlist: state.playlist.map((item) =>
          item.id === "w"
            ? { ...item, lyrics: item.lyrics.map((line) => line.text.startsWith("作词") ? { ...line, hidden: true } : line) }
            : item
        ),
        showLyricsRoman: false,
      }))
    );
    expect(screen.queryByText("作词：某人")).toBeNull();
    expect(screen.queryByText("ha-ro")).toBeNull();
    expect(screen.getByText("Plain line")).toBeInTheDocument();
  });

  it("逐字行唱完且距下一句尚远时当前句进入间奏淡出；行级歌词不受影响", () => {
    const { container } = render(<LyricsPanel />);
    // currentTime=11：正在唱，不是间奏
    expect(container.querySelector("[data-intermission]")).toBeNull();
    // 行 end=14，下一句 20 → 空档 6s；16s 起淡出
    act(() => usePlayerStore.setState({ currentTime: 16.5 }));
    const faded = container.querySelector<HTMLElement>("[data-intermission='true']");
    expect(faded).not.toBeNull();
    expect(faded!.textContent).toContain("Hello");
    expect(faded!.className).toContain("opacity-55");
    // 到下一句（无 end 的行级歌词）后不再判定
    act(() => usePlayerStore.setState({ currentTime: 40 }));
    expect(container.querySelector("[data-intermission]")).toBeNull();
    const plain = screen.getByText("Plain line").closest(".flex.items-start") as HTMLElement;
    expect(plain.className).toContain("opacity-100");
  });
});
