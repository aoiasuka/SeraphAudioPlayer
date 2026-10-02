import { useEffect } from "react";
import { isTauriRuntime } from "@/lib/tauri";
import { runAfterFirstPaint } from "@/lib/startup";

/**
 * 显示主窗口（tauri.conf.json 里 `visible:false`，首帧绘制后才显示以免白闪）。
 * 除正常启动外，崩溃兜底页也要调用：App 首帧就抛错时它的 effect 永远不会执行，
 * 兜底页会渲染在一个从未显示的窗口里（BUG-02）。后端另有超时兜底（lib.rs）。
 */
export async function revealMainWindow(isCancelled: () => boolean = () => false) {
  if (!isTauriRuntime()) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    if (isCancelled()) return;
    await getCurrentWindow().show();
  } catch (err) {

    console.warn("Failed to reveal window", err);
  }
}

export function useRevealWindow() {
  useEffect(() => {
    let cancelled = false;

    const cancelReveal = runAfterFirstPaint(() => {
      void revealMainWindow(() => cancelled);
    });

    return () => {
      cancelled = true;
      cancelReveal();
    };
  }, []);
}
