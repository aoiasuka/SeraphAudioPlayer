import { useEffect } from "react";
import { isTauriRuntime } from "@/lib/tauri";
import { runAfterFirstPaint } from "@/lib/startup";

export function useRevealWindow() {
  useEffect(() => {
    let cancelled = false;

    async function reveal() {
      if (!isTauriRuntime()) return;

      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        if (cancelled) return;
        await getCurrentWindow().show();
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Failed to reveal window", err);
      }
    }

    const cancelReveal = runAfterFirstPaint(() => {
      void reveal();
    });

    return () => {
      cancelled = true;
      cancelReveal();
    };
  }, []);
}
