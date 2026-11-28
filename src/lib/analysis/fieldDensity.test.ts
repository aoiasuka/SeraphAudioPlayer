import { describe, expect, it } from "vitest";
import {
  IMMERSIVE_FIELD_POINTS,
  IMMERSIVE_FIELD_TRAIL_ALPHA,
  PAGE_FIELD_POINTS,
  fieldPixelRatio,
  immersiveFieldPointSize,
  pageFieldPointSize,
} from "./fieldDensity";

describe("声场散点密度", () => {
  it("每帧采样点数是固定值，不随画布分档", () => {
    // 与后端 `seraph_visualizer::analysis` 的上限一致；超过上限后端会钳住
    expect(IMMERSIVE_FIELD_POINTS).toBe(1440);
    expect(PAGE_FIELD_POINTS).toBe(960);
  });

  it("余晖 10 帧、逐帧按 0.8 衰减，总墨量约 3.1 帧", () => {
    expect(IMMERSIVE_FIELD_TRAIL_ALPHA).toHaveLength(10);
    expect(IMMERSIVE_FIELD_TRAIL_ALPHA[0]).toBe(0.7);
    for (let age = 1; age < IMMERSIVE_FIELD_TRAIL_ALPHA.length; age += 1) {
      const ratio = IMMERSIVE_FIELD_TRAIL_ALPHA[age] / IMMERSIVE_FIELD_TRAIL_ALPHA[age - 1];
      expect(ratio).toBeCloseTo(0.8, 1);
    }
    const total = IMMERSIVE_FIELD_TRAIL_ALPHA.reduce((sum, alpha) => sum + alpha, 0);
    expect(total).toBeGreaterThan(3);
    expect(total).toBeLessThan(3.3);
  });

  it("点径有下限，随画布放大并封顶", () => {
    // 2560×1440、缩放 150% 下沉浸页的实测半径约 110 px：正好落在下限
    expect(immersiveFieldPointSize(60)).toBe(1.4);
    expect(immersiveFieldPointSize(109)).toBeCloseTo(1.4, 2);
    expect(immersiveFieldPointSize(156)).toBeCloseTo(2);
    expect(immersiveFieldPointSize(500)).toBe(2.6);

    expect(pageFieldPointSize(100)).toBe(2.2);
    expect(pageFieldPointSize(180)).toBeCloseTo(3);
    expect(pageFieldPointSize(400)).toBe(3.4);
  });

  it("画布像素比不低于 2，并且是设备像素比的整数倍", () => {
    expect(fieldPixelRatio(1)).toBe(2);
    expect(fieldPixelRatio(1.25)).toBe(2.5);
    expect(fieldPixelRatio(1.5)).toBe(3);
    expect(fieldPixelRatio(1.75)).toBe(3.5);
    expect(fieldPixelRatio(2)).toBe(2);
    expect(fieldPixelRatio(3)).toBe(3);
    for (const dpr of [1, 1.1, 1.25, 1.5, 1.75, 2, 2.25, 2.5, 3]) {
      const ratio = fieldPixelRatio(dpr);
      expect(ratio).toBeGreaterThanOrEqual(2);
      expect(Number.isInteger(Math.round((ratio / dpr) * 1e6) / 1e6)).toBe(true);
    }
    // 取不到设备像素比时按 1 处理
    expect(fieldPixelRatio(0)).toBe(2);
    expect(fieldPixelRatio(Number.NaN)).toBe(2);
  });

  it("150% 缩放下每个点正好是整数个设备像素（此前是 1.5 个）", () => {
    const dpr = 1.5;
    const ratio = fieldPixelRatio(dpr);
    // 与 drawImmersiveField 相同的取整：按画布像素取整
    const backingPixels = Math.max(1, Math.round(ratio * immersiveFieldPointSize(109)));
    const devicePixels = backingPixels / (ratio / dpr);
    expect(backingPixels).toBe(4);
    expect(devicePixels).toBe(2);
    // 旧规则：像素比 2、点径 1.1 → 2 个画布像素 = 1.5 个设备像素
    expect(Math.round(2 * 1.1) / (2 / dpr)).toBe(1.5);
  });
});
