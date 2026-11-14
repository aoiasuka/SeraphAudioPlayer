import { Disc3, Minus, Square, X } from "lucide-react";
import { NotificationHistory } from "@/components/layout/NotificationHistory";
import { useCallback, useEffect, useState } from "react";
import { isTauri } from "@/lib/tauri";

export function TitleBar() {
  const onMinimize = useCallback(async () => {
    if (await isTauri()) {
      const w = await import("@tauri-apps/api/window");
      await w.getCurrentWindow().minimize();
    }
  }, []);

  const onToggleMaximize = useCallback(async () => {
    if (await isTauri()) {
      const w = await import("@tauri-apps/api/window");
      await w.getCurrentWindow().toggleMaximize();
    }
  }, []);

  // UI5-06：按钮名称跟随窗口状态（最大化后读作「还原」）
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void (async () => {
      if (!(await isTauri())) return;
      const current = (await import("@tauri-apps/api/window")).getCurrentWindow();
      const refresh = () => {
        void current.isMaximized().then((value) => { if (!disposed) setMaximized(value); }).catch(() => undefined);
      };
      refresh();
      const stop = await current.onResized(refresh);
      if (disposed) stop();
      else unlisten = stop;
    })().catch(() => undefined);
    return () => { disposed = true; unlisten?.(); };
  }, []);

  const onClose = useCallback(async () => {
    if (await isTauri()) {
      const w = await import("@tauri-apps/api/window");
      await w.getCurrentWindow().close();
    }
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="h-10 w-full bg-paper flex justify-between items-center px-4 border-b-2 border-ink z-20"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pointer-events-none">
        <Disc3 className="text-stamp w-4 h-4 animate-spin-slow" />
        <span className="font-tw text-xs font-bold text-ink tracking-wide">
          SERAPH<span className="text-stamp">_</span> AUDIO ARCHIVE
        </span>
      </div>
      <div className="flex items-center gap-5 text-ink2">
        <NotificationHistory />
        <button
          onClick={onMinimize}
          className="hover:text-ink transition-colors"
          aria-label="最小化"
        >
          <Minus className="w-3.5 h-3.5" />
        </button>
        <button
          onClick={onToggleMaximize}
          className="hover:text-ink transition-colors"
          aria-label={maximized ? "还原" : "最大化"}
          title={maximized ? "还原" : "最大化"}
        >
          <Square className="w-3 h-3" />
        </button>
        <button
          onClick={onClose}
          className="hover:text-stamp transition-colors"
          aria-label="关闭"
        >
          <X className="w-4 h-4" />
        </button>
      </div>
    </div>
  );
}
