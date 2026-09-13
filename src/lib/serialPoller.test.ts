// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSerialPoller } from "./serialPoller";

describe("串行轮询的生命周期", () => {
  let stop: (() => void) | undefined;
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  });
  afterEach(() => {
    stop?.();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("300ms 慢响应期间最多一个请求在途，完成后再轮询", async () => {
    const poller = createSerialPoller();
    let complete!: (value: number) => void;
    const read = vi.fn(() => new Promise<number>((resolve) => { complete = resolve; }));
    const onData = vi.fn();
    stop = poller.start({ intervalMs: 33, read, onData });
    await vi.advanceTimersByTimeAsync(300);
    expect(read).toHaveBeenCalledTimes(1);
    complete(7);
    await vi.advanceTimersByTimeAsync(1);
    expect(onData).toHaveBeenCalledWith(7);
    expect(read).toHaveBeenCalledTimes(2);
    stop();
    complete(8);
    await vi.advanceTimersByTimeAsync(300);
    expect(onData).toHaveBeenCalledTimes(1);
    expect(read).toHaveBeenCalledTimes(2);
  });

  it("切歌/切页后等待旧请求结束，旧帧不写回新视图", async () => {
    const poller = createSerialPoller();
    let complete!: (value: string) => void;
    const firstData = vi.fn();
    const stopFirst = poller.start({ intervalMs: 33,
      read: () => new Promise<string>((resolve) => { complete = resolve; }), onData: firstData });
    await vi.advanceTimersByTimeAsync(0);
    const nextRead = vi.fn(async () => "new");
    const nextData = vi.fn();
    stop = poller.start({ intervalMs: 33, read: nextRead, onData: nextData });
    stopFirst(); // 旧 effect 的 cleanup 不得停止新任务。
    await vi.advanceTimersByTimeAsync(100);
    expect(nextRead).not.toHaveBeenCalled();
    complete("old");
    await vi.advanceTimersByTimeAsync(1);
    expect(firstData).not.toHaveBeenCalled();
    expect(nextData).toHaveBeenCalledWith("new");
    expect(nextRead).toHaveBeenCalledTimes(1);
  });

  it("隐藏时停止请求，恢复后丢弃隐藏前的迟到响应", async () => {
    const poller = createSerialPoller();
    let complete!: (value: string) => void;
    const read = vi.fn(() => new Promise<string>((resolve) => { complete = resolve; }));
    const onData = vi.fn();
    stop = poller.start({ intervalMs: 33, read, onData });
    await vi.advanceTimersByTimeAsync(0);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(500);
    expect(read).toHaveBeenCalledTimes(1);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    document.dispatchEvent(new Event("visibilitychange"));
    complete("stale");
    await vi.advanceTimersByTimeAsync(1);
    expect(onData).not.toHaveBeenCalled();
    expect(read).toHaveBeenCalledTimes(2);
    complete("fresh");
    await vi.advanceTimersByTimeAsync(1);
    expect(onData).toHaveBeenCalledWith("fresh");
  });
});
