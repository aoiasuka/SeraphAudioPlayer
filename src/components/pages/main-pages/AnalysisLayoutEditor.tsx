import { useEffect, useRef, useState } from "react";
import {
  GRID_COLS,
  GRID_ROWS,
  moveItem,
  resizeItem,
  type GridLayout,
} from "@/lib/analysis/gridLayout";
import { cn } from "@/lib/utils";
import type { AnalysisPanelId } from "@/store/analysisSettings";

const PANEL_INFO: Record<AnalysisPanelId, { no: string; title: string }> = {
  loudness: { no: "NO.01", title: "LOUDNESS" },
  levels: { no: "NO.02", title: "LEVELS" },
  field: { no: "NO.03", title: "SOUND FIELD" },
  spectrum: { no: "NO.04", title: "SPECTRUM" },
  spectrogram: { no: "NO.05", title: "SPECTROGRAM" },
  scope: { no: "NO.06", title: "OSCILLOSCOPE" },
};

const DRAG_THRESHOLD_PX = 4;

interface DragState {
  id: AnalysisPanelId;
  mode: "move" | "resize";
  pointerId: number;
  startX: number;
  startY: number;
  /** move：指针相对面板左上角的格偏移，保证吸附不跳变 */
  grabDx: number;
  grabDy: number;
  /** 每次 move 都从拖拽起点布局重算（无记忆式），Esc/pointercancel 可整体回弹 */
  origLayout: GridLayout;
  active: boolean;
  box: DOMRect;
}

/**
 * 图纸编辑模式：12×12 网格上的六面板图框。拖标题行移动、拖右下角柄缩放，
 * 实时吸附 + 碰撞下推 + 重力压缩预览（moveItem/resizeItem 纯函数）。
 * 受控组件——draft 布局由父级持有，「完成/取消/恢复默认」也在父级页眉。
 */
