import { useEffect } from "react";
import { listen, FRONTEND_EVENT, isTauriRuntime } from "@/lib/tauri";

interface PlayerEventPayload {
  type: string;
  [key: string]: unknown;
}

/**
 * 监听后端推送的 `PlayerEvent`。
 * Stub 模式（纯浏览器）不会触发任何回调，安全无副作用。
 */
export function usePlayerEvents(
  handler: (event: PlayerEventPayload) => void
) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    (async () => {
      if (!isTauriRuntime()) return;
      unlisten = await listen<PlayerEventPayload>(FRONTEND_EVENT, (payload) => {
        if (!cancelled) handler(payload);
      });
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [handler]);
}
