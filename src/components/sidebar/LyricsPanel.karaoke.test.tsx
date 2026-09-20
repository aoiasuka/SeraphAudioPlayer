// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { lyricDocument } from "@/lib/lyrics/document";
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
  lyrics: lyricDocument([
    { startMs: 0, text: "作词：某人" },
    {
      startMs: 10000,
      endMs: 14000,
      text: "Hello world",
      translations: [{ text: "你好世界" }],
      roman: { text: "ha-ro" },
      words: [
        { startMs: 10000, endMs: 12000, text: "Hello " },
        { startMs: 12000, endMs: 14000, text: "world" },
      ],
    },
    { startMs: 20000, text: "Plain line" },
  ]),
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
            ? { ...item, lyrics: { ...item.lyrics, lines: item.lyrics.lines.map((line) => line.text.startsWith("作词") ? { ...line, hidden: true } : line) } }
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

  it("逐字进度按引擎输出延迟回拨：按可听位置而不是引擎位置填色", () => {
    // 引擎位置 11s、输出延迟 0.5s → 可听位置 10.5s：首音节（10–12s）只唱了四分之一
    usePlayerStore.setState({ currentTime: 11, outputLatency: 0.5 });
    render(<LyricsPanel />);
    const karaoke = screen.getByTestId("karaoke-line");
    expect(karaoke.querySelectorAll(".karaoke-word")[0]).toHaveAttribute("data-progress", "0.25");
    // 延迟归零后恢复到 11s 的进度（首音节唱到一半）
    act(() => usePlayerStore.setState({ outputLatency: 0 }));
    expect(karaoke.querySelectorAll(".karaoke-word")[0]).toHaveAttribute("data-progress", "0.50");
    // 延迟大到可听位置退回上一句（9.5s < 10s）：当前行切回行级歌词，不再有逐字节点
    act(() => usePlayerStore.setState({ outputLatency: 1.5 }));
    expect(screen.queryByTestId("karaoke-line")).toBeNull();
  });
});

describe("歌词稿：角色渲染与多活动区间（B2）", () => {
  const duet = {
    id: "duet",
    duration: 100,
    lyrics: lyricDocument([
      { startMs: 0, text: "作词：某人", role: "credit" },
      { startMs: 0, text: "作曲：某人", role: "credit" },
      {
        startMs: 10000, endMs: 20000, text: "You sing first", agent: "v1",
        words: [{ startMs: 10000, endMs: 15000, text: "You sing " }, { startMs: 15000, endMs: 20000, text: "first" }],
      },
      {
        startMs: 12000, endMs: 16000, text: "(ooh)", role: "background", agent: "v1",
        words: [{ startMs: 12000, endMs: 16000, text: "(ooh)" }],
      },
      {
        startMs: 15000, endMs: 25000, text: "Then I join", agent: "v2",
        words: [{ startMs: 15000, endMs: 25000, text: "Then I join" }],
      },
      { startMs: 40000, text: "Outro" },
    ]),
  } as Track;

  beforeEach(() => {
    vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
    vi.stubGlobal("requestAnimationFrame", () => 0);
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
    Element.prototype.scrollTo = vi.fn();
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      playlist: [duet],
      currentTime: 5,
      isPlaying: false,
      showLyricsTranslation: true,
    });
  });
  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  const rowOf = (text: string) => screen.getByText(text).closest(".origin-left") as HTMLElement;

  it("制作信息并成一块、用 credit 样式且不逐字；和声挂在主句下；对唱重叠时两句同时高亮", () => {
    const { container } = render(<LyricsPanel />);
    // 制作信息：两行在同一组，data-role=credit，前奏期间它是当前组但不走逐字 / 打字机
    const credits = rowOf("作词：某人");
    expect(credits).toHaveAttribute("data-role", "credit");
    expect(credits).toBe(rowOf("作曲：某人"));
    expect(credits).toHaveAttribute("data-active", "true");
    expect(container.querySelector("[data-testid='karaoke-line']")).toBeNull();
    // 和声挂在第一句主句的组里，而不是自成一组
    expect(rowOf("You sing first")).toBe(screen.getByText("(ooh)").closest(".origin-left"));
    expect(screen.getByText("(ooh)").closest("[data-role='background']")).not.toBeNull();

    // 16s：第一句（10–20）与第二句（15–25）都在唱 → 两组都活动，主句是先开始的第一句
    act(() => usePlayerStore.setState({ currentTime: 16 }));
    expect(rowOf("Outro")).not.toHaveAttribute("data-active");
    const karaokes = container.querySelectorAll("[data-testid='karaoke-line']");
    // 第一句主唱 + 其和声 + 第二句主唱三条逐字线都在填色
    expect(karaokes).toHaveLength(3);
    const activeRows = container.querySelectorAll("[data-active='true']");
    expect(activeRows).toHaveLength(2);
    expect(activeRows[0].textContent).toContain("You sing");
    expect(activeRows[1].textContent).toContain("Then I join");
    expect(credits).not.toHaveAttribute("data-active");

    // 21s：第一句唱完，只剩第二句活动
    act(() => usePlayerStore.setState({ currentTime: 21 }));
    expect(container.querySelectorAll("[data-active='true']")).toHaveLength(1);
    expect(container.querySelector("[data-active='true']")!.textContent).toContain("Then I join");
  });

  it("显示制作信息关闭（后端按 hidden 打标）时制作信息块不渲染，前奏期间不高亮任何句", () => {
    usePlayerStore.setState({
      playlist: [{
        ...duet,
        lyrics: { ...duet.lyrics, lines: duet.lyrics.lines.map((line) => line.role === "credit" ? { ...line, hidden: true } : line) },
      }],
    });
    const { container } = render(<LyricsPanel />);
    expect(screen.queryByText("作词：某人")).toBeNull();
    expect(container.querySelector("[data-active='true']")).toBeNull();
    act(() => usePlayerStore.setState({ currentTime: 11 }));
    expect(container.querySelector("[data-active='true']")!.textContent).toContain("You sing");
  });
});
