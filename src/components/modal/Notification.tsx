import { useEffect, useState } from "react";
import { Stamp } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";

export function Notification() {
  const notification = usePlayerStore((s) => s.notification);
  const dismiss = usePlayerStore((s) => s.dismissNotification);
  const [visible, setVisible] = useState(false);
  const [content, setContent] = useState("");

  useEffect(() => {
    if (!notification) {
      // store 已清空：仅触发滑出（保留 content 供退场动画显示），不立即清空文字
      setVisible(false);
      return;
    }
    // M-16：快照文字，使 dismiss 把 store 置 null 后，退场动画期间仍有内容可显示
    setContent(notification.text);
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
        "fixed top-14 right-10 bg-card border-2 border-ink text-ink px-4 py-3 shadow-[5px_5px_0_rgba(43,39,34,0.2)] flex items-center gap-3 z-50 transition-all duration-500 ease-out",
        visible
          ? "translate-x-0 opacity-100 pointer-events-auto"
          : "translate-x-[120%] opacity-0 pointer-events-none"
      )}
    >
      <Stamp className="w-4 h-4 text-stamp" />
      <span className="font-tw text-xs font-bold">
        {notification?.text ?? content}
      </span>
    </div>
  );
}
