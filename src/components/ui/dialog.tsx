import * as React from "react";
import { cn } from "@/lib/utils";

interface DialogProps {
  open: boolean;
  onClose: () => void;
  children: React.ReactNode;
  className?: string;
  "aria-label"?: string;
  "aria-labelledby"?: string;
  "aria-describedby"?: string;
}

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

/** 上下文跨 portal 保留祖先关系，避免子组件先执行 effect 时父层抢到栈顶。 */
const DialogAncestors = React.createContext<symbol[]>([]);
interface DialogStackEntry {
  id: symbol;
  ancestors: symbol[];
  container: HTMLDivElement;
  returnFocus: HTMLElement | null;
}
const dialogStack: DialogStackEntry[] = [];

export function Dialog({ open, onClose, children, className, "aria-label": label, "aria-labelledby": labelledBy, "aria-describedby": describedBy }: DialogProps) {
  const containerRef = React.useRef<HTMLDivElement | null>(null);
  const pointerDownOnOverlay = React.useRef(false);
  const closeRef = React.useRef(onClose);
  const stackId = React.useRef(Symbol("dialog")).current;
  const ancestors = React.useContext(DialogAncestors);
  const childAncestors = React.useMemo(() => [...ancestors, stackId], [ancestors, stackId]);
  const titleId = React.useId();
  const [automaticTitle, setAutomaticTitle] = React.useState<string>();

  React.useLayoutEffect(() => { closeRef.current = onClose; }, [onClose]);

  // 从实际 DOM 关联标题，兼容既有调用方把标题包在任意子组件中的写法。
  React.useLayoutEffect(() => {
    const container = containerRef.current;
    if (!open || !container || label || labelledBy) {
      setAutomaticTitle(undefined);
      return;
    }
    const associateTitle = () => {
      const heading = Array.from(container.querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6, [role='heading']"))
        .find((element) => element.closest('[role="dialog"]') === container);
      if (heading && !heading.id) heading.id = `${titleId}-title`;
      setAutomaticTitle(heading?.id);
    };
    associateTitle();
    const observer = new MutationObserver(associateTitle);
    observer.observe(container, { childList: true, subtree: true, attributes: true, attributeFilter: ["id"] });
    return () => observer.disconnect();
  }, [open, label, labelledBy, titleId]);

  React.useEffect(() => {
    const container = containerRef.current;
    if (!open || !container) return;
    const entry: DialogStackEntry = {
      id: stackId,
      ancestors,
      container,
      returnFocus: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    };
    const descendantIndex = dialogStack.findIndex((item) => item.ancestors.includes(stackId));
    if (descendantIndex < 0) dialogStack.push(entry);
    else dialogStack.splice(descendantIndex, 0, entry);
    if (dialogStack.at(-1) === entry) container.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || dialogStack.at(-1) !== entry) return;
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopImmediatePropagation();
        closeRef.current();
        return;
      }
      if (e.key === "Tab") {
        const focusables = Array.from(container.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR))
          .filter((element) => element.closest('[role="dialog"]') === container && !element.closest('[hidden], [inert]'));
        if (focusables.length === 0) {
          e.preventDefault();
          container.focus();
          return;
        }
        const first = focusables[0];
        const last = focusables[focusables.length - 1];
        const active = document.activeElement;
        const inside = active instanceof Node && container.contains(active) && active !== container;
        if (e.shiftKey ? (!inside || active === first) : (!inside || active === last)) {
          e.preventDefault();
          (e.shiftKey ? last : first).focus();
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      const wasTop = dialogStack.at(-1) === entry;
      const index = dialogStack.indexOf(entry);
      if (index >= 0) dialogStack.splice(index, 1);
      window.removeEventListener("keydown", onKey);
      pointerDownOnOverlay.current = false;
      // 下层先卸载时，存活的上层不能把焦点归还到已移除的下层里。
      for (const item of dialogStack) {
        if (item.returnFocus && container.contains(item.returnFocus)) item.returnFocus = entry.returnFocus;
      }
      if (wasTop) {
        const top = dialogStack.at(-1);
        if (entry.returnFocus?.isConnected && (!top || top.container.contains(entry.returnFocus))) {
          entry.returnFocus.focus();
        } else top?.container.focus();
      }
    };
  }, [open, stackId, ancestors]);

  if (!open) return null;
  return (
    <DialogAncestors.Provider value={childAncestors}>
      <div
        className="fixed inset-0 z-50 flex items-center justify-center bg-ink/40 backdrop-blur-sm p-4"
        onPointerDown={(e) => {
          pointerDownOnOverlay.current = e.target === e.currentTarget;
        }}
        onClick={(e) => {
          // 按下与松开均在最顶层遮罩才关闭，保留弹窗内拖选文本的行为。
          if (pointerDownOnOverlay.current && e.target === e.currentTarget && dialogStack.at(-1)?.id === stackId) {
            closeRef.current();
          }
          pointerDownOnOverlay.current = false;
        }}
      >
        <div
          ref={containerRef}
          role="dialog"
          aria-modal="true"
          aria-label={label}
          aria-labelledby={labelledBy || (!label ? automaticTitle : undefined)}
          aria-describedby={describedBy}
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
    </DialogAncestors.Provider>
  );
}
