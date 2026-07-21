import { describe, expect, it } from "vitest";
import {
  clampRect,
  collides,
  DEFAULT_GRID_LAYOUT,
  GRID_COLS,
  GRID_PANEL_IDS,
  GRID_ROWS,
  moveItem,
  orderForNarrow,
  PANEL_MIN_SIZE,
  resizeItem,
  resolveLayout,
  sanitizeGridLayout,
  type GridLayout,
  type GridRect,
} from "./gridLayout";
import type { AnalysisPanelId } from "@/store/analysisSettings";

const ALL = GRID_PANEL_IDS;

function assertNoOverlap(
  rects: Partial<Record<AnalysisPanelId, GridRect>>,
  ids: AnalysisPanelId[]
) {
  for (let i = 0; i < ids.length; i += 1) {
    for (let j = i + 1; j < ids.length; j += 1) {
      expect(
        collides(rects[ids[i]]!, rects[ids[j]]!),
        `${ids[i]} overlaps ${ids[j]}`
      ).toBe(false);
    }
  }
}

function assertInBounds(
  rects: Partial<Record<AnalysisPanelId, GridRect>>,
  ids: AnalysisPanelId[]
) {
  for (const id of ids) {
    const rect = rects[id]!;
    expect(rect.x).toBeGreaterThanOrEqual(0);
    expect(rect.y).toBeGreaterThanOrEqual(0);
    expect(rect.x + rect.w).toBeLessThanOrEqual(GRID_COLS);
    expect(rect.y + rect.h).toBeLessThanOrEqual(GRID_ROWS);
  }
}

