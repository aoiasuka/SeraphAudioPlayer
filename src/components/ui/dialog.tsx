import * as React from "react";
import { cn } from "@/lib/utils";

interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: React.ReactNode;
  className?: string;
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** 同开多弹窗时的层级栈：Esc / Tab 循环只作用于最顶层弹窗 */
const dialogStack: symbol[] = [];

export function Dialog({ open, onClose, children, className }: DialogProps) {
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const pointerDownOnOverlay = React.useRef(false);

  // 发现13：打开时把焦点移入弹窗，避免焦点留在背景触发按钮上
  React.useEffect(() => {
    if (open) containerRef.current?.focus();
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const stackId = Symbol("dialog");
    dialogStack.push(stackId);
    const onKey = (e: KeyboardEvent) => {
      // L-19 附带：多弹窗同开时非顶层不响应（此前一次 Esc 全关）
      if (dialogStack[dialogStack.length - 1] !== stackId) return;
      if (e.key === "Escape") {
        onClose();
        return;
      }
      // 发现13：简单 Tab 循环，把键盘焦点困在弹窗内
      if (e.key === "Tab") {
        const container = containerRef.current;
        if (!container) return;
        const focusables = Array.from(
          container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR)
        );
        if (focusables.length === 0) {
          e.preventDefault();
          return;
        }
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        const active = document.activeElement;
        const inside = active instanceof Node && container.contains(active);
        if (e.shiftKey) {
          if (!inside || active === first) {
            e.preventDefault();
            last.focus();
          }
        } else if (!inside || active === last) {
          e.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      const index = dialogStack.indexOf(stackId);
      if (index >= 0) dialogStack.splice(index, 1);
      window.removeEventListener("keydown", onKey);
    };
  }, [open, onClose]);

  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-sm p-4"
      onPointerDown={(e) => {
        pointerDownOnOverlay.current = e.target === e.currentTarget;
      }}
      onClick={(e) => {
        // L-19：按下与松开都发生在遮罩上才关闭——弹窗内选文本拖到遮罩
        // 松手时 click 落在遮罩，旧的纯 click 判定会误关弹窗丢用户输入。
        if (pointerDownOnOverlay.current && e.target === e.currentTarget) {
          onClose();
        }
        pointerDownOnOverlay.current = false;
      }}
    >
      <div
        ref={containerRef}
        role="dialog"
        aria-modal="true"
        tabIndex={-1}
        className={cn(
          "relative w-full max-w-md border-2 border-ink bg-card p-6 shadow-[6px_6px_0_rgba(43,39,34,0.25)] outline-none",
          className
        )}
        onClick={(e) => e.stopPropagation()}
      >
        {children}
      </div>
    </div>
  );
}
