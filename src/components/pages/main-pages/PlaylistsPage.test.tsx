// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { PlaylistsPage } from "./PlaylistsPage";

function track(index: number): Track {
  return {
    id: `t${index}`, title: `曲目 ${index}`, artist: "艺术家", album: "专辑", cover: "", format: "FLAC",
    bitdepth: "", bitrate: "", channels: "", size: "", path: `C:/m/${index}.flac`, duration: 180,
    glowColor: "", lyrics: lyricDocument([]),
  };
}

const playlist = Array.from({ length: 500 }, (_, index) => track(index));

beforeEach(() => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  usePlayerStore.setState({
    ...usePlayerStore.getInitialState(),
    playlist,
    userPlaylists: [{ id: "big", name: "大歌单", trackIds: playlist.map((item) => item.id), createdAt: 0 }],
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("PERF-02：大歌单详情只渲染可见区附近的行", () => {
  render(<PlaylistsPage />);
  fireEvent.click(screen.getByText("大歌单"));
  expect(screen.getByText("500 首曲目")).toBeInTheDocument();
  expect(screen.getByText("曲目 0")).toBeInTheDocument();
  expect(screen.queryByText("曲目 499")).toBeNull();
  expect(screen.getAllByRole("button", { name: /^播放 曲目/ }).length).toBeLessThan(60);
});

it("BUG-07：打开的歌单被删除后回到列表，且不在渲染期间更新父组件", () => {
  const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
  render(<PlaylistsPage />);
  fireEvent.click(screen.getByText("大歌单"));
  act(() => usePlayerStore.setState({ userPlaylists: [] }));
  expect(screen.getByText("DRAWER B — 歌单档案")).toBeInTheDocument();
  expect(consoleError).not.toHaveBeenCalledWith(
    expect.stringContaining("Cannot update a component"),
    expect.anything(),
    expect.anything(),
    expect.anything()
  );
});
