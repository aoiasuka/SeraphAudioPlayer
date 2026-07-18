import { describe, expect, it } from "vitest";
import { parseApoPreset, toApoText } from "@/lib/eqApoParser";

const AUTOEQ_PARAMETRIC = `Preamp: -6.4 dB
Filter 1: ON LSC Fc 105 Hz Gain 2.4 dB Q 0.70
Filter 2: ON PK Fc 31 Hz Gain 5.8 dB Q 0.70
Filter 3: ON PK Fc 1359 Hz Gain -1.9 dB Q 1.20
Filter 4: OFF PK Fc 5000 Hz Gain 0.0 dB Q 1.00
Filter 5: ON HSC Fc 10000 Hz Gain -4.5 dB Q 0.70
`;

describe("EqualizerAPO / AutoEq 预设解析", () => {
  it("解析 ParametricEQ：preamp、类型映射、OFF 行跳过", () => {
    const result = parseApoPreset(AUTOEQ_PARAMETRIC);

    expect(result.preamp).toBeCloseTo(-6.4);
    expect(result.bands).toHaveLength(4);
    expect(result.bands[0]).toMatchObject({
      kind: "lowshelf",
      freq: 105,
      gain: 2.4,
      q: 0.7,
      enabled: true,
    });
    expect(result.bands[1].kind).toBe("peaking");
    expect(result.bands[2].gain).toBeCloseTo(-1.9);
    expect(result.bands[3].kind).toBe("highshelf");
  });

  it("解析 GraphicEQ 行为一组 peaking 频段", () => {
    const result = parseApoPreset(
      "GraphicEQ: 20 -1.5; 100 2.0; 1000 -3.0; 10000 1.0"
    );

    expect(result.bands).toHaveLength(4);
    expect(result.bands.every((band) => band.kind === "peaking")).toBe(true);
    expect(result.bands[1].freq).toBe(100);
    expect(result.bands[1].gain).toBeCloseTo(2.0);
    expect(result.bands.every((band) => band.q > 0 && band.q <= 10)).toBe(true);
  });

  it("无有效频段时抛错", () => {
    expect(() => parseApoPreset("hello world\nnothing here")).toThrow();
    expect(() => parseApoPreset("")).toThrow();
  });

  it("容忍大小写与多余空白", () => {
    const result = parseApoPreset(
      "preamp: -3 dB\nfilter 1: ON pk Fc 250 Hz Gain 3.5 dB Q 1.41"
    );
    expect(result.preamp).toBeCloseTo(-3);
    expect(result.bands[0]).toMatchObject({ kind: "peaking", freq: 250 });
  });

  it("toApoText 输出可被自身解析（roundtrip）", () => {
    const original = parseApoPreset(AUTOEQ_PARAMETRIC);
    const text = toApoText(original.preamp, original.bands);
    const back = parseApoPreset(text);

    expect(back.preamp).toBeCloseTo(original.preamp, 1);
    expect(back.bands).toHaveLength(original.bands.length);
    for (const [index, band] of back.bands.entries()) {
      expect(band.kind).toBe(original.bands[index].kind);
      expect(band.freq).toBeCloseTo(original.bands[index].freq, 0);
      expect(band.gain).toBeCloseTo(original.bands[index].gain, 1);
    }
  });
});
