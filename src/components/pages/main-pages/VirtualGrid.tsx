import { useLayoutEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";

const GAP = 16;
const OVERSCAN_ROWS = 2;

/** 卡片等高的自适应网格；列数和行高由实际 CSS 布局测量。 */
export function VirtualGrid<T>({ items, itemKey, renderItem, label }: {
  items: T[];
  itemKey: (item: T) => string;
  renderItem: (item: T) => ReactNode;
  label: string;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const gridRef = useRef<HTMLDivElement>(null);
  const pendingFocus = useRef<number | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [size, setSize] = useState({ columns: 2, stride: 280, height: 600 });
  const rows = Math.ceil(items.length / size.columns);
  const startRow = Math.max(0, Math.min(rows - 1, Math.floor(scrollTop / size.stride) - OVERSCAN_ROWS));
  const endRow = Math.min(rows, Math.ceil((scrollTop + size.height) / size.stride) + OVERSCAN_ROWS);
  const start = startRow * size.columns;
  const end = Math.min(items.length, Math.max(startRow + 1, endRow) * size.columns);

  useLayoutEffect(() => {
    const scroll = scrollRef.current;
    const grid = gridRef.current;
    if (!scroll || !grid) return;
    const measure = () => {
      const template = getComputedStyle(grid).gridTemplateColumns;
      const columns = template && template !== "none" ? template.trim().split(/\s+/).length : 2;
      const first = grid.firstElementChild as HTMLElement | null;
      const height = first?.getBoundingClientRect().height ?? 0;
      setSize((previous) => {
        const next = { columns, stride: height > 0 ? height + GAP : previous.stride, height: scroll.clientHeight || previous.height };
        return next.columns === previous.columns && next.stride === previous.stride && next.height === previous.height ? previous : next;
      });
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(scroll);
    observer.observe(grid);
    return () => observer.disconnect();
  }, [items.length]);

  useLayoutEffect(() => {
    const index = pendingFocus.current;
    if (index === null) return;
    const target = gridRef.current?.querySelector<HTMLButtonElement>(`[data-grid-index="${index}"] button`);
    if (target) {
      pendingFocus.current = null;
      target.focus();
    }
  }, [start, end, size.columns]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    const item = (event.target as HTMLElement).closest<HTMLElement>("[data-grid-index]");
    if (!item) return;
    const index = Number(item.dataset.gridIndex);
    const offsets: Record<string, number> = { ArrowLeft: -1, ArrowRight: 1, ArrowUp: -size.columns, ArrowDown: size.columns };
    let next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1 : index + (offsets[event.key] ?? 0);
    if (next === index) return;
    event.preventDefault();
    next = Math.max(0, Math.min(items.length - 1, next));
    const target = gridRef.current?.querySelector<HTMLButtonElement>(`[data-grid-index="${next}"] button`);
    if (target) { target.focus(); return; }
    pendingFocus.current = next;
    const top = Math.floor(next / size.columns) * size.stride;
    if (scrollRef.current) scrollRef.current.scrollTop = top;
    setScrollTop(top);
  };

  return (
    <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto pr-1"
      onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)} onKeyDown={onKeyDown}>
      <div ref={gridRef} role="list" aria-label={label}
        className="grid grid-cols-2 content-start gap-4 lg:grid-cols-3 xl:grid-cols-4 2xl:grid-cols-5"
        style={{ paddingTop: startRow * size.stride, paddingBottom: Math.max(0, rows - Math.ceil(end / size.columns)) * size.stride }}>
        {items.slice(start, end).map((item, offset) => (
          <div key={itemKey(item)} role="listitem" aria-setsize={items.length} aria-posinset={start + offset + 1}
            data-grid-index={start + offset} className="grid min-w-0">
            {renderItem(item)}
          </div>
        ))}
      </div>
    </div>
  );
}
