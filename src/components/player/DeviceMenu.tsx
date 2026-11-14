import { Check, Monitor } from "lucide-react";
import { useEffect, useRef, type KeyboardEvent } from "react";
import { usePlayerStore } from "@/store/player";

export function DeviceMenu() {
  const deviceMenuOpen = usePlayerStore((s) => s.deviceMenuOpen);
  const toggleDeviceMenu = usePlayerStore((s) => s.toggleDeviceMenu);
  const closeDeviceMenu = usePlayerStore((s) => s.closeDeviceMenu);
  const currentDeviceId = usePlayerStore((s) => s.currentDeviceId);
  const devices = usePlayerStore((s) => s.devices);
  const selectDevice = usePlayerStore((s) => s.selectDevice);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // 发现10：点击菜单外部时关闭设备菜单
  useEffect(() => {
    if (!deviceMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) closeDeviceMenu();
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [deviceMenuOpen, closeDeviceMenu]);

  // 打开后把焦点放到当前设备（没有则第一项），键盘可以直接上下选
  useEffect(() => {
    if (!deviceMenuOpen) return;
    const items = menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]');
    if (!items || items.length === 0) return;
    const current = Array.from(items).find((item) => item.getAttribute("aria-checked") === "true");
    (current ?? items[0]).focus();
  }, [deviceMenuOpen]);

  const closeAndRestoreFocus = () => {
    closeDeviceMenu();
    triggerRef.current?.focus();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      closeAndRestoreFocus();
      return;
    }
    const offsets: Record<string, number> = { ArrowDown: 1, ArrowUp: -1 };
    if (!(event.key in offsets) && event.key !== "Home" && event.key !== "End") return;
    const items = Array.from(
      menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') ?? []
    );
    if (items.length === 0) return;
    event.preventDefault();
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : (index + offsets[event.key] + items.length) % items.length;
    items[next].focus();
  };

  return (
    <div className="relative" ref={rootRef}>
      <button
        ref={triggerRef}
        type="button"
        onClick={toggleDeviceMenu}
        className="text-ink2 hover:text-ink transition-colors"
        title="切换输出设备"
        aria-label="切换输出设备"
        aria-haspopup="menu"
        aria-expanded={deviceMenuOpen}
      >
        <Monitor className="w-3.5 h-3.5" />
      </button>

      {deviceMenuOpen && (
        <div
          ref={menuRef}
          role="menu"
          aria-label="输出设备"
          onKeyDown={onMenuKeyDown}
          className="absolute bottom-8 right-0 z-30 max-h-[min(60vh,320px)] w-52 space-y-1 overflow-y-auto border-2 border-ink bg-card p-1.5 shadow-[4px_4px_0_rgba(43,39,34,0.18)]"
        >
          {devices.length === 0 ? (
            <p className="px-2 py-1.5 font-tw text-[10px] text-ink3">没有可用的输出设备</p>
          ) : null}
          {devices.map((device) => {
            const active = device.id === currentDeviceId;
            return (
              <button
                key={device.id}
                type="button"
                role="menuitemradio"
                aria-checked={active}
                onClick={() => selectDevice(device.id)}
                title={device.name}
                className={`w-full text-left px-2 py-1.5 font-tw text-[10px] flex justify-between items-center gap-2 ${
                  active
                    ? "text-ink bg-paper2 font-bold"
                    : "text-ink2 hover:text-ink hover:bg-paper2"
                }`}
              >
                <span className="min-w-0 truncate">
                  {device.name}
                  {device.isDefault && (
                    <span className="ml-1 text-[9px] text-ink3">默认</span>
                  )}
                </span>
                {active && <Check className="w-2.5 h-2.5 shrink-0 text-stamp" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
