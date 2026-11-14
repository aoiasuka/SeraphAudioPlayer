import { describe, expect, it } from "vitest";
import { volumeToGain } from "./volume";

describe("volumeToGain（OUT5-02）", () => {
  it("两端不变：0 是静音，1 是满幅", () => {
    expect(volumeToGain(0)).toBe(0);
    expect(volumeToGain(1)).toBe(1);
  });

  it("中段按立方衰减：50% 约 −18 dB", () => {
    expect(20 * Math.log10(volumeToGain(0.5))).toBeCloseTo(-18.06, 1);
    expect(volumeToGain(0.25)).toBeLessThan(volumeToGain(0.5));
  });

  it("越界与非法值被收敛", () => {
    expect(volumeToGain(2)).toBe(1);
    expect(volumeToGain(-1)).toBe(0);
    expect(volumeToGain(Number.NaN)).toBe(0);
  });
});
