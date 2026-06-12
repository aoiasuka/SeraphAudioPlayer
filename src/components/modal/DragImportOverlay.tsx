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
        "pointer-events-none fixed inset-0 z-40 flex items-center justify-center bg-ink/15 backdrop-blur-sm transition-all duration-200",
        visible ? "opacity-100" : "opacity-0"
      )}
    >
      <div
        className={cn(
          "flex h-[220px] w-[min(520px,calc(100vw-56px))] flex-col items-center justify-center border-2 border-dashed border-ink bg-card text-center shadow-[6px_6px_0_rgba(43,39,34,0.18)] transition-all duration-200",
          visible ? "scale-100" : "scale-95"
        )}
      >
        <div className="mb-4 flex h-12 w-12 items-center justify-center border-2 border-ink bg-paper2 text-brown">
          <UploadCloud className="h-6 w-6" />
        </div>
        <p className="font-serif text-base font-bold text-ink">释放以归档本地音乐</p>
        <p className="mt-1 font-tw text-xs font-medium text-ink2">
          支持音频文件和文件夹
        </p>
      </div>
    </div>
  );
}
