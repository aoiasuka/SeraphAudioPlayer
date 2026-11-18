import { describe, expect, it } from "vitest";
import {
  DEFAULT_EQ_BANDS,
  GENRE_EQ_PRESETS,
  clipRisk,
  headroomPreamp,
} from "@/lib/eqPresets";
import type { EqBand } from "@/types/dsp";

const peaking = (freq: number, gain: number, q = 1): EqBand => ({
  kind: "peaking",
  freq,
  gain,
  q,
  enabled: true,
});

describe("clipRisk（DSP-04）", () => {
  it("平直 EQ、预放大 0 dB 没有削波风险", () => {
    const risk = clipRisk(0, DEFAULT_EQ_BANDS);
    expect(risk.clipping).toBe(false);
    expect(risk.overDb).toBe(0);
    expect(risk.suggestedPreamp).toBe(0);
  });

  it("单段 +6 dB 且预放大留在 0 dB：提示超出约 6 dB，建议 -6 dB", () => {
    const risk = clipRisk(0, [peaking(1000, 6)]);
    expect(risk.clipping).toBe(true);
    expect(risk.overDb).toBeGreaterThan(5.9);
    expect(risk.overDb).toBeLessThan(6.1);
    expect(risk.suggestedPreamp).toBe(-6);
  });

  it("预放大已经压够就不提示；比建议值多压也不提示", () => {
    const bands = [peaking(1000, 6)];
    expect(clipRisk(-6, bands).clipping).toBe(false);
    expect(clipRisk(-9, bands).clipping).toBe(false);
    expect(clipRisk(-5.5, bands).clipping).toBe(true);
  });

  it("没有提升的频段，但预放大是正的：同样会削波，建议回到 0 dB", () => {
    const risk = clipRisk(3, [peaking(1000, -4)]);
    expect(risk.clipping).toBe(true);
    expect(risk.overDb).toBeCloseTo(3, 1);
    expect(risk.suggestedPreamp).toBe(0);
  });

  it("停用的频段不计入", () => {
    const risk = clipRisk(0, [{ ...peaking(1000, 12), enabled: false }]);
    expect(risk.clipping).toBe(false);
  });

  it("相邻频段重叠时按合成响应的峰值算，而不是单段最大增益", () => {
    const bands = [peaking(800, 6), peaking(1000, 6), peaking(1250, 6)];
    const risk = clipRisk(0, bands);
    expect(risk.overDb).toBeGreaterThan(12);
    expect(risk.suggestedPreamp).toBe(headroomPreamp(bands));
    expect(clipRisk(risk.suggestedPreamp, bands).clipping).toBe(false);
  });

  it("建议值不会超出预放大的可调范围（-24 dB）", () => {
    const bands = [peaking(900, 24), peaking(1000, 24), peaking(1100, 24)];
    const risk = clipRisk(0, bands);
    expect(risk.clipping).toBe(true);
    expect(risk.suggestedPreamp).toBe(-24);
  });

  it("内置曲风预设自带的预放大都不触发提示", () => {
    for (const preset of GENRE_EQ_PRESETS) {
      expect(clipRisk(preset.preamp, preset.bands).clipping, preset.id).toBe(false);
    }
  });

  it("坏的预放大值按 0 处理，不抛错", () => {
    expect(clipRisk(Number.NaN, [peaking(1000, 6)]).suggestedPreamp).toBe(-6);
  });
});
