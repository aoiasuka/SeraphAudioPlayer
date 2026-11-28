// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AnalysisFrame } from "@/lib/analysis/types";
import { encodeAnalysisFrame } from "@/lib/analysis/wire.testutil";
import { usePlayerStore } from "@/store/player";
import { useImmersiveStore } from "@/store/immersive";
import { AnalysisPage } from "@/components/pages/main-pages/AnalysisPage";
import { ImmersiveAnalysis } from "./ImmersiveAnalysis";

const bridge = vi.hoisted(() => ({ desktop: true, invoke: vi.fn() }));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...await importOriginal<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => bridge.desktop,
  invoke: bridge.invoke,
}));

/** 后端返回的是二进制帧（VIZ-01）：按同一布局编码后交给被 mock 的 invoke。 */
function frame(requestId: string, sequence: number, correlation = 0.9): ArrayBuffer {
  const value: AnalysisFrame & { requestId: string; sequence: number } = {
    requestId, sequence, spectrum: Array(96).fill(0.65), peakLeft: 0.5, peakRight: 0.4, rmsLeft: 0.25, rmsRight: 0.2,
    correlation, scatter: [0.1, 0.1, -0.1, -0.1], waveform: [], sampleRate: 48000,
    momentaryLufs: null, shortTermLufs: null, integratedLufs: null, loudnessRangeLu: null, truePeakDb: null, truePeakMaxDb: null,
  };
  return encodeAnalysisFrame(value);
}

async function advance(ms: number) { await act(async () => { await vi.advanceTimersByTimeAsync(ms); }); }

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "Date", "performance"] });
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} unobserve() {} });
  vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => window.setTimeout(() => callback(performance.now()), 16)));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: false, addEventListener() {}, removeEventListener() {} }));
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
  vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
  bridge.desktop = true;
  bridge.invoke.mockReset();
  let sequence = 0;
  bridge.invoke.mockImplementation(async (_command: string, args: { request: { requestId: string } }) => frame(args.request.requestId, ++sequence));
  usePlayerStore.setState({ isPlaying: true, currentTime: 42 });
  useImmersiveStore.setState({ isOpen: false, mode: "lyrics" });
});

