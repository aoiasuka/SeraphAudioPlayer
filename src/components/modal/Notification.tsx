import { useEffect, useState } from "react";
import { CheckCircle2 } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";

export function Notification() {
  const notification = usePlayerStore((s) => s.notification);
  const dismiss = usePlayerStore((s) => s.dismissNotification);
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!notification) {
      setVisible(false);
      return;
    }
    setVisible(true);
    const hideTimer = window.setTimeout(() => setVisible(false), 2700);
    const clearTimer = window.setTimeout(() => dismiss(), 3200);
    return () => {
      window.clearTimeout(hideTimer);
      window.clearTimeout(clearTimer);
    };
  }, [notification, dismiss]);

  return (
    <div
      className={cn(
        "fixed top-10 right-10 bg-white border border-cyan-500/20 text-slate-800 px-4 py-3 rounded-lg shadow-[0_8px_32px_rgba(15,23,42,0.08)] flex items-center gap-3 z-50 transition-all duration-500 ease-out",
        visible
          ? "translate-x-0 opacity-100 pointer-events-auto"
          : "translate-x-[120%] opacity-0 pointer-events-none"
      )}
    >
      <CheckCircle2 className="w-4 h-4 text-cyan-600" />
      <span className="text-xs font-semibold">
        {notification?.text ?? ""}
      </span>
    </div>
  );
}
