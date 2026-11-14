import { describe, expect, it } from "vitest";
import { smoothingFactor } from "./smoothing";

/** 以固定帧率向目标 1 逼近 `seconds` 秒后的值。 */
function settle(fps: number, seconds: number, tau: number) {
  let value = 0;
  for (let frame = 0; frame < Math.round(fps * seconds); frame += 1) {
    value += (1 - value) * smoothingFactor(1 / fps, tau);
  }
  return value;
}

describe("smoothingFactor", () => {
  it("同样的时间内，60Hz 与 144Hz 走到同一个位置", () => {
    const at60 = settle(60, 0.1, 0.067);
    const at144 = settle(144, 0.1, 0.067);
    expect(Math.abs(at60 - at144)).toBeLessThan(0.01);
  });

  it("侧栏频谱的时间常数在 60Hz 下等效于原先的每帧系数", () => {
    expect(smoothingFactor(1 / 60, 0.024)).toBeCloseTo(0.5, 2);
    expect(smoothingFactor(1 / 60, 0.067)).toBeCloseTo(0.22, 2);
  });

  it("非法输入返回 0，长间隔被封顶", () => {
    expect(smoothingFactor(0, 0.05)).toBe(0);
    expect(smoothingFactor(-1, 0.05)).toBe(0);
    expect(smoothingFactor(Number.NaN, 0.05)).toBe(0);
    expect(smoothingFactor(0.1, 0)).toBe(0);
    expect(smoothingFactor(60, 1)).toBe(smoothingFactor(0.25, 1));
  });
});
