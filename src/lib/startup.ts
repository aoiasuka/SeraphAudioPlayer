export function runAfterFirstPaint(callback: () => void) {
  let cancelled = false;
  const rafId = window.requestAnimationFrame(() => {
    if (!cancelled) callback();
  });

  return () => {
    cancelled = true;
    window.cancelAnimationFrame(rafId);
  };
}

export function runWhenIdle(callback: () => void, timeout = 1200) {
  let cancelled = false;

  if (typeof window.requestIdleCallback === "function") {
    const idleId = window.requestIdleCallback(
      () => {
        if (!cancelled) callback();
      },
      { timeout }
    );

    return () => {
      cancelled = true;
      window.cancelIdleCallback?.(idleId);
    };
  }

  const timeoutId = window.setTimeout(() => {
    if (!cancelled) callback();
  }, Math.min(timeout, 250));

  return () => {
    cancelled = true;
    window.clearTimeout(timeoutId);
  };
}
