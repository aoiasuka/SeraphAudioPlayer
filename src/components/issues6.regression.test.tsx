// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AudioInfoCard } from "@/components/sidebar/AudioInfoCard";
import { DeviceMenu } from "@/components/player/DeviceMenu";
import { UpNextCard } from "@/components/sidebar/UpNextCard";
import { ArtistsPage } from "@/components/pages/main-pages/ArtistsPage";
import { PlaylistsPage } from "@/components/pages/main-pages/PlaylistsPage";
import { TrackRows } from "@/components/pages/main-pages/TrackRows";
import { lyricDocument } from "@/lib/lyrics/document";
import { forgetAllViews, recallView } from "@/lib/viewMemory";
import { useEqStore } from "@/store/eq";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

function track(id: string, title: string, artist: string): Track {
  return {
    id, title, artist, album: "Album", cover: "", format: "FLAC",
    bitdepth: "FLAC 24-bit / 96 kHz", bitrate: "Unknown", channels: "Stereo", size: "1 MB",
    path: `C:/Music/${id}.flac`, duration: 180, glowColor: "#fff", lyrics: lyricDocument([]),
  } as Track;
}

const TRACKS = [
  track("t1", "夜曲", "周杰伦"),
  track("t2", "First Love", "宇多田ヒカル"),
  track("t3", "合作曲", "周杰伦/方文山"),
];

beforeAll(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
  Element.prototype.scrollTo = () => {};
});

beforeEach(() => {
  forgetAllViews();
  usePlayerStore.setState({
    ...usePlayerStore.getInitialState(),
    playlist: TRACKS,
    currentTrackIndex: 0,
  });
});

afterEach(cleanup);

describe("页面状态记忆（切换页面后恢复）", () => {
  it("曲目列表重新挂载后保留搜索词和排序；不同列表互不影响", () => {
    const first = render(<TrackRows tracks={TRACKS} empty="空" stateKey="tracks:test" />);
    fireEvent.change(screen.getByLabelText("检索曲目"), { target: { value: "first" } });
    fireEvent.change(screen.getByLabelText("排序方式"), { target: { value: "title" } });
    first.unmount();

    render(<TrackRows tracks={TRACKS} empty="空" stateKey="tracks:test" />);
    expect(screen.getByLabelText("检索曲目")).toHaveValue("first");
    expect(screen.getByLabelText("排序方式")).toHaveValue("title");
    expect(screen.queryByText("夜曲")).toBeNull();
    cleanup();

    render(<TrackRows tracks={TRACKS} empty="空" stateKey="tracks:other" />);
    expect(screen.getByLabelText("检索曲目")).toHaveValue("");
  });

  it("没有 stateKey 的列表不记忆", () => {
    const first = render(<TrackRows tracks={TRACKS} empty="空" />);
    fireEvent.change(screen.getByLabelText("检索曲目"), { target: { value: "first" } });
    first.unmount();
    render(<TrackRows tracks={TRACKS} empty="空" />);
    expect(screen.getByLabelText("检索曲目")).toHaveValue("");
  });

  it("滚动位置随滚动写入记忆，清空搜索时归零", () => {
    render(<TrackRows tracks={TRACKS} empty="空" stateKey="tracks:test" />);
    const scroller = screen.getByLabelText("检索曲目").closest("div.flex-col")!
      .querySelector(".overflow-y-auto") as HTMLElement;
    fireEvent.scroll(scroller, { target: { scrollTop: 118 } });
    expect(recallView("tracks:test:scroll", 0)).toBe(118);
    fireEvent.change(screen.getByLabelText("检索曲目"), { target: { value: "夜" } });
    expect(recallView("tracks:test:scroll", -1)).toBe(0);
  });

  it("艺术家页记住打开的艺术家；合作曲目归到每位艺术家名下并按名称排序", () => {
    const first = render(<ArtistsPage />);
    expect(screen.getByText("3 ARTISTS")).toBeInTheDocument();
    // 「周杰伦/方文山」被拆开：周杰伦名下 2 首
    fireEvent.click(screen.getByText("周杰伦"));
    expect(screen.getByText("2 首曲目")).toBeInTheDocument();
    first.unmount();

    render(<ArtistsPage />);
    expect(screen.getByText("返回艺术家")).toBeInTheDocument();
    expect(screen.getByText("2 首曲目")).toBeInTheDocument();
  });

  it("艺术家页的筛选框按名称过滤", () => {
    render(<ArtistsPage />);
    fireEvent.change(screen.getByLabelText("筛选艺术家"), { target: { value: "方文" } });
    expect(screen.getByText("1 / 3 ARTISTS")).toBeInTheDocument();
    expect(screen.queryByText("宇多田ヒカル")).toBeNull();
  });
});

