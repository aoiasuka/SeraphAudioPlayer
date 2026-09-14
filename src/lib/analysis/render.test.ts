// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { prepCanvas } from "./render";

afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function canvasWithContext() {
  const canvas = document.createElement("canvas");
  const setTransform = vi.fn();
  vi.spyOn(canvas, "getContext").mockReturnValue({ setTransform } as unknown as CanvasRenderingContext2D);
  return { canvas, setTransform };
}

describe("分析画布的像素密度", () => {
  it("125% 系统缩放下，小声场仍提供至少两倍绘制精度", () => {
    vi.stubGlobal("devicePixelRatio", 1.25);
    const { canvas } = canvasWithContext();
    prepCanvas(canvas, { w: 300, h: 140 }, 2);
    expect(canvas.width).toBe(600);
    expect(canvas.height).toBe(280);
  });

  it("高分屏使用原生精度，不受两倍下限约束", () => {
    vi.stubGlobal("devicePixelRatio", 3);
    const { canvas } = canvasWithContext();
    prepCanvas(canvas, { w: 300, h: 140 }, 2);
    expect(canvas.width).toBe(900);
    expect(canvas.height).toBe(420);
  });

  it("小数网格尺寸精确映射到完整像素边界，避免二次拉伸", () => {
    vi.stubGlobal("devicePixelRatio", 1.25);
    const { canvas, setTransform } = canvasWithContext();
    const size = { w: 301.3, h: 139.7 };
    prepCanvas(canvas, size);
    const [scaleX, , , scaleY] = setTransform.mock.calls[0];
    expect(scaleX * size.w).toBeCloseTo(canvas.width, 10);
    expect(scaleY * size.h).toBeCloseTo(canvas.height, 10);
  });

  it("尺寸不变时复用像素缓冲，跨缩放比例后重建", () => {
    vi.stubGlobal("devicePixelRatio", 1.25);
    const { canvas } = canvasWithContext();
    const setWidth = vi.spyOn(canvas, "width", "set");
    const setHeight = vi.spyOn(canvas, "height", "set");
    const size = { w: 300, h: 140 };
    prepCanvas(canvas, size, 2);
    prepCanvas(canvas, size, 2);
    expect(setWidth).toHaveBeenCalledTimes(1);
    expect(setHeight).toHaveBeenCalledTimes(1);
    vi.stubGlobal("devicePixelRatio", 3);
    prepCanvas(canvas, size, 2);
    expect(setWidth).toHaveBeenCalledTimes(2);
    expect(setHeight).toHaveBeenCalledTimes(2);
    expect(canvas.width).toBe(900);
    expect(canvas.height).toBe(420);
  });
});
