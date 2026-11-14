// @vitest-environment jsdom
import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { responseCurveRange } from "@/components/pages/main-pages/EqPage";
import { useSpacePlayback } from "@/hooks/useSpacePlayback";
import { useUpdateCheck } from "@/hooks/useUpdateCheck";
import { checkForUpdate } from "@/lib/update";
import { useImmersiveStore } from "@/store/immersive";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/lib/update", () => ({
  checkForUpdate: vi.fn().mockResolvedValue({ updateAvailable: false, currentVersion: "0.0.0", latestVersion: "0.0.0" }),
}));

function pressSpace(target: HTMLElement) {
  const event = new KeyboardEvent("keydown", { code: "Space", key: " ", bubbles: true, cancelable: true });
  target.dispatchEvent(event);
  return event;
}

describe("UI5-02 空格键不抢可交互控件", () => {
  const togglePlayback = vi.fn();
  beforeEach(() => {
    togglePlayback.mockClear();
    usePlayerStore.setState({ togglePlayback, currentTrack: () => ({ id: "t" }) as Track } as never);
  });
  afterEach(() => { document.body.innerHTML = ""; });

  it.each([
    ["button", {}],
    ["div", { role: "switch" }],
    ["div", { role: "tab" }],
    ["a", { href: "#" }],
  ] as const)("焦点在 <%s %o> 上时保留原生空格行为", (tag, attributes) => {
    const { unmount } = renderHook(() => useSpacePlayback());
    const element = document.createElement(tag);
    for (const [name, value] of Object.entries(attributes)) element.setAttribute(name, value);
    const inner = document.createElement("span");
    element.appendChild(inner);
    document.body.appendChild(element);
    const event = pressSpace(inner);
    expect(togglePlayback).not.toHaveBeenCalled();
    expect(event.defaultPrevented).toBe(false);
    unmount();
  });

  it("焦点在非交互元素上时仍是播放快捷键", () => {
    const { unmount } = renderHook(() => useSpacePlayback());
    const div = document.createElement("div");
    document.body.appendChild(div);
    const event = pressSpace(div);
    expect(togglePlayback).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
    unmount();
  });
});

describe("UI5-04 启动时检查更新开关", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(checkForUpdate).mockClear();
    usePlayerStore.setState(usePlayerStore.getInitialState());
  });
  afterEach(() => { vi.useRealTimers(); });

  it("默认开启，启动 8 秒后检查一次", () => {
    expect(usePlayerStore.getState().autoUpdateCheck).toBe(true);
    renderHook(() => useUpdateCheck());
    vi.advanceTimersByTime(8000);
    expect(checkForUpdate).toHaveBeenCalledTimes(1);
  });

  it("关闭后不自动联网", () => {
    usePlayerStore.getState().setAutoUpdateCheck(false);
    renderHook(() => useUpdateCheck());
    vi.advanceTimersByTime(8000);
    expect(checkForUpdate).not.toHaveBeenCalled();
  });
});

describe("UI5-05 频响曲线量程", () => {
  it("±15 dB 以内保持默认量程，超出按 6 dB 一档放大", () => {
    expect(responseCurveRange([0, 3, -14.9])).toBe(15);
    expect(responseCurveRange([0, 16])).toBe(18);
    expect(responseCurveRange([-24, 5])).toBe(24);
    expect(responseCurveRange([Number.NaN, 40])).toBe(42);
  });
});

describe("沉浸页歌词字号", () => {
  it("关闭沉浸页再打开仍保留", () => {
    useImmersiveStore.setState({ largeLyrics: false });
    useImmersiveStore.getState().toggleLargeLyrics();
    useImmersiveStore.getState().close();
    useImmersiveStore.getState().open();
    expect(useImmersiveStore.getState().largeLyrics).toBe(true);
    useImmersiveStore.setState({ largeLyrics: false });
  });
});
