import { useEffect, useRef, useState } from "react";
import { AlertTriangle, History } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";

function formatClock(at: number) {
  const date = new Date(at);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/**
 * 标题栏的「最近通知」（UI4-03）。通知只有一个槽位、几秒后自动消失：连续两个错误时
 * 第一个看不到，批量删除的失败原因、导入时跳过的文件数错过了也没处回看。
 * 这里保留本次运行期间最近的若干条（不持久化），点开即可回看。
 */
export function NotificationHistory() {
  const history = usePlayerStore((s) => s.notificationHistory);
  const clear = usePlayerStore((s) => s.clearNotificationHistory);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointerDown = (event: PointerEvent) => {
      if (rootRef.current && !rootRef.current.contains(event.target as Node)) setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
      buttonRef.current?.focus();
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown, true);
    };
  }, [open]);

  const errorCount = history.reduce((count, item) => count + (item.level === "error" ? 1 : 0), 0);
  return (
    <div ref={rootRef} className="relative flex items-center">
      <button
        ref={buttonRef}
        type="button"
        onClick={() => setOpen((value) => !value)}
        className="hover:text-ink transition-colors"
        aria-label={`最近通知（${history.length} 条）`}
        title="最近通知"
        aria-haspopup="dialog"
        aria-expanded={open}
      >
        <History className="w-3.5 h-3.5" aria-hidden="true" />
      </button>
      {open ? (
        <div
          role="dialog"
          aria-label="最近通知"
          className="absolute right-0 top-6 z-[60] w-80 max-h-[60vh] overflow-y-auto bg-card border-2 border-ink shadow-[5px_5px_0_rgba(43,39,34,0.2)] text-ink"
        >
          <div className="flex items-center justify-between px-3 py-2 border-b border-ink/20">
            <span className="font-tw text-[11px] font-bold tracking-wide">
              最近通知{errorCount > 0 ? ` · ${errorCount} 条错误` : ""}
            </span>
            <button
              type="button"
              className="font-tw text-[11px] text-ink2 hover:text-stamp disabled:opacity-40"
              disabled={history.length === 0}
              onClick={clear}
            >
              清空
            </button>
          </div>
          {history.length === 0 ? (
            <p className="px-3 py-4 font-tw text-xs text-ink2">本次运行还没有通知。</p>
          ) : (
            <ul>
              {history.map((item) => (
                <li
                  key={item.id}
                  className={cn(
                    "flex gap-2 px-3 py-2 border-b border-ink/10 last:border-b-0",
                    item.level === "error" && "bg-stamp/5"
                  )}
                >
                  <span className="font-tw text-[10px] text-ink2 tabular-nums pt-0.5 shrink-0">{formatClock(item.at)}</span>
                  {item.level === "error" ? (
                    <AlertTriangle className="w-3.5 h-3.5 text-stamp shrink-0 mt-0.5" aria-label="错误" />
                  ) : null}
                  <span className="font-tw text-xs break-words min-w-0 select-text">{item.text}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : null}
    </div>
  );
}
