// @vitest-environment jsdom
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SMOOTH_TIME_MAX_EXTRAPOLATION_MS, useSmoothTime } from "./useSmoothTime";

describe("useSmoothTime", () => {
  let now = 0;
  let frames: Array<FrameRequestCallback> = [];

  beforeEach(() => {
    now = 1000;
    frames = [];
    vi.spyOn(performance, "now").mockImplementation(() => now);
    vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
      frames.push(cb);
      return frames.length;
    });
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  const tick = () => {
    const pending = frames;
    frames = [];
    act(() => pending.forEach((cb) => cb(now)));
  };

  it("播放中按墙钟外推", () => {
    const { result } = renderHook(() => useSmoothTime(5000, true));
    now += 120;
    tick();
    expect(result.current).toBe(5120);
  });

  it("进度事件停止后外推封顶，不无限前进", () => {
    const { result } = renderHook(() => useSmoothTime(5000, true));
    now += 10_000;
    tick();
    expect(result.current).toBe(5000 + SMOOTH_TIME_MAX_EXTRAPOLATION_MS);
  });

  it("新的锚点到来后重新校准", () => {
    const { result, rerender } = renderHook(({ ms }) => useSmoothTime(ms, true), { initialProps: { ms: 5000 } });
    now += 10_000;
    tick();
    rerender({ ms: 5250 });
    now += 40;
    tick();
    expect(result.current).toBe(5290);
  });
});
