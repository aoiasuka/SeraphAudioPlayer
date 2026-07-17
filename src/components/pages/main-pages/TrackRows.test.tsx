// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { TrackRows } from "./TrackRows";
import { isSeparator, useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

// jsdom 缺失的浏览器 API：虚拟列表用 ResizeObserver 测容器高度，
// 检索/排序变化时调用 scrollTo 复位
beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
  );
  Element.prototype.scrollTo = () => {};
});

function makeTrack(id: string, title: string, artist: string, duration: number): Track {
  return {
    id,
    title,
    artist,
    album: "Album",
    cover: "",
    format: "FLAC",
    bitdepth: "FLAC 24-bit / 96 kHz",
    bitrate: "Unknown",
    channels: "Stereo",
    size: "1 MB",
    path: `C:/Music/${id}.flac`,
    duration,
    glowColor: "#fff",
    lyrics: [],
  } as Track;
}

const TRACKS = [
  makeTrack("t1", "夜曲", "周杰伦", 226),
  makeTrack("t2", "First Love", "宇多田ヒカル", 258),
  makeTrack("t3", "Answer", "幾田りら", 190),
];

describe("TrackRows", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      playlist: TRACKS,
      currentTrackIndex: 0,
      liked: {},
      userPlaylists: [],
      isPlaying: false,
    });
  });

  afterEach(() => {
    cleanup();
  });

  it("渲染全部曲目与记录计数", () => {
    render(<TrackRows tracks={TRACKS} empty="空" />);
    expect(screen.getByText("夜曲")).toBeInTheDocument();
    expect(screen.getByText("First Love")).toBeInTheDocument();
    expect(screen.getByText("Answer")).toBeInTheDocument();
    expect(screen.getByText("3 RECORDS")).toBeInTheDocument();
  });

  it("空曲目列表显示占位文案", () => {
    render(<TrackRows tracks={[]} empty="暂无本地曲目" />);
    expect(screen.getByText("暂无本地曲目")).toBeInTheDocument();
  });

  it("检索过滤曲目并显示匹配计数", async () => {
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.type(screen.getByLabelText("检索曲目"), "first");

    expect(screen.getByText("First Love")).toBeInTheDocument();
    expect(screen.queryByText("夜曲")).not.toBeInTheDocument();
    expect(screen.getByText("1 / 3 RECORDS")).toBeInTheDocument();
  });

  it("检索无匹配时显示提示", async () => {
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.type(screen.getByLabelText("检索曲目"), "zzz");

    expect(screen.getByText(/没有匹配「zzz」的曲目/)).toBeInTheDocument();
  });

  it("清除检索恢复完整列表", async () => {
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.type(screen.getByLabelText("检索曲目"), "first");
    await user.click(screen.getByLabelText("清除检索"));

    expect(screen.getByText("夜曲")).toBeInTheDocument();
    expect(screen.getByText("3 RECORDS")).toBeInTheDocument();
  });

  it("按时长排序改变行顺序", async () => {
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.selectOptions(screen.getByLabelText("排序方式"), "duration");

    const titles = screen
      .getAllByRole("button", { name: /^播放 / })
      .map((button) => button.getAttribute("aria-label"));
    expect(titles).toEqual(["播放 Answer", "播放 夜曲", "播放 First Love"]);
  });

  it("点击「加入歌单」在无歌单时给出指引", async () => {
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.click(screen.getByLabelText("把 夜曲 加入歌单"));

    expect(
      screen.getByText(/还没有歌单——先到「歌单」页新建一个/)
    ).toBeInTheDocument();
  });

  it("从弹窗把曲目加入既有歌单", async () => {
    usePlayerStore.setState({
      userPlaylists: [
        { id: "pl-1", name: "驾车歌单", trackIds: [], createdAt: 1 },
      ],
    });
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.click(screen.getByLabelText("把 夜曲 加入歌单"));
    await user.click(screen.getByRole("button", { name: /驾车歌单/ }));

    expect(usePlayerStore.getState().userPlaylists[0].trackIds).toEqual(["t1"]);
  });

  it("右键曲目行打开全局右键菜单（v0.4.3）", () => {
    useContextMenuStore.getState().closeContextMenu();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    fireEvent.contextMenu(screen.getByText("夜曲"));

    const menu = useContextMenuStore.getState();
    expect(menu.open).toBe(true);
    const labels = menu.entries
      .filter((entry) => !isSeparator(entry))
      .map((entry) => (isSeparator(entry) ? "" : entry.label));
    expect(labels).toContain("播放");
    expect(labels).toContain("加入歌单");
    expect(labels).toContain("曲目信息…");
    expect(labels).toContain("删除曲库记录");
  });

  it("行内删除按钮改走全局删除确认（v0.4.3）", async () => {
    useContextMenuStore.setState({ confirmDeleteTrackId: null });
    const user = userEvent.setup();
    render(<TrackRows tracks={TRACKS} empty="空" />);

    await user.click(screen.getByLabelText("删除曲库记录 夜曲"));

    expect(useContextMenuStore.getState().confirmDeleteTrackId).toBe("t1");
  });
});
