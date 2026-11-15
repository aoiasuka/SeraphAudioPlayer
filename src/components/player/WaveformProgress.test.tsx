// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { WaveformProgress } from "./WaveformProgress";

vi.mock("@/hooks/useWaveform", () => ({ useWaveform: () => undefined, WAVEFORM_SIDE_PADDING: 0 }));

const track: Track = {
  id: "a", title: "a", artist: "x", album: "y", cover: "", format: "FLAC", bitdepth: "16-bit",
  bitrate: "", channels: "Stereo", size: "1 MB", path: "C:/a.flac", duration: 200,
  glowColor: "#fff", lyrics: lyricDocument([]),
};
const seek = vi.fn();

beforeEach(() => {
  seek.mockReset();
  usePlayerStore.setState({ ...usePlayerStore.getInitialState(), playlist: [track], currentTrackIndex: 0, currentTime: 100, seek });
});
afterEach(cleanup);

it("FE-03：进度条是可聚焦的 slider，方向键 / Home / End 可跳转", () => {
  render(<WaveformProgress />);
  const slider = screen.getByRole("slider", { name: "播放进度" });
  expect(slider).toHaveAttribute("tabindex", "0");
  expect(slider).toHaveAttribute("aria-valuetext", "01:40 / 03:20");

  fireEvent.keyDown(slider, { key: "ArrowRight" });
  expect(seek).toHaveBeenLastCalledWith(105);
  fireEvent.keyDown(slider, { key: "ArrowLeft" });
  expect(seek).toHaveBeenLastCalledWith(95);
  fireEvent.keyDown(slider, { key: "PageUp" });
  expect(seek).toHaveBeenLastCalledWith(130);
  fireEvent.keyDown(slider, { key: "Home" });
  expect(seek).toHaveBeenLastCalledWith(0);
  fireEvent.keyDown(slider, { key: "End" });
  expect(seek).toHaveBeenLastCalledWith(200);
  fireEvent.keyDown(slider, { key: "a" });
  expect(seek).toHaveBeenCalledTimes(5);
});

it("时长未知时不可聚焦、按键不跳转", () => {
  usePlayerStore.setState({ playlist: [{ ...track, duration: 0 }] });
  render(<WaveformProgress />);
  const slider = screen.getByRole("slider");
  expect(slider).toHaveAttribute("tabindex", "-1");
  fireEvent.keyDown(slider, { key: "ArrowRight" });
  expect(seek).not.toHaveBeenCalled();
});