afterEach(() => { cleanup(); vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

describe("沉浸分析的真实数据与生命周期", () => {
  it("只请求频谱、电平和声场；冻结停止 IPC，保留仪表且不暂停播放器", async () => {
    render(<ImmersiveAnalysis />);
    await advance(120);
    expect(bridge.invoke).toHaveBeenCalledWith("get_analysis_frame", expect.objectContaining({ demand: { spectrum: true, loudness: false, levels: true, field: true, scope: false, fieldPoints: 1440 } }));
    expect(screen.getByLabelText("声道相关度")).not.toHaveTextContent("—");
    fireEvent.click(screen.getByRole("button", { name: "冻结画面" }));
    await advance(20);
    const calls = bridge.invoke.mock.calls.length;
    const levels = screen.getByLabelText("左右声道电平").textContent;
    const correlation = screen.getByLabelText("声道相关度").textContent;
    act(() => usePlayerStore.setState({ currentTime: 60 }));
    await advance(1000);
    expect(bridge.invoke).toHaveBeenCalledTimes(calls);
    expect(screen.getByLabelText("左右声道电平").textContent).toBe(levels);
    expect(screen.getByLabelText("声道相关度").textContent).toBe(correlation);
    expect(usePlayerStore.getState()).toMatchObject({ isPlaying: true, currentTime: 60 });
    fireEvent.click(screen.getByRole("button", { name: "继续分析" }));
    await advance(60);
    expect(bridge.invoke.mock.calls.length).toBeGreaterThan(calls);
  });

  it("冻结前未完成的帧会被丢弃，恢复时使用新的请求身份", async () => {
    let complete!: (value: ReturnType<typeof frame>) => void;
    bridge.invoke.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
    render(<ImmersiveAnalysis />);
    await advance(0);
    const oldId = bridge.invoke.mock.calls[0][1].request.requestId as string;
    fireEvent.click(screen.getByRole("button", { name: "冻结画面" }));
    complete(frame(oldId, 100, -1));
    await advance(100);
    expect(screen.getByLabelText("声道相关度")).toHaveTextContent("—");
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "继续分析" }));
    await advance(60);
    expect(screen.getByLabelText("声道相关度")).toHaveTextContent("+");
    expect(bridge.invoke.mock.calls[1][1].request.requestId).not.toBe(oldId);
  });

  it("隐藏停止取帧和绘制；恢复可继续，暂停停止 IPC", async () => {
    render(<ImmersiveAnalysis />);
    await advance(120);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("hidden");
    fireEvent(document, new Event("visibilitychange"));
    const calls = bridge.invoke.mock.calls.length;
    const scheduled = vi.mocked(requestAnimationFrame).mock.calls.length;
    await advance(500);
    expect(bridge.invoke).toHaveBeenCalledTimes(calls);
    expect(vi.mocked(requestAnimationFrame).mock.calls.length).toBe(scheduled);
    vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
    fireEvent(document, new Event("visibilitychange"));
    await advance(80);
    expect(bridge.invoke.mock.calls.length).toBeGreaterThan(calls);
    act(() => usePlayerStore.setState({ isPlaying: false }));
    const pausedCalls = bridge.invoke.mock.calls.length;
    await advance(300);
    expect(bridge.invoke).toHaveBeenCalledTimes(pausedCalls);
  });

  it("切歌重建会话并等待旧请求退出，不把上一首的帧送给下一首", async () => {
    let complete!: (value: ReturnType<typeof frame>) => void;
    bridge.invoke.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
    const { rerender } = render(<ImmersiveAnalysis key="track-a" />);
    await advance(0);
    const oldId = bridge.invoke.mock.calls[0][1].request.requestId as string;
    rerender(<ImmersiveAnalysis key="track-b" />);
    await advance(100);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("声道相关度")).toHaveTextContent("—");
    complete(frame(oldId, 100, -1));
    await advance(60);
    expect(screen.getByLabelText("声道相关度")).toHaveTextContent("+");
    expect(bridge.invoke.mock.calls[1][1].request.sessionId).not.toBe(bridge.invoke.mock.calls[0][1].request.sessionId);
  });

  it("进入沉浸分析时后台分析页让出轮询，退出后恢复原分析页", async () => {
    function Harness() {
      const open = useImmersiveStore((s) => s.isOpen);
      return <><AnalysisPage />{open && <ImmersiveAnalysis />}</>;
    }
    render(<Harness />);
    await advance(50);
    const backgroundSession = bridge.invoke.mock.calls[0][1].request.sessionId;
    act(() => useImmersiveStore.getState().open());
    const atOpen = bridge.invoke.mock.calls.length;
    await advance(120);
    const foregroundCalls = bridge.invoke.mock.calls.slice(atOpen);
    expect(foregroundCalls.length).toBeGreaterThan(0);
    expect(foregroundCalls.every(([, args]) => args.request.sessionId !== backgroundSession && !args.demand.loudness)).toBe(true);
    act(() => useImmersiveStore.getState().close());
    await advance(70);
    expect(bridge.invoke.mock.calls.at(-1)?.[1].request.sessionId).toBe(backgroundSession);
  });

  it("声场每帧固定要 1440 个点，画布像素比是设备像素比的整数倍", async () => {
    // jsdom 没有布局也没有 canvas：给容器一个尺寸、给一个只记录矩形的 2D 上下文
    const rects: Array<{ canvas: HTMLCanvasElement | null; w: number; h: number }> = [];
    const contexts = new WeakMap<HTMLCanvasElement, unknown>();
    const contextFor = (canvas: HTMLCanvasElement) => {
      let scale = 1;
      return new Proxy({}, {
        get: (_target, key) =>
          key === "setTransform" ? (a: number) => { scale = a; }
            : key === "getTransform" ? () => ({ a: scale, d: scale })
            : key === "fillRect" ? (_x: number, _y: number, w: number, h: number) => { rects.push({ canvas, w, h }); }
            : key === "measureText" ? () => ({ width: 10 })
            : typeof key === "string" && key.startsWith("create") ? () => ({ addColorStop() {} })
            : () => undefined,
        set: () => true,
      });
    };
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (this: HTMLCanvasElement) {
      if (!contexts.has(this)) contexts.set(this, contextFor(this));
      return contexts.get(this) as never;
    });
    // 2560×1440、系统缩放 150% 下实测的声场容器：约 238 px 见方
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(238);
    vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(238);
    vi.stubGlobal("devicePixelRatio", 1.5);

    render(<ImmersiveAnalysis />);
    for (let step = 0; step < 6; step += 1) await advance(40);

    const requested = bridge.invoke.mock.calls
      .filter(([command]) => command === "get_analysis_frame")
      .map(([, args]) => (args as { demand: { fieldPoints: number } }).demand.fieldPoints);
    expect(requested.length).toBeGreaterThan(2);
    expect(new Set(requested)).toEqual(new Set([1440]));

    // 像素比 3（= 1.5 × 2）：缩到 150% 显示是整数比。此前是 2，每个点落在 1.5 个设备像素上
    const field = screen.getByRole("img", { name: "左右声道矢量图" }) as HTMLCanvasElement;
    expect(field.width).toBe(238 * 3);
    expect(field.height).toBe(238 * 3);
    // 每个点 4 个画布像素见方 = 2 个设备像素
    const points = rects.filter((rect) => rect.canvas === field);
    expect(points.length).toBeGreaterThan(0);
    for (const point of points) {
      expect(point.w * 3).toBeCloseTo(4, 6);
      expect(point.h * 3).toBeCloseTo(4, 6);
    }
  });

  it("纯浏览器不伪造分析数据，冻结和键盘游标仍可使用", async () => {
    bridge.desktop = false;
    render(<ImmersiveAnalysis />);
    await advance(100);
    expect(bridge.invoke).not.toHaveBeenCalled();
    expect(screen.getByLabelText("声道相关度")).toHaveTextContent("—");
    fireEvent.keyDown(screen.getByRole("img", { name: /频谱，可用/ }), { key: "End" });
    await advance(50);
    expect(screen.getByText(/20.00 kHz \/ — dBFS/)).toBeInTheDocument();
  });
});