describe("歌单卡片", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      userPlaylists: [{ id: "p1", name: "我的歌单", trackIds: ["t1"], createdAt: 0 }],
    });
  });

  it("打开与删除是两个独立按钮；在删除按钮上按回车不会同时打开歌单", () => {
    render(<PlaylistsPage />);
    const remove = screen.getByRole("button", { name: "删除歌单 我的歌单" });
    expect(remove.closest('[role="button"]')).toBeNull();
    fireEvent.keyDown(remove, { key: "Enter" });
    fireEvent.click(remove);
    // 仍在列表页，弹出的是删除确认
    expect(screen.getByText("DRAWER B — 歌单档案")).toBeInTheDocument();
    expect(screen.getByText("Delete Playlist")).toBeInTheDocument();
    expect(recallView("playlists:selected", null)).toBeNull();
  });

  it("记住打开的歌单", () => {
    const first = render(<PlaylistsPage />);
    fireEvent.click(screen.getByRole("button", { name: /^打开歌单 我的歌单/ }));
    expect(screen.getByText("返回歌单")).toBeInTheDocument();
    first.unmount();
    render(<PlaylistsPage />);
    expect(screen.getByText("返回歌单")).toBeInTheDocument();
  });
});

describe("设备菜单", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      devices: [
        { id: "a", name: "扬声器", isDefault: true },
        { id: "b", name: "USB DAC", isDefault: false },
      ] as never,
      currentDeviceId: "b",
      deviceMenuOpen: false,
    });
  });

  it("触发按钮报告展开状态，打开后焦点落在当前设备，Esc 关闭并归还焦点", () => {
    render(<DeviceMenu />);
    const trigger = screen.getByRole("button", { name: "切换输出设备" });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");
    const current = screen.getByRole("menuitemradio", { name: /USB DAC/ });
    expect(current).toHaveAttribute("aria-checked", "true");
    expect(document.activeElement).toBe(current);

    fireEvent.keyDown(current, { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("menuitemradio", { name: /扬声器/ }));

    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});

describe("「下一首」卡片", () => {
  it("是一个可聚焦的按钮，点击切到下一首", () => {
    const playNextPreview = vi.fn();
    usePlayerStore.setState({ playNextPreview, nextTrackPreview: () => TRACKS[1] });
    render(<UpNextCard />);
    const button = screen.getByRole("button", { name: /^播放下一首：First Love/ });
    fireEvent.click(button);
    expect(playNextPreview).toHaveBeenCalledTimes(1);
  });
});

describe("音频信息卡片的输出链路", () => {
  const format = {
    trackId: "t1", mode: "shared" as const, sourceRate: 96_000, sourceBits: 24, sourceChannels: 2,
    outputRate: 48_000, outputBits: 32, outputFloat: true, outputChannels: 2,
  };

  beforeEach(() => {
    useEqStore.setState({ enabled: false });
  });

  it("显示当前曲目的输出格式与处理环节", () => {
    usePlayerStore.setState({ outputFormat: format });
    render(<AudioInfoCard />);
    expect(screen.getByText("共享（系统混音） · 48 kHz / 32-bit float")).toBeInTheDocument();
    expect(screen.getByText("重采样 96 → 48 kHz")).toBeInTheDocument();
    act(() => useEqStore.setState({ enabled: true }));
    expect(screen.getByText("重采样 96 → 48 kHz · EQ")).toBeInTheDocument();
  });

  it("输出格式属于别的曲目（切歌后尚未起播）或没有时不显示", () => {
    usePlayerStore.setState({ outputFormat: { ...format, trackId: "t2" } });
    render(<AudioInfoCard />);
    expect(screen.queryByTestId("output-chain")).toBeNull();
    act(() => usePlayerStore.setState({ outputFormat: null }));
    expect(screen.queryByTestId("output-chain")).toBeNull();
  });
});
