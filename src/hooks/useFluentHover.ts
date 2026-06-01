import { useCallback } from "react";

/**
 * Fluent Design hover：在元素 CSS 自定义属性上记录鼠标位置，
 * 配合 `.fluent-item::before` 的 radial-gradient 营造跟随光斑。
 */
export function useFluentHover() {
  return useCallback((e: React.MouseEvent<HTMLElement>) => {
    const target = e.currentTarget;
    const rect = target.getBoundingClientRect();
    target.style.setProperty("--fluent-x", `${e.clientX - rect.left}px`);
    target.style.setProperty("--fluent-y", `${e.clientY - rect.top}px`);
  }, []);
}
