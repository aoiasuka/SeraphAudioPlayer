import { useLayoutEffect, useMemo, useRef, useState, type UIEvent } from "react";
import { recallView, rememberView } from "@/lib/viewMemory";

/**
 * 定高行的虚拟滚动：只渲染可见区 ± overscan 行，上下用 padding 撑出总高度。
 * 容器高度用 ResizeObserver 测量（M-17：`mountKey` 变化 = 滚动容器重挂，须重新观察）。
 *
 * `stateKey` 给出后记住滚动位置：页面被卸载重建（切换视图再回来）时恢复到离开时的
 * 位置，见 `lib/viewMemory.ts`。只在容器首次挂载时恢复一次；之后的重挂（例如列表
 * 清空又出现）仍按真实 DOM 位置走。
 */
export function useVirtualRows(
  count: number,
  rowHeight: number,
  { overscan = 6, initialHeight = 420, mountKey = true, stateKey }: {
    overscan?: number;
    initialHeight?: number;
    mountKey?: unknown;
    stateKey?: string;
  } = {}
) {
  const scrollKey = stateKey ? `${stateKey}:scroll` : null;
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const pendingRestore = useRef<number | null>(recallView<number | null>(scrollKey, null));
  const [scrollTop, setScrollTop] = useState(() => recallView(scrollKey, 0));
  const [viewportHeight, setViewportHeight] = useState(initialHeight);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const restore = pendingRestore.current;
    pendingRestore.current = null;
    if (restore !== null && restore > 0) {
      element.scrollTop = restore;
      setScrollTop(restore);
    } else {
      setScrollTop(element.scrollTop);
    }
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

  const onScroll = (event: UIEvent<HTMLDivElement>) => {
    const top = event.currentTarget.scrollTop;
    setScrollTop(top);
    rememberView(scrollKey, top);
  };

  /** 回到顶部：列表内容整体换了（改了搜索词、排序）时用，同时清掉记住的滚动位置。 */
  const scrollToTop = () => {
    setScrollTop(0);
    rememberView(scrollKey, 0);
    pendingRestore.current = null;
    scrollRef.current?.scrollTo({ top: 0 });
  };

  return { scrollRef, onScroll, scrollToTop, ...range };
}
