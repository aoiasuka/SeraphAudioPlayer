import { UploadCloud } from "lucide-react";
import { cn } from "@/lib/utils";

interface DragImportOverlayProps {
  visible: boolean;
}

export function DragImportOverlay({ visible }: DragImportOverlayProps) {
  return (
    <div
      aria-hidden={!visible}
      className={cn(
        "pointer-events-none fixed inset-0 z-40 flex items-center justify-center bg-cyan-950/10 backdrop-blur-sm transition-all duration-200",
        visible ? "opacity-100" : "opacity-0"
      )}
    >
      <div
        className={cn(
          "flex h-[220px] w-[min(520px,calc(100vw-56px))] flex-col items-center justify-center rounded-lg border border-dashed border-cyan-500/45 bg-white/80 text-center shadow-[0_20px_60px_rgba(15,23,42,0.12)] transition-all duration-200",
          visible ? "scale-100" : "scale-95"
        )}
      >
        <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-lg bg-cyan-600/10 text-cyan-700">
          <UploadCloud className="h-6 w-6" />
        </div>
        <p className="text-base font-bold text-slate-800">释放以添加本地音乐</p>
        <p className="mt-1 text-xs font-medium text-slate-500">
          支持音频文件和文件夹
        </p>
      </div>
    </div>
  );
}
