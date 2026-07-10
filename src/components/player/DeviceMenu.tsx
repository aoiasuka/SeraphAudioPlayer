import { Check, Monitor } from "lucide-react";
import { useEffect, useRef } from "react";
import { usePlayerStore } from "@/store/player";

export function DeviceMenu() {
  const deviceMenuOpen = usePlayerStore((s) => s.deviceMenuOpen);
  const toggleDeviceMenu = usePlayerStore((s) => s.toggleDeviceMenu);
  const closeDeviceMenu = usePlayerStore((s) => s.closeDeviceMenu);
  const currentDeviceId = usePlayerStore((s) => s.currentDeviceId);
  const devices = usePlayerStore((s) => s.devices);
  const selectDevice = usePlayerStore((s) => s.selectDevice);
  const rootRef = useRef<HTMLDivElement | null>(null);

  // 发现10：点击菜单外部时关闭设备菜单
  useEffect(() => {
    if (!deviceMenuOpen) return;
    const onPointerDown = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) closeDeviceMenu();
    };
    window.addEventListener("pointerdown", onPointerDown);
    return () => window.removeEventListener("pointerdown", onPointerDown);
  }, [deviceMenuOpen, closeDeviceMenu]);

  return (
    <div className="relative" ref={rootRef}>
      <button
        onClick={toggleDeviceMenu}
        className="text-ink2 hover:text-ink transition-colors"
        title="切换输出设备"
        aria-label="切换输出设备"
      >
        <Monitor className="w-3.5 h-3.5" />
      </button>

      {deviceMenuOpen && (
        <div className="absolute bottom-8 right-0 w-52 bg-card border-2 border-ink shadow-[4px_4px_0_rgba(43,39,34,0.18)] p-1.5 space-y-1 z-30">
          {devices.map((device) => {
            const active = device.id === currentDeviceId;
            return (
              <button
                key={device.id}
                onClick={() => selectDevice(device.id)}
                className={`w-full text-left px-2 py-1.5 font-tw text-[10px] flex justify-between items-center gap-2 ${
                  active
                    ? "text-ink bg-paper2 font-bold"
                    : "text-ink2 hover:text-ink hover:bg-paper2"
                }`}
              >
                <span className="min-w-0 truncate">
                  {device.name}
                  {device.isDefault && (
                    <span className="ml-1 text-[9px] text-ink3">Default</span>
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
