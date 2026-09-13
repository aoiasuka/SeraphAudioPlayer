/** 共享单请求槽位：切页、隐藏与暂停只替换下一次任务，迟到结果不会回填。 */
export function createSerialPoller() {
  let generation = 0;
  let busy = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let active: { intervalMs: number; run: (generation: number, current: () => boolean) => Promise<void> } | null = null;
  const visible = () => document.visibilityState !== "hidden";
  const clearTimer = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  const schedule = (delay: number) => {
    clearTimer();
    if (active && !busy && visible()) timer = setTimeout(() => { void tick(); }, delay);
  };
  const tick = async () => {
    timer = undefined;
    if (!active || busy || !visible()) return;
    const task = active;
    const version = generation;
    const started = performance.now();
    busy = true;
    try {
      await task.run(version, () => active === task && generation === version && visible());
    } finally {
      busy = false;
      schedule(active === task && generation === version
        ? Math.max(0, task.intervalMs - (performance.now() - started)) : 0);
    }
  };
  const onVisibilityChange = () => {
    generation += 1;
    schedule(0);
  };

  return {
    start<T>(options: {
      intervalMs: number;
      read: (generation: number) => Promise<T>;
      onData: (value: T) => void;
      onError?: (error: unknown) => void;
    }) {
      generation += 1;
      const task = {
        intervalMs: options.intervalMs,
        run: async (version: number, current: () => boolean) => {
          try {
            const value = await options.read(version);
            if (current()) options.onData(value);
          } catch (error) {
            if (current()) options.onError?.(error);
          }
        },
      };
      active = task;
      document.removeEventListener("visibilitychange", onVisibilityChange);
      document.addEventListener("visibilitychange", onVisibilityChange);
      schedule(0);
      return () => {
        if (active !== task) return;
        active = null;
        generation += 1;
        clearTimer();
        document.removeEventListener("visibilitychange", onVisibilityChange);
      };
    },
  };
}
