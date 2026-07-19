import { describe, expect, it } from "vitest";
import {
  applyAnalysisFrame,
  createAnalysisView,
  resetAnalysisSession,
  stepAnalysisView,
} from "./view";
import {
  ANALYSIS_BIN_COUNT,
  SPECTRUM_DB_FLOOR,
  type AnalysisFrame,
} from "./types";

function frame(overrides: Partial<AnalysisFrame> = {}): AnalysisFrame {
  return {
    spectrum: new Array(ANALYSIS_BIN_COUNT).fill(0),
    peakLeft: 0,
    peakRight: 0,
    rmsLeft: 0,
    rmsRight: 0,
    momentaryLufs: null,
    shortTermLufs: null,
    integratedLufs: null,
    loudnessRangeLu: null,
    truePeakDb: null,
    truePeakMaxDb: null,
    correlation: 0,
    scatter: [],
    waveform: [],
    sampleRate: 48000,
    ...overrides,
  };
}

describe("analysis view ballistics (v0.4.6)", () => {
  it("maps normalized spectrum toward dB targets with fast attack", () => {
    const view = createAnalysisView();
    // 连续摄入满幅 bin：显示值应快速逼近 0dB
    let now = 1;
    for (let i = 0; i < 30; i += 1) {
      now += 0.033;
      applyAnalysisFrame(view, frame({ spectrum: new Array(ANALYSIS_BIN_COUNT).fill(1) }), now);
    }
    expect(view.spectrumDb[10]).toBeGreaterThan(-3);
    expect(view.hasData).toBe(true);
  });

  it("holds level peaks then falls back after the hold window", () => {
    const view = createAnalysisView();
    // 一记 -6dBFS 峰值
    applyAnalysisFrame(view, frame({ peakLeft: 0.5, rmsLeft: 0.3 }), 1);
    stepAnalysisView(view, 1.016);
    const heldDb = view.levels.l.holdDb;
    expect(heldDb).toBeGreaterThan(-7);

    // 1.4s 保持期内不回落
    stepAnalysisView(view, 2.0);
    expect(view.levels.l.holdDb).toBeCloseTo(heldDb, 5);

    // 保持期过后以 14dB/s 回落
    stepAnalysisView(view, 2.6);
    stepAnalysisView(view, 2.7);
    expect(view.levels.l.holdDb).toBeLessThan(heldDb - 1);
  });

  it("raises the clip flag near full scale and clears it after 3s", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(view, frame({ peakLeft: 0.999 }), 1);
    expect(view.levels.clip).toBe(true);

    stepAnalysisView(view, 2);
    expect(view.levels.clip).toBe(true);
    stepAnalysisView(view, 4.2);
    expect(view.levels.clip).toBe(false);
  });

  it("pushes spectrogram history rows on the 90ms cadence", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(view, frame({ spectrum: new Array(ANALYSIS_BIN_COUNT).fill(1) }), 1);
    const headBefore = view.historyHead;
    stepAnalysisView(view, 1.0);
    stepAnalysisView(view, 1.05); // 未到 90ms，不推进
    const headAfterFirst = view.historyHead;
    stepAnalysisView(view, 1.15); // 距上次 ≥90ms，推进一行
    expect(headAfterFirst).not.toBe(headBefore); // 首次 step 立即建行
    expect(view.historyHead).toBe((headAfterFirst + 1) % view.history.length);
  });

  it("decays spectrum toward the floor when frames stop arriving", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(view, frame({ spectrum: new Array(ANALYSIS_BIN_COUNT).fill(1) }), 1);
    stepAnalysisView(view, 1.016);
    const liveDb = view.spectrumDb[5];

    // 断流 2 秒：显示值应明显滑向地板
    let now = 1.3;
    for (let i = 0; i < 120; i += 1) {
      now += 0.016;
      stepAnalysisView(view, now);
    }
    expect(view.spectrumDb[5]).toBeLessThan(liveDb - 20);
    expect(view.spectrumDb[5]).toBeGreaterThanOrEqual(SPECTRUM_DB_FLOOR);
  });

  it("keeps session values on null frames and clears them on session reset", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(
      view,
      frame({ integratedLufs: -14.2, loudnessRangeLu: 6.1, truePeakMaxDb: -0.4 }),
      1
    );
    // 后续帧字段为 null（数据不足）时保持上次值
    applyAnalysisFrame(view, frame(), 1.05);
    expect(view.loud.i).toBeCloseTo(-14.2, 5);
    expect(view.loud.tpMax).toBeCloseTo(-0.4, 5);

    resetAnalysisSession(view);
    expect(view.loud.i).toBe(null);
    expect(view.loud.lra).toBe(null);
    expect(view.loud.tpMax).toBe(null);
  });
});

describe("analysis view v0.4.8 additions", () => {
  it("splits interleaved i16 waveform into L/R and derives the window length", () => {
    const view = createAnalysisView();
    // 交错 [L=+满幅, R=-半幅] × 4 点
    const waveform = [32767, -16384, 32767, -16384, 32767, -16384, 32767, -16384];
    applyAnalysisFrame(view, frame({ waveform, sampleRate: 48000 }), 1);

    expect(view.wave.points).toBe(4);
    expect(view.wave.l[0]).toBeCloseTo(1, 3);
    expect(view.wave.r[0]).toBeCloseTo(-0.5, 2);
    // 每点代表 2 帧（后端抽取步长）→ 窗口 = 8 帧 / 48000
    expect(view.wave.windowSec).toBeCloseTo(8 / 48000, 8);
  });

  it("drives the VU needle with ~300ms ballistics toward the rms level", () => {
    const view = createAnalysisView();
    // 0 VU = -18 dBFS 的稳态正弦 RMS
    const rms = Math.pow(10, -18 / 20);
    let now = 1;
    // 60ms：表针应仍明显低于目标
    for (let i = 0; i < 2; i += 1) {
      now += 0.03;
      applyAnalysisFrame(view, frame({ rmsLeft: rms, peakLeft: rms }), now);
    }
    expect(view.levels.l.vuDb).toBeLessThan(-19);
    // 再 600ms：应收敛到 -18 dBFS 附近
    for (let i = 0; i < 20; i += 1) {
      now += 0.03;
      applyAnalysisFrame(view, frame({ rmsLeft: rms, peakLeft: rms }), now);
    }
    expect(view.levels.l.vuDb).toBeGreaterThan(-18.5);
    expect(view.levels.l.vuDb).toBeLessThan(-17.5);
  });

  it("bumps historyVersion each time the spectrogram advances a row", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(view, frame({ spectrum: new Array(ANALYSIS_BIN_COUNT).fill(1) }), 1);
    const before = view.historyVersion;
    stepAnalysisView(view, 1.0);
    stepAnalysisView(view, 1.05); // 未到 90ms 不推进
    const afterFirst = view.historyVersion;
    stepAnalysisView(view, 1.15);
    expect(afterFirst).toBe(before + 1);
    expect(view.historyVersion).toBe(before + 2);
  });

  it("fades the waveform toward zero when frames stop arriving", () => {
    const view = createAnalysisView();
    applyAnalysisFrame(view, frame({ waveform: [32767, 32767] }), 1);
    stepAnalysisView(view, 1.016);
    let now = 1.3;
    for (let i = 0; i < 120; i += 1) {
      now += 0.016;
      stepAnalysisView(view, now);
    }
    expect(Math.abs(view.wave.l[0])).toBeLessThan(0.05);
  });
});
