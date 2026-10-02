import { useEffect, useState } from "react";
import { AlertTriangle, Stamp } from "lucide-react";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";
import type { NotificationLevel } from "@/store/player/types";

/** 可见时长（毫秒）：错误提示需要更多阅读时间（REL-08）。 */
const VISIBLE_MS: Record<NotificationLevel, number> = { info: 2700, error: 6500 };
const EXIT_ANIMATION_MS = 500;

export function Notification() {
  const notification = usePlayerStore((s) => s.notification);
  const dismiss = usePlayerStore((s) => s.dismissNotification);
  const [visible, setVisible] = useState(false);
  const [content, setContent] = useState<{ text: string; level: NotificationLevel }>({
    text: "",
    level: "info",
  });

  useEffect(() => {
    if (!notification) {
      // store 已清空：仅触发滑出（保留 content 供退场动画显示），不立即清空文字
      setVisible(false);
      return;
    }
    // M-16：快照文字，使 dismiss 把 store 置 null 后，退场动画期间仍有内容可显示
    const level = notification.level ?? "info";
    setContent({ text: notification.text, level });
    setVisible(true);
    const hideTimer = window.setTimeout(() => setVisible(false), VISIBLE_MS[level]);
    const clearTimer = window.setTimeout(() => dismiss(), VISIBLE_MS[level] + EXIT_ANIMATION_MS);
    return () => {
      window.clearTimeout(hideTimer);
      window.clearTimeout(clearTimer);
    };
  }, [notification, dismiss]);

  const level = notification?.level ?? content.level;
  const isError = level === "error";
  return (
    <div
      role={isError ? "alert" : "status"}
      aria-live={isError ? "assertive" : "polite"}
      className={cn(
        "fixed top-14 right-10 bg-card border-2 text-ink px-4 py-3 shadow-[5px_5px_0_rgba(43,39,34,0.2)] flex items-center gap-3 z-50 transition-all duration-500 ease-out",
        isError ? "border-stamp" : "border-ink",
        visible
          ? "translate-x-0 opacity-100 pointer-events-auto"
          : "translate-x-[120%] opacity-0 pointer-events-none"
      )}
    >
      {isError ? (
        <AlertTriangle className="w-4 h-4 text-stamp" aria-hidden="true" />
      ) : (
        <Stamp className="w-4 h-4 text-stamp" aria-hidden="true" />
      )}
      <span className="font-tw text-xs font-bold">
        {notification?.text ?? content.text}
      </span>
    </div>
  );
}