describe("gridLayout engine (v0.4.9)", () => {
  it("default layout covers six panels without overlap inside 12×12", () => {
    expect(ALL).toHaveLength(6);
    assertNoOverlap(DEFAULT_GRID_LAYOUT, ALL);
    assertInBounds(DEFAULT_GRID_LAYOUT, ALL);
    for (const id of ALL) {
      const rect = DEFAULT_GRID_LAYOUT[id];
      expect(rect.w).toBeGreaterThanOrEqual(PANEL_MIN_SIZE[id].w);
      expect(rect.h).toBeGreaterThanOrEqual(PANEL_MIN_SIZE[id].h);
    }
  });

  it("clampRect enforces per-panel minimum size and grid bounds", () => {
    expect(clampRect({ x: 0, y: 0, w: 1, h: 1 }, "spectrum")).toEqual({
      x: 0,
      y: 0,
      w: 4,
      h: 3,
    });
    // 越界位置被拉回；尺寸超出网格被截断
    expect(clampRect({ x: 20, y: 20, w: 30, h: 30 }, "scope")).toEqual({
      x: 0,
      y: 0,
      w: 12,
      h: 12,
    });
    expect(clampRect({ x: 10, y: 10, w: 4, h: 3 }, "spectrum")).toEqual({
      x: 8,
      y: 9,
      w: 4,
      h: 3,
    });
  });

  it("moveItem places a panel at a free spot (locked against gravity)", () => {
    const visible: AnalysisPanelId[] = ["spectrum", "scope"];
    const next = moveItem(DEFAULT_GRID_LAYOUT, visible, "scope", 0, 8);
    expect(next).not.toBeNull();
    expect(next!.scope).toMatchObject({ x: 0, y: 8 });
    // 未参与的隐藏面板 rect 原样保留
    expect(next!.field).toEqual(DEFAULT_GRID_LAYOUT.field);
  });

  it("moveItem pushes the collided panel down (swap effect)", () => {
    const visible: AnalysisPanelId[] = ["spectrum", "scope"];
    const next = moveItem(DEFAULT_GRID_LAYOUT, visible, "scope", 3, 0);
    expect(next).not.toBeNull();
    expect(next!.scope).toMatchObject({ x: 3, y: 0 });
    expect(next!.spectrum).toMatchObject({ x: 3, y: 3 });
    assertNoOverlap(next!, visible);
  });

  it("moveItem returns null when pushed panels would overflow 12 rows", () => {
    const layout: GridLayout = {
      ...DEFAULT_GRID_LAYOUT,
      spectrum: { x: 0, y: 0, w: 12, h: 6 },
      scope: { x: 0, y: 6, w: 12, h: 6 },
    };
    const visible: AnalysisPanelId[] = ["spectrum", "scope"];
    expect(moveItem(layout, visible, "scope", 0, 3)).toBeNull();
  });

  it("resizeItem respects minimum size and pushes neighbours", () => {
    const visible: AnalysisPanelId[] = ["spectrum", "scope"];
    // 缩到 1×1 被钳到最小 4×3（尺寸没变则返回原布局引用）
    const clamped = resizeItem(DEFAULT_GRID_LAYOUT, visible, "spectrum", 1, 1);
    expect(clamped).not.toBeNull();
    expect(clamped!.spectrum).toMatchObject({ w: 4, h: 3 });

    // 加高一行把正下方的示波器推下去
    const grown = resizeItem(DEFAULT_GRID_LAYOUT, visible, "spectrum", 9, 5);
    expect(grown).not.toBeNull();
    expect(grown!.spectrum).toMatchObject({ h: 5 });
    expect(grown!.scope).toMatchObject({ y: 5 });
    assertNoOverlap(grown!, visible);
  });

  it("resizeItem returns null when growth cannot fit", () => {
    const layout: GridLayout = {
      ...DEFAULT_GRID_LAYOUT,
      spectrum: { x: 0, y: 0, w: 12, h: 6 },
      scope: { x: 0, y: 6, w: 12, h: 6 },
    };
    expect(
      resizeItem(layout, ["spectrum", "scope"], "spectrum", 12, 7)
    ).toBeNull();
  });

  it("resolveLayout compacts the visible subset upward when panels hide", () => {
    const visible: AnalysisPanelId[] = ["scope", "spectrogram"];
    const rects = resolveLayout(DEFAULT_GRID_LAYOUT, visible);
    // 频谱隐藏：示波器从 y4 浮到 y0，瀑布贴上来
    expect(rects.scope).toMatchObject({ x: 3, y: 0 });
    expect(rects.spectrogram).toMatchObject({ x: 3, y: 3 });
    assertNoOverlap(rects, visible);
    assertInBounds(rects, visible);
  });

  it("resolveLayout self-heals overlapping dirty data", () => {
    const dirty: GridLayout = {
      ...DEFAULT_GRID_LAYOUT,
      spectrum: { x: 0, y: 0, w: 6, h: 4 },
      scope: { x: 0, y: 0, w: 6, h: 4 },
    };
    const visible: AnalysisPanelId[] = ["spectrum", "scope"];
    const rects = resolveLayout(dirty, visible);
    assertNoOverlap(rects, visible);
    assertInBounds(rects, visible);
  });

  it("orderForNarrow sorts panels by reading order (y, then x)", () => {
    expect(orderForNarrow(DEFAULT_GRID_LAYOUT, [...ALL])).toEqual([
      "loudness",
      "spectrum",
      "scope",
      "levels",
      "spectrogram",
      "field",
    ]);
  });

  it("sanitizeGridLayout accepts the default, clamps stray values, rejects garbage", () => {
    expect(sanitizeGridLayout(DEFAULT_GRID_LAYOUT)).toEqual(DEFAULT_GRID_LAYOUT);

    const stray = {
      ...DEFAULT_GRID_LAYOUT,
      field: { x: 99, y: -5, w: 4, h: 5 },
    };
    const cleaned = sanitizeGridLayout(stray);
    expect(cleaned).not.toBeNull();
    assertInBounds(cleaned!, ALL);

    expect(sanitizeGridLayout(null)).toBeNull();
    expect(sanitizeGridLayout("nope")).toBeNull();
    expect(sanitizeGridLayout({})).toBeNull();
    expect(
      sanitizeGridLayout({
        ...DEFAULT_GRID_LAYOUT,
        scope: { x: Number.NaN, y: 0, w: 9, h: 3 },
      })
    ).toBeNull();
    const { loudness: _dropped, ...missing } = DEFAULT_GRID_LAYOUT;
    expect(sanitizeGridLayout(missing)).toBeNull();
  });
});
