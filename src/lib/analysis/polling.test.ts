// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AnalysisFrame } from "./types";
import type { SpectrumFrame } from "./wire";
import { encodeAnalysisFrame, encodeSpectrumFrame } from "./wire.testutil";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@/lib/tauri", () => ({ invoke: bridge.invoke }));

import { pollVisualizer } from "./polling";

const DEMAND = { spectrum: true, loudness: false, levels: true, field: false, scope: false };

function analysisFrame(requestId: string, sequence: number, correlation: number): ArrayBuffer {
  return encodeAnalysisFrame({
    requestId, sequence, correlation,
    spectrum: [0.25, 0.5], scatter: [], waveform: [], sampleRate: 48000,
    peakLeft: 0.5, peakRight: 0.5, rmsLeft: 0.25, rmsRight: 0.25,
    momentaryLufs: null, shortTermLufs: null, integratedLufs: null,
    loudnessRangeLu: null, truePeakDb: null, truePeakMaxDb: null,
  });
}

const tick = (ms: number) => vi.advanceTimersByTimeAsync(ms);

describe("pollVisualizer（VIZ-01 二进制帧）", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    bridge.invoke.mockReset();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("频谱命令返回的字节解码成帧交给回调", async () => {
    bridge.invoke.mockImplementation(async (_command: string, args: { request: { requestId: string } }) =>
      encodeSpectrumFrame({ bins: [0, 0.5, 1], peakLeft: 0.25, peakRight: 0.75, requestId: args.request.requestId, sequence: 1 }));
    const frames: SpectrumFrame[] = [];
    const stop = pollVisualizer("get_spectrum_frame", "session-a", (frame) => frames.push(frame));
    await tick(10);
    stop();
    expect(bridge.invoke).toHaveBeenCalledWith("get_spectrum_frame", {
      request: { sessionId: "session-a", requestId: expect.stringMatching(/^session-a:/) },
    });
    expect(frames).toHaveLength(1);
    expect(frames[0].bins).toBeInstanceOf(Float32Array);
    expect(Array.from(frames[0].bins)).toEqual([0, 0.5, 1]);
    expect(frames[0].peakRight).toBe(0.75);
  });

  it("分析命令带上需求；还没有帧（空字节串）时不回调", async () => {
    bridge.invoke.mockResolvedValue(new ArrayBuffer(0));
    const onFrame = vi.fn();
    const stop = pollVisualizer("get_analysis_frame", "session-b", onFrame, DEMAND);
    await tick(100);
    stop();
    expect(bridge.invoke).toHaveBeenCalledWith("get_analysis_frame", expect.objectContaining({ demand: DEMAND }));
    expect(bridge.invoke.mock.calls.length).toBeGreaterThan(1);
    expect(onFrame).not.toHaveBeenCalled();
  });

  it("请求身份对不上、或序号没前进的帧被丢弃", async () => {
    const replies = [
      (id: string) => analysisFrame(id, 5, 0.1),
      // 上一个请求的迟到帧：身份不符
      () => analysisFrame("someone-else:1", 6, 0.2),
      // 后端还没算出新帧，重复返回同一序号
      (id: string) => analysisFrame(id, 5, 0.3),
      (id: string) => analysisFrame(id, 6, 0.4),
    ];
    let call = 0;
    bridge.invoke.mockImplementation(async (_command: string, args: { request: { requestId: string } }) => {
      const reply = replies[Math.min(call, replies.length - 1)];
      call += 1;
      return reply(args.request.requestId);
    });
    const seen: number[] = [];
    const stop = pollVisualizer("get_analysis_frame", "session-c", (frame: AnalysisFrame) => {
      seen.push(Math.round(frame.correlation * 10) / 10);
    }, DEMAND);
    await tick(200);
    stop();
    expect(seen).toEqual([0.1, 0.4]);
  });

  it("解不开的帧不回调，整轮轮询只记一次日志，后续好帧照常送达", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    let call = 0;
    bridge.invoke.mockImplementation(async (_command: string, args: { request: { requestId: string } }) => {
      call += 1;
      // 前三次返回 JSON 时代的对象形态（前后端版本不一致时会出现）
      return call <= 3 ? { spectrum: [] } : analysisFrame(args.request.requestId, call, 0.5);
    });
    const onFrame = vi.fn();
    const stop = pollVisualizer("get_analysis_frame", "session-d", onFrame, DEMAND);
    await tick(300);
    stop();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0][0])).toContain("get_analysis_frame");
    expect(onFrame).toHaveBeenCalled();
  });
});