export function AnalysisLayoutEditor({
  layout,
  visible,
  onChange,
  onRequestCancel,
}: {
  layout: GridLayout;
  visible: AnalysisPanelId[];
  onChange: (layout: GridLayout) => void;
  onRequestCancel: () => void;
}) {
  const boxRef = useRef<HTMLDivElement | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const [dragging, setDragging] = useState<AnalysisPanelId | null>(null);

  const cbRef = useRef({ onChange, onRequestCancel });
  cbRef.current = { onChange, onRequestCancel };

  // Esc：拖拽中回弹取消当次；空闲时请求退出编辑（由父级丢弃 draft）
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      const drag = dragRef.current;
      if (drag) {
        cbRef.current.onChange(drag.origLayout);
        dragRef.current = null;
        setDragging(null);
      } else {
        cbRef.current.onRequestCancel();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const beginDrag =
    (id: AnalysisPanelId, mode: DragState["mode"]) =>
    (event: React.PointerEvent<HTMLElement>) => {
      if (event.button !== 0 || dragRef.current) return;
      const box = boxRef.current?.getBoundingClientRect();
      if (!box || box.width <= 0 || box.height <= 0) return;
      event.preventDefault();
      const cellW = box.width / GRID_COLS;
      const cellH = box.height / GRID_ROWS;
      const rect = layout[id];
      dragRef.current = {
        id,
        mode,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        grabDx: (event.clientX - box.left) / cellW - rect.x,
        grabDy: (event.clientY - box.top) / cellH - rect.y,
        origLayout: layout,
        active: false,
        box,
      };
      try {
        event.currentTarget.setPointerCapture?.(event.pointerId);
      } catch {
        // jsdom 等环境无 pointer capture；桌面 WebView 正常路径不受影响
      }
    };

  const onPointerMove = (event: React.PointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    if (!drag.active) {
      const dist = Math.hypot(
        event.clientX - drag.startX,
        event.clientY - drag.startY
      );
      if (dist < DRAG_THRESHOLD_PX) return;
      drag.active = true;
      setDragging(drag.id);
    }
    const cellW = drag.box.width / GRID_COLS;
    const cellH = drag.box.height / GRID_ROWS;
    const px = (event.clientX - drag.box.left) / cellW;
    const py = (event.clientY - drag.box.top) / cellH;
    const orig = drag.origLayout;
    const next =
      drag.mode === "move"
        ? moveItem(
            orig,
            visible,
            drag.id,
            Math.round(px - drag.grabDx),
            Math.round(py - drag.grabDy)
          )
        : resizeItem(
            orig,
            visible,
            drag.id,
            Math.round(px - orig[drag.id].x),
            Math.round(py - orig[drag.id].y)
          );
    // null = 推不开（会溢出 12 行），保持上一个有效布局
    if (next) onChange(next);
  };

  const endDrag = (event: React.PointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    dragRef.current = null;
    setDragging(null);
  };

  const onPointerCancel = (event: React.PointerEvent<HTMLElement>) => {
    const drag = dragRef.current;
    if (!drag || event.pointerId !== drag.pointerId) return;
    onChange(drag.origLayout);
    dragRef.current = null;
    setDragging(null);
  };

  return (
    <div
      ref={boxRef}
      data-testid="analysis-layout-editor"
      className="relative min-h-0 flex-1 border-[1.5px] border-dashed border-ink2 bg-paper2/50"
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${GRID_COLS}, minmax(0,1fr))`,
        gridTemplateRows: `repeat(${GRID_ROWS}, minmax(0,1fr))`,
        gap: "0.75rem",
        padding: "0.375rem",
        // 图纸坐标网格（氛围线，1/12 周期；淡墨色固定值——项目色板无 CSS 变量）
        backgroundImage:
          "linear-gradient(to right, rgba(43,39,34,0.15) 1px, transparent 1px)," +
          "linear-gradient(to bottom, rgba(43,39,34,0.15) 1px, transparent 1px)",
        backgroundSize: `${100 / GRID_COLS}% ${100 / GRID_ROWS}%`,
      }}
    >
      {visible.map((id) => {
        const rect = layout[id];
        const info = PANEL_INFO[id];
        const active = dragging === id;
        return (
          <div
            key={id}
            data-testid={`layout-box-${id}`}
            style={{
              gridColumn: `${rect.x + 1} / span ${rect.w}`,
              gridRow: `${rect.y + 1} / span ${rect.h}`,
              minWidth: 0,
              minHeight: 0,
            }}
            className={cn(
              "relative flex min-h-0 select-none flex-col border-[1.5px] border-dashed bg-card/90",
              active
                ? "z-10 border-stamp shadow-[3px_3px_0_rgba(166,77,60,0.25)]"
                : "border-ink2"
            )}
            onPointerMove={onPointerMove}
            onPointerUp={endDrag}
            onPointerCancel={onPointerCancel}
          >
            <div
              data-testid={`layout-handle-${id}`}
              className={cn(
                "flex items-center gap-2 border-b border-dashed border-line px-2 py-1",
                active ? "cursor-grabbing" : "cursor-grab"
              )}
              style={{ touchAction: "none" }}
              onPointerDown={beginDrag(id, "move")}
            >
              <span className="shrink-0 bg-ink2 px-1 font-tw text-[9px] font-bold tracking-[1.5px] text-paper">
                {info.no}
              </span>
              <span className="truncate font-tw text-[10px] font-bold tracking-[1.5px] text-ink2">
                {info.title}
              </span>
              <span className="ml-auto shrink-0 font-tw text-[9px] font-bold text-brown [font-variant-numeric:tabular-nums]">
                {rect.w}×{rect.h}
              </span>
            </div>
            <div className="pointer-events-none flex min-h-0 flex-1 items-center justify-center">
              <span className="font-tw text-[clamp(16px,2.4vw,40px)] font-bold tracking-[4px] text-ink3/25">
                {info.no.slice(3)}
              </span>
            </div>
            <div
              role="presentation"
              aria-hidden
              data-testid={`layout-resize-${id}`}
              className={cn(
                "absolute bottom-0 right-0 h-4 w-4 cursor-nwse-resize border-b-[3px] border-r-[3px]",
                active ? "border-stamp" : "border-ink2 hover:border-stamp"
              )}
              style={{ touchAction: "none" }}
              onPointerDown={beginDrag(id, "resize")}
            />
          </div>
        );
      })}
    </div>
  );
}
