import { useLayoutEffect, useMemo, useRef, useState, type UIEvent } from "react";

/**
 * 定高行的虚拟滚动：只渲染可见区 ± overscan 行，上下用 padding 撑出总高度。
 * 容器高度用 ResizeObserver 测量（M-17：`mountKey` 变化 = 滚动容器重挂，须重新观察）。
 */
export function useVirtualRows(
  count: number,
  rowHeight: number,
  { overscan = 6, initialHeight = 420, mountKey = true }: {
    overscan?: number;
    initialHeight?: number;
    mountKey?: unknown;
  } = {}
) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(initialHeight);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    setScrollTop(element.scrollTop);
    const update = () => setViewportHeight(element.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, [mountKey]);

  const range = useMemo(() => {
    const effectiveScrollTop = Math.min(scrollTop, Math.max(0, count * rowHeight - viewportHeight));
    const start = Math.max(0, Math.floor(effectiveScrollTop / rowHeight) - overscan);
    const end = Math.min(count, Math.ceil((effectiveScrollTop + viewportHeight) / rowHeight) + overscan);
    return {
      start,
      end,
      paddingTop: start * rowHeight,
      paddingBottom: (count - end) * rowHeight,
    };
  }, [scrollTop, count, rowHeight, viewportHeight, overscan]);

  const onScroll = (event: UIEvent<HTMLDivElement>) => setScrollTop(event.currentTarget.scrollTop);

  return { scrollRef, onScroll, ...range };
}
