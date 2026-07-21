import type { AnalysisPanelId } from "@/store/analysisSettings";

/**
 * 声学分析自由布局引擎：12×12 视口分数网格（一格 = 主区宽/高的 1/12，
 * 持久化值与设备无关）。纯函数进出，全部可在 node 环境直测。
 *
 * 语义与 Grafana 类仪表盘一致：拖拽移动 / 角部缩放 → 被压面板递归下推 →
 * 重力压缩上浮；压缩后仍超出 12 行则本次操作无效（返回 null，调用方保持
 * 上一个有效布局）。渲染路径 resolveLayout 只对可见面板压缩补位，隐藏
 * 面板的 rect 始终保留在持久化里，重新显示自动回到记忆位置。
 */

export const GRID_COLS = 12;
export const GRID_ROWS = 12;

export interface GridRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export type GridLayout = Record<AnalysisPanelId, GridRect>;

/** 各面板最小跨度（列×行）：读数与图形的可读性下限 */
export const PANEL_MIN_SIZE: Record<AnalysisPanelId, { w: number; h: number }> = {
  loudness: { w: 3, h: 4 },
  levels: { w: 3, h: 3 },
  field: { w: 3, h: 4 },
  spectrum: { w: 4, h: 3 },
  scope: { w: 4, h: 2 },
  spectrogram: { w: 4, h: 3 },
};

/** 默认布局 = v0.4.7 三区仪表台在 12×12 下的等价映射（编辑模式「恢复默认」目标） */
export const DEFAULT_GRID_LAYOUT: GridLayout = {
  loudness: { x: 0, y: 0, w: 3, h: 5 },
  levels: { x: 0, y: 5, w: 3, h: 7 },
  spectrum: { x: 3, y: 0, w: 9, h: 4 },
  scope: { x: 3, y: 4, w: 9, h: 3 },
  spectrogram: { x: 3, y: 7, w: 5, h: 5 },
  field: { x: 8, y: 7, w: 4, h: 5 },
};

export const GRID_PANEL_IDS = Object.keys(DEFAULT_GRID_LAYOUT) as AnalysisPanelId[];

