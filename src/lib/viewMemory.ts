import { useCallback, useState } from "react";

/**
 * 页面内状态的会话级记忆（搜索词、排序、滚动位置、打开的详情）。
 *
 * `MainPages` 用 `key={activeView}` 在切换视图时整页卸载重建，组件本地 state 随之
 * 丢失：在专辑详情里听歌，去一趟设置再回来就回到了网格顶部。这里按键保存这些
 * 状态，页面重新挂载时取回。
 *
 * 只存在内存里：不持久化、不进配置导出，重启后从头开始。键由调用方按「视图 +
 * 用途」命名（如 `tracks:local`、`artists:selected`）。
 */
const memory = new Map<string, unknown>();

export function recallView<T>(key: string | null | undefined, fallback: T): T {
  if (!key || !memory.has(key)) return fallback;
  return memory.get(key) as T;
}

export function rememberView<T>(key: string | null | undefined, value: T): void {
  if (key) memory.set(key, value);
}

/** 测试用：清空全部记忆。 */
export function forgetAllViews(): void {
  memory.clear();
}

/**
 * 与 `useState` 同形，但初值取自记忆、每次更新写回记忆。`key` 为空时退化为
 * 普通的本地 state。
 */
export function useRememberedState<T>(key: string | null | undefined, initial: T) {
  const [value, setValue] = useState<T>(() => recallView(key, initial));
  const set = useCallback(
    (next: T | ((previous: T) => T)) => {
      setValue((previous) => {
        const resolved =
          typeof next === "function" ? (next as (previous: T) => T)(previous) : next;
        rememberView(key, resolved);
        return resolved;
      });
    },
    [key]
  );
  return [value, set] as const;
}
