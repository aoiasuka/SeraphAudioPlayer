// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { useSpacePlayback } from "./useSpacePlayback";
import type { Track } from "@/types/track";

const testTrack = { id: "t1", title: "Song", path: "C:/a.flac" } as unknown as Track;

function pressSpace(target?: HTMLElement) {
  const event = new KeyboardEvent("keydown", {
    code: "Space",
    key: " ",
    bubbles: true,
    cancelable: true,
  });
  (target ?? window).dispatchEvent(event);
  return event;
}

describe("useSpacePlayback", () => {
  const togglePlayback = vi.fn();

  beforeEach(() => {
    togglePlayback.mockClear();
    usePlayerStore.setState({
      togglePlayback,
      currentTrack: () => testTrack,
    } as never);
  });

  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("空格切换播放并阻止默认滚动", () => {
    const { unmount } = renderHook(() => useSpacePlayback());
    const event = pressSpace();
    expect(togglePlayback).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
    unmount();
  });

  it("焦点在文本输入框时不劫持空格", () => {
    const { unmount } = renderHook(() => useSpacePlayback());
    const input = document.createElement("input");
    document.body.appendChild(input);
    pressSpace(input);
    expect(togglePlayback).not.toHaveBeenCalled();
    unmount();
  });

  it("弹窗内交互不劫持空格", () => {
    const { unmount } = renderHook(() => useSpacePlayback());
    const dialog = document.createElement("div");
    dialog.setAttribute("role", "dialog");
    const button = document.createElement("button");
    dialog.appendChild(button);
    document.body.appendChild(dialog);
    pressSpace(button);
    expect(togglePlayback).not.toHaveBeenCalled();
    unmount();
  });

  it("没有当前曲目时不触发", () => {
    usePlayerStore.setState({ currentTrack: () => null } as never);
    const { unmount } = renderHook(() => useSpacePlayback());
    pressSpace();
    expect(togglePlayback).not.toHaveBeenCalled();
    unmount();
  });

  it("卸载后移除监听", () => {
    const { unmount } = renderHook(() => useSpacePlayback());
    unmount();
    pressSpace();
    expect(togglePlayback).not.toHaveBeenCalled();
  });
});
