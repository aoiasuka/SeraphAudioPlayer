import { describe, expect, it } from "vitest";
import {
  MAX_EQ_BANDS,
  sanitizeCrossfeed,
  sanitizeEqBand,
  sanitizeEqBandPatch,
  sanitizeEqBands,
  sanitizeEqPreamp,
  sanitizeUserPresets,
} from "./eqSanitize";

const validBand = {
  kind: "peaking",
  freq: 1000,
  gain: 3,
  q: 1.2,
  enabled: true,
};

describe("sanitizeEqPreamp", () => {
  it("放行合法值并钳制越界", () => {
    expect(sanitizeEqPreamp(-6.5)).toBe(-6.5);
    expect(sanitizeEqPreamp(999)).toBe(24);
    expect(sanitizeEqPreamp(-999)).toBe(-24);
  });

  it("NaN / 字符串垃圾 / undefined 回落 0（H-1 白屏链源头）", () => {
    expect(sanitizeEqPreamp(Number.NaN)).toBe(0);
    expect(sanitizeEqPreamp("abc")).toBe(0);
    expect(sanitizeEqPreamp(undefined)).toBe(0);
    expect(sanitizeEqPreamp(null)).toBe(0);
    expect(sanitizeEqPreamp(Infinity)).toBe(0);
  });

  it("数字字符串宽容转换", () => {
    expect(sanitizeEqPreamp("-3.5")).toBe(-3.5);
  });
});

describe("sanitizeEqBand", () => {
  it("合法频段原样通过", () => {
    expect(sanitizeEqBand(validBand)).toEqual(validBand);
  });

  it("freq 无效整段判废", () => {
    expect(sanitizeEqBand({ ...validBand, freq: Number.NaN })).toBeNull();
    expect(sanitizeEqBand({ ...validBand, freq: "x" })).toBeNull();
    expect(sanitizeEqBand({ ...validBand, freq: undefined })).toBeNull();
    expect(sanitizeEqBand(null)).toBeNull();
    expect(sanitizeEqBand("not a band")).toBeNull();
  });

  it("gain/q 无效回落默认，kind 未知回落 peaking", () => {
    expect(sanitizeEqBand({ freq: 100, gain: Number.NaN, q: -1, kind: "weird" })).toEqual({
      kind: "peaking",
      freq: 100,
      gain: 0,
      q: 0.01,
      enabled: true,
    });
  });

  it("freq/gain 钳制到合法区间", () => {
    const band = sanitizeEqBand({ ...validBand, freq: -50, gain: 100 });
    expect(band).toMatchObject({ freq: 1, gain: 24 });
  });
});

describe("sanitizeEqBands", () => {
  it("无效项丢弃、有效项保留", () => {
    const bands = sanitizeEqBands([validBand, { freq: Number.NaN }, null, { ...validBand, freq: 200 }]);
    expect(bands).toHaveLength(2);
    expect(bands?.[1]?.freq).toBe(200);
  });

  it("非数组或全无效返回 null", () => {
    expect(sanitizeEqBands(undefined)).toBeNull();
    expect(sanitizeEqBands("junk")).toBeNull();
    expect(sanitizeEqBands([{ freq: "bad" }])).toBeNull();
    expect(sanitizeEqBands([])).toBeNull();
  });

  it("截断到 MAX_EQ_BANDS 上限", () => {
    const many = Array.from({ length: MAX_EQ_BANDS + 20 }, (_, i) => ({
      ...validBand,
      freq: 20 + i,
    }));
    expect(sanitizeEqBands(many)).toHaveLength(MAX_EQ_BANDS);
  });
});

describe("sanitizeEqBandPatch", () => {
  it("清空输入框产生的 NaN 字段被丢弃（等价于不修改）", () => {
    expect(sanitizeEqBandPatch({ freq: Number.NaN })).toEqual({});
    expect(sanitizeEqBandPatch({ gain: Number.NaN, q: 2 })).toEqual({ q: 2 });
  });

  it("合法字段钳制后通过", () => {
    expect(sanitizeEqBandPatch({ freq: 500, gain: 99 })).toEqual({
      freq: 500,
      gain: 24,
    });
  });

  it("未知 kind 丢弃", () => {
    expect(sanitizeEqBandPatch({ kind: "nope" as never })).toEqual({});
    expect(sanitizeEqBandPatch({ kind: "lowshelf" })).toEqual({ kind: "lowshelf" });
  });
});

describe("sanitizeCrossfeed", () => {
  const fallback = { enabled: false, amount: 0.3, cutoffHz: 700 };

  it("坏结构回落默认", () => {
    expect(sanitizeCrossfeed(null, fallback)).toEqual(fallback);
    expect(sanitizeCrossfeed("x", fallback)).toEqual(fallback);
  });

  it("数值字段无效则回落、合法则钳制", () => {
    expect(
      sanitizeCrossfeed({ enabled: true, amount: Number.NaN, cutoffHz: 5 }, fallback)
    ).toEqual({ enabled: true, amount: 0.3, cutoffHz: 20 });
  });
});

describe("sanitizeUserPresets", () => {
  it("结构坏 / 频段全废的预设整条丢弃", () => {
    const presets = sanitizeUserPresets([
      { id: "a", name: "好预设", preamp: -3, bands: [validBand], createdAt: 1 },
      { id: "b", name: "坏预设", preamp: 0, bands: [{ freq: "x" }], createdAt: 2 },
      { name: "缺 id", preamp: 0, bands: [validBand] },
      null,
    ]);
    expect(presets).toHaveLength(1);
    expect(presets[0].id).toBe("a");
  });

  it("非数组返回空表", () => {
    expect(sanitizeUserPresets(undefined)).toEqual([]);
  });
});
