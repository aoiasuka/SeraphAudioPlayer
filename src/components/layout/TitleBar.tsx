import { Disc3, Minus, Square, X } from "lucide-react";
import { useCallback } from "react";
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

  const onClose = useCallback(async () => {
    if (await isTauri()) {
      const w = await import("@tauri-apps/api/window");
      await w.getCurrentWindow().close();
    }
  }, []);

  return (
    <div
      data-tauri-drag-region
      className="h-10 w-full bg-white/70 backdrop-blur-md flex justify-between items-center px-4 border-b border-black/[0.04] z-20"
    >
      <div data-tauri-drag-region className="flex items-center gap-2 pointer-events-none">
        <Disc3 className="text-cyan-600 w-4 h-4 animate-spin-slow" />
        <span className="text-xs font-semibold text-slate-700 tracking-wide">
          Seraph Audio Player
        </span>
      </div>
      <div className="flex items-center gap-6 text-slate-500">
        <button
          onClick={onMinimize}
          className="hover:text-slate-800 transition-colors"
          aria-label="最小化"
        >
          <Minus className="w-3 h-3" />
        </button>
        <button
          onClick={onToggleMaximize}
          className="hover:text-slate-800 transition-colors"
          aria-label="最大化"
        >
          <Square className="w-3 h-3" />
        </button>
        <button
          onClick={onClose}
          className="hover:text-red-500 transition-colors"
          aria-label="关闭"
        >
          <X className="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  );
}
