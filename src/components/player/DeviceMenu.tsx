import { Check, Monitor } from "lucide-react";
import { usePlayerStore } from "@/store/player";

export function DeviceMenu() {
  const deviceMenuOpen = usePlayerStore((s) => s.deviceMenuOpen);
  const toggleDeviceMenu = usePlayerStore((s) => s.toggleDeviceMenu);
  const currentDeviceId = usePlayerStore((s) => s.currentDeviceId);
  const devices = usePlayerStore((s) => s.devices);
  const selectDevice = usePlayerStore((s) => s.selectDevice);

  return (
    <div className="relative">
      <button
        onClick={toggleDeviceMenu}
        className="text-slate-500 hover:text-slate-800 transition-colors"
        title="切换输出设备"
        aria-label="切换输出设备"
      >
        <Monitor className="w-3 h-3" />
      </button>

      {deviceMenuOpen && (
        <div className="absolute bottom-7 right-0 w-52 bg-white border border-black/10 rounded-lg shadow-2xl p-1.5 space-y-1 z-30">
          {devices.map((device) => {
            const active = device.id === currentDeviceId;
            return (
              <button
                key={device.id}
                onClick={() => selectDevice(device.id)}
                className={`w-full text-left px-2 py-1.5 text-[10px] rounded flex justify-between items-center gap-2 ${
                  active
                    ? "text-cyan-700 bg-cyan-50/80 font-semibold"
                    : "text-slate-600 hover:text-slate-800 hover:bg-slate-50"
                }`}
              >
                <span className="min-w-0 truncate">
                  {device.name}
                  {device.isDefault && (
                    <span className="ml-1 text-[9px] text-slate-400">Default</span>
                  )}
                </span>
                {active && <Check className="w-2.5 h-2.5 shrink-0" />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
