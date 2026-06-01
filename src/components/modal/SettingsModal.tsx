import { CheckCircle2, Sliders, X } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { usePlayerStore } from "@/store/player";
import type { DriverKind } from "@/types/track";

const drivers: { value: DriverKind; label: string; hint: string }[] = [
  {
    value: "wasapi",
    label: "系统共享输出",
    hint: "当前可用的稳定输出路径，使用系统默认音频栈和已选择设备。",
  },
  {
    value: "asio",
    label: "ASIO 专业驱动",
    hint: "预留给专业声卡和低延迟播放链路，当前版本会暂时使用共享输出。",
  },
  {
    value: "direct",
    label: "兼容输出",
    hint: "用于保守兼容设置，适合普通扬声器和蓝牙设备。",
  },
];

export function SettingsModal() {
  const open = usePlayerStore((s) => s.settingsOpen);
  const toggleSettings = usePlayerStore((s) => s.toggleSettings);
  const driverKind = usePlayerStore((s) => s.driverKind);
  const setDriver = usePlayerStore((s) => s.setDriver);
  const currentDeviceId = usePlayerStore((s) => s.currentDeviceId);
  const devices = usePlayerStore((s) => s.devices);
  const loadDevices = usePlayerStore((s) => s.loadDevices);
  const selectDevice = usePlayerStore((s) => s.selectDevice);
  const showNotification = usePlayerStore((s) => s.showNotification);

  const currentDevice = devices.find((device) => device.id === currentDeviceId);
  const currentDriver = drivers.find((driver) => driver.value === driverKind);

  const apply = () => {
    toggleSettings();
    showNotification(`已保存输出配置: ${currentDriver?.label ?? driverKind}`);
  };

  return (
    <Dialog
      open={open}
      onClose={toggleSettings}
      className="space-y-6"
    >
      <button
        onClick={toggleSettings}
        className="absolute top-4 right-4 text-slate-400 hover:text-slate-700"
        aria-label="关闭"
      >
        <X className="w-4 h-4" />
      </button>

      <div className="space-y-2">
        <h3 className="text-base font-bold text-slate-800 flex items-center gap-2">
          <Sliders className="w-4 h-4 text-cyan-600" />
          音频输出设置
        </h3>
        <p className="text-[11px] text-slate-500 leading-relaxed">
          管理当前播放设备和输出偏好。
        </p>
      </div>

      <div className="space-y-4">
        <div className="space-y-1.5">
          <label className="text-[9px] font-bold text-slate-400 uppercase">
            Driver Interface
          </label>
          <select
            value={driverKind}
            onChange={(e) => setDriver(e.target.value as DriverKind)}
            className="w-full bg-[#f1f5f9] border border-black/5 rounded-lg p-2 text-xs text-slate-700 focus:outline-none focus:border-cyan-600"
          >
            {drivers.map((driver) => (
              <option key={driver.value} value={driver.value}>
                {driver.label}
              </option>
            ))}
          </select>
          <p className="text-[10px] text-slate-400">
            {currentDriver?.hint}
          </p>
        </div>

        <div className="p-3 bg-white/60 rounded-lg border border-black/[0.04] space-y-2">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h4 className="text-xs font-semibold text-slate-800">
                当前输出设备
              </h4>
              <p className="text-[10px] text-slate-500">
                {currentDevice?.name ?? currentDeviceId}
              </p>
            </div>
            <button
              onClick={loadDevices}
              className="px-2 py-1 text-[10px] font-semibold rounded bg-cyan-600/10 text-cyan-700 hover:bg-cyan-600/15 transition-colors"
            >
              刷新
            </button>
          </div>
          <select
            value={currentDeviceId}
            onChange={(event) => selectDevice(event.target.value)}
            className="w-full bg-[#f1f5f9] border border-black/5 rounded-lg p-2 text-xs text-slate-700 focus:outline-none focus:border-cyan-600"
          >
            {devices.map((device) => (
              <option key={device.id} value={device.id}>
                {device.name}
                {device.isDefault ? " · Default" : ""}
              </option>
            ))}
          </select>
        </div>

        <div className="p-3 bg-cyan-50 rounded-lg border border-cyan-500/10 space-y-1.5">
          <h4 className="text-xs font-semibold text-cyan-800 flex items-center gap-1.5">
            <CheckCircle2 className="w-3.5 h-3.5" />
            输出链路已接入
          </h4>
          <p className="text-[10px] text-slate-500 leading-relaxed">
            本地解码、播放进度事件和系统输出设备已经由 Rust 音频线程驱动。
          </p>
        </div>
      </div>

      <div className="flex justify-end gap-3 pt-3 border-t border-black/5">
        <button
          onClick={toggleSettings}
          className="px-4 py-1.5 text-xs font-semibold text-slate-500 hover:text-slate-800 transition-colors"
        >
          取消
        </button>
        <button
          onClick={apply}
          className="px-4 py-1.5 text-xs font-semibold bg-cyan-600 hover:bg-cyan-500 text-white rounded shadow-lg shadow-cyan-600/10 transition-all"
        >
          保存配置
        </button>
      </div>
    </Dialog>
  );
}
