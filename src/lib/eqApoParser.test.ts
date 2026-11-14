import { describe, expect, it } from "vitest";
import { GRAPHIC_EQ_MAX_BANDS, parseApoPreset, toApoText } from "@/lib/eqApoParser";
import { combinedResponseDb, logFreqPoints } from "@/lib/eqResponse";

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

  it("解析 GraphicEQ 行：两端搁架、中间 peaking，合成响应贴合原曲线", () => {
    const result = parseApoPreset(
      "GraphicEQ: 20 -1.5; 100 2.0; 1000 -3.0; 10000 1.0"
    );

    expect(result.bands).toHaveLength(4);
    expect(result.bands.map((band) => band.kind)).toEqual(["lowshelf", "peaking", "peaking", "highshelf"]);
    expect(result.bands[1].freq).toBe(100);
    expect(result.bands.every((band) => band.q > 0 && band.q <= 10)).toBe(true);
    expect(result.bands.every((band) => Math.abs(band.gain) <= 24)).toBe(true);
    // 4 个点相隔一个十倍频程，4 个滤波器只能给出折中的近似（真实预设点密得多，见下一例）
    const response = combinedResponseDb(0, result.bands, [100, 1000]);
    expect(Math.abs(response[0] - 2.0)).toBeLessThan(1.5);
    expect(Math.abs(response[1] + 3.0)).toBeLessThan(1.5);
  });

  const tilt = (freq: number) => 4 - 10 * (Math.log10(freq / 20) / 3);
  const graphicText = (curve: (freq: number) => number, count = 127) =>
    `GraphicEQ: ${logFreqPoints(count)
      .map((freq) => `${freq.toFixed(1)} ${curve(freq).toFixed(2)}`)
      .join("; ")}`;

  it("超长 GraphicEQ 在全频段重采样而不是截断低频（EQ4-01）", () => {
    // 127 点、20 Hz–20 kHz 对数分布：低频 +4、高频 −6 的倾斜曲线
    const result = parseApoPreset(graphicText(tilt));

    expect(result.bands).toHaveLength(GRAPHIC_EQ_MAX_BANDS);
    expect(result.bands[0].freq).toBeLessThan(25);
    expect(result.bands[result.bands.length - 1].freq).toBeGreaterThan(15_000);
    expect(result.warnings[0]).toContain("重采样");
    // 中高频也被校正：合成响应在常见采样率下都贴合原曲线
    const probes = logFreqPoints(120).filter((freq) => freq <= 8000);
    for (const sampleRate of [44_100, 48_000, 96_000]) {
      const response = combinedResponseDb(0, result.bands, probes, sampleRate);
      probes.forEach((freq, index) => {
        expect(Math.abs(response[index] - tilt(freq))).toBeLessThan(0.75);
      });
      const [top] = combinedResponseDb(0, result.bands, [14_000], sampleRate);
      expect(Math.abs(top - tilt(14_000))).toBeLessThan(2);
    }
  });

  it("平坦的 GraphicEQ 曲线不会被重叠滤波器叠深（EQ4-02）", () => {
    const result = parseApoPreset(graphicText(() => -5, 31));
    const response = combinedResponseDb(0, result.bands, [50, 100, 1000, 5000]);
    for (const value of response) expect(Math.abs(value + 5)).toBeLessThan(0.75);
  });

  it("导出保留一位小数的频率", () => {
    const text = toApoText(0, [{ kind: "peaking", freq: 31.5, gain: 1, q: 1, enabled: true }]);
    expect(text).toContain("Fc 31.5 Hz");
    expect(parseApoPreset(text).bands[0].freq).toBe(31.5);
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
