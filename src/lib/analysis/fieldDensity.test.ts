import { describe, expect, it } from "vitest";
import {
  FIELD_POINT_TIERS,
  fieldPointBudget,
  immersiveFieldPointSize,
  pageFieldPointSize,
} from "./fieldDensity";

describe("声场散点密度", () => {
  it("普通尺寸落在最低档，画布越大档位越高", () => {
    expect(FIELD_POINT_TIERS).toEqual([480, 960, 1440]);
    expect(fieldPointBudget(100)).toBe(480);
    expect(fieldPointBudget(151)).toBe(480);
    expect(fieldPointBudget(152)).toBe(960);
    expect(fieldPointBudget(200)).toBe(960);
    expect(fieldPointBudget(252)).toBe(1440);
    expect(fieldPointBudget(900)).toBe(1440);
  });

  it("换档带回差，尺寸在分界线附近抖动时不来回切", () => {
    // 分界线 140：升档要到 152，降档要低于 128
    expect(fieldPointBudget(145, 480)).toBe(480);
    expect(fieldPointBudget(145, 960)).toBe(960);
    expect(fieldPointBudget(127, 960)).toBe(480);
    // 分界线 240
    expect(fieldPointBudget(245, 960)).toBe(960);
    expect(fieldPointBudget(245, 1440)).toBe(1440);
    expect(fieldPointBudget(227, 1440)).toBe(960);
    // 一次跨两档
    expect(fieldPointBudget(60, 1440)).toBe(480);
    expect(fieldPointBudget(400, 480)).toBe(1440);
  });

  it("尺寸无效时保持当前档位，未知的档位按最低档起算", () => {
    expect(fieldPointBudget(0, 960)).toBe(960);
    expect(fieldPointBudget(Number.NaN, 1440)).toBe(1440);
    expect(fieldPointBudget(-5)).toBe(480);
    expect(fieldPointBudget(100, 777)).toBe(480);
  });

  it("点径随画布放大并封顶；沉浸页在原上限以内与之前相同", () => {
    expect(immersiveFieldPointSize(50)).toBe(1.1);
    expect(immersiveFieldPointSize(110)).toBeCloseTo(1.1);
    // 此前的公式是 clamp(size / 110, 1.1, 1.75)
    for (const extent of [121, 150, 192]) {
      expect(immersiveFieldPointSize(extent)).toBeCloseTo(Math.min(1.75, extent / 110), 2);
    }
    expect(immersiveFieldPointSize(220)).toBeCloseTo(2);
    expect(immersiveFieldPointSize(500)).toBe(2.5);

    expect(pageFieldPointSize(100)).toBe(2.2);
    expect(pageFieldPointSize(132)).toBeCloseTo(2.2);
    expect(pageFieldPointSize(180)).toBeCloseTo(3);
    expect(pageFieldPointSize(400)).toBe(3.4);
  });

  it("两头一起补之后，大画布的墨量密度不低于普通尺寸", () => {
    // 密度 ∝ 点数 × 点径² ÷ 画布面积；以半径 100 px 为基准
    const density = (extent: number) =>
      (fieldPointBudget(extent) * immersiveFieldPointSize(extent) ** 2) / extent ** 2;
    const base = density(100);
    for (const extent of [160, 200, 260, 320, 380]) {
      expect(density(extent) / base).toBeGreaterThan(0.95);
    }
    // 只放大画布、什么都不补时的情形：半径 300 px 只剩九分之一
    expect((480 * 1.1 ** 2) / 300 ** 2 / base).toBeLessThan(0.12);
  });
});