export function collides(a: GridRect, b: GridRect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** 取整 + 面板尺寸下限 + 12×12 边界钳制 */
export function clampRect(rect: GridRect, id: AnalysisPanelId): GridRect {
  const min = PANEL_MIN_SIZE[id];
  const w = Math.min(GRID_COLS, Math.max(min.w, Math.round(rect.w)));
  const h = Math.min(GRID_ROWS, Math.max(min.h, Math.round(rect.h)));
  const x = Math.min(GRID_COLS - w, Math.max(0, Math.round(rect.x)));
  const y = Math.min(GRID_ROWS - h, Math.max(0, Math.round(rect.y)));
  return { x, y, w, h };
}

type RectMap = Partial<Record<AnalysisPanelId, GridRect>>;

const byReadingOrder =
  (rects: RectMap) => (a: AnalysisPanelId, b: AnalysisPanelId) => {
    const ra = rects[a]!;
    const rb = rects[b]!;
    return ra.y - rb.y || ra.x - rb.x || a.localeCompare(b);
  };

/**
 * 重力压缩：按 (y,x) 阅读序逐个「先尽量上浮、起点重叠则向下让位」，
 * 与 react-grid-layout 的 compactItem 同语义。locked 面板固定不参与上浮。
 */
function compactRects(rects: RectMap, ids: AnalysisPanelId[], lockedId?: AnalysisPanelId) {
  const placed: GridRect[] = [];
  const locked = lockedId ? rects[lockedId] : undefined;
  if (locked) placed.push(locked);
  const sorted = ids.filter((id) => id !== lockedId).sort(byReadingOrder(rects));
  for (const id of sorted) {
    let rect = rects[id]!;
    while (
      rect.y > 0 &&
      !placed.some((other) => collides({ ...rect, y: rect.y - 1 }, other))
    ) {
      rect = { ...rect, y: rect.y - 1 };
    }
    while (placed.some((other) => collides(rect, other))) {
      rect = { ...rect, y: rect.y + 1 };
    }
    rects[id] = rect;
    placed.push(rect);
  }
}

function maxBottom(rects: RectMap, ids: AnalysisPanelId[]): number {
  let bottom = 0;
  for (const id of ids) {
    const rect = rects[id]!;
    bottom = Math.max(bottom, rect.y + rect.h);
  }
  return bottom;
}

/** 编辑操作共用：id 固定在 rect，被压面板递归下推 → 压缩 → 超出 12 行判无效 */
function settle(
  layout: GridLayout,
  visible: AnalysisPanelId[],
  id: AnalysisPanelId,
  rect: GridRect
): GridLayout | null {
  const rects: RectMap = {};
  for (const key of visible) rects[key] = { ...layout[key] };
  rects[id] = rect;

  const queue: AnalysisPanelId[] = [id];
  let guard = 0;
  while (queue.length > 0) {
    if (++guard > 64) return null;
    const cur = queue.shift()!;
    const curRect = rects[cur]!;
    for (const other of visible) {
      if (other === cur || other === id) continue;
      const otherRect = rects[other]!;
      if (collides(curRect, otherRect)) {
        rects[other] = { ...otherRect, y: curRect.y + curRect.h };
        queue.push(other);
      }
    }
  }

  compactRects(rects, visible, id);
  if (maxBottom(rects, visible) > GRID_ROWS) return null;

  const next: GridLayout = { ...layout };
  for (const key of visible) next[key] = rects[key]!;
  return next;
}

/** 拖拽移动：目标位置钳制进界后放置。无效（推不开）返回 null */
export function moveItem(
  layout: GridLayout,
  visible: AnalysisPanelId[],
  id: AnalysisPanelId,
  x: number,
  y: number
): GridLayout | null {
  const cur = layout[id];
  const rect = clampRect({ ...cur, x, y }, id);
  if (rect.x === cur.x && rect.y === cur.y) return layout;
  return settle(layout, visible, id, rect);
}

/** 角部缩放：左上角固定，改 w/h。无效返回 null */
export function resizeItem(
  layout: GridLayout,
  visible: AnalysisPanelId[],
  id: AnalysisPanelId,
  w: number,
  h: number
): GridLayout | null {
  const cur = layout[id];
  const rect = clampRect({ ...cur, w, h }, id);
  if (rect.w === cur.w && rect.h === cur.h) return layout;
  return settle(layout, visible, id, rect);
}

/**
 * 渲染路径：可见子集压缩补位后的实际展示位置。
 * 对脏数据（重叠/越界）自愈；压缩后仍超界则底边钳回（不破版兜底）。
 */
export function resolveLayout(
  layout: GridLayout,
  visible: AnalysisPanelId[]
): Partial<Record<AnalysisPanelId, GridRect>> {
  const rects: RectMap = {};
  for (const id of visible) rects[id] = clampRect(layout[id], id);
  compactRects(rects, visible);
  for (const id of visible) {
    const rect = rects[id]!;
    if (rect.y + rect.h > GRID_ROWS) {
      rects[id] = { ...rect, y: Math.max(0, GRID_ROWS - rect.h) };
    }
  }
  return rects;
}

/** 窄屏纵向堆叠顺序：按宽屏布局的 (y,x) 阅读序 */
export function orderForNarrow(
  layout: GridLayout,
  visible: AnalysisPanelId[]
): AnalysisPanelId[] {
  return [...visible].sort(byReadingOrder(layout));
}

/**
 * 持久化/导入校验：六面板齐备且 x/y/w/h 均为有限数则钳制进界返回，
 * 否则整体判废（调用方回退 auto 布局）。轻微重叠不拒收——resolveLayout 自愈。
 */
export function sanitizeGridLayout(value: unknown): GridLayout | null {
  if (!value || typeof value !== "object") return null;
  const source = value as Record<string, unknown>;
  const result = {} as GridLayout;
  for (const id of GRID_PANEL_IDS) {
    const raw = source[id];
    if (!raw || typeof raw !== "object") return null;
    const rect = raw as Record<string, unknown>;
    const { x, y, w, h } = rect;
    if (
      typeof x !== "number" ||
      typeof y !== "number" ||
      typeof w !== "number" ||
      typeof h !== "number" ||
      ![x, y, w, h].every(Number.isFinite)
    ) {
      return null;
    }
    result[id] = clampRect({ x, y, w, h }, id);
  }
  return result;
}
