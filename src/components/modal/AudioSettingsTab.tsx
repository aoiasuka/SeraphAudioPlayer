import { CheckCircle2, Sliders } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import type { DriverKind } from "@/types/track";

const drivers: {
  value: DriverKind;
  label: string;
  hint: string;
  disabled?: boolean;
}[] = [
  {
    value: "wasapi",
    label: "WASAPI 独占",
    hint: "已接入 Windows 独占输出；当前仍经过 PCM 混音链路，bit-perfect 旁路尚未开放。",
  },
  {
    value: "asio",
    label: "ASIO 专业驱动（未开放）",
    hint: "ASIO 后端尚未实现，发布前不会作为可选输出路径开放。",
    disabled: true,
  },
  {
    value: "direct",
    label: "系统共享输出",
    hint: "使用系统共享音频栈，兼容性最高，适合普通扬声器和蓝牙设备。",
  },
];

/** 设置弹窗里的「音频输出」标签页。驱动与设备的选择即时生效。 */
export function AudioSettingsTab() {
  const driverKind = usePlayerStore((s) => s.driverKind);
  const setDriver = usePlayerStore((s) => s.setDriver);
  const currentDeviceId = usePlayerStore((s) => s.currentDeviceId);
  const devices = usePlayerStore((s) => s.devices);
  const loadDevices = usePlayerStore((s) => s.loadDevices);
  const selectDevice = usePlayerStore((s) => s.selectDevice);

  const currentDevice = devices.find((device) => device.id === currentDeviceId);
  const currentDriver = drivers.find((driver) => driver.value === driverKind);

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
          <Sliders className="w-4 h-4 text-brown" />
          音频输出设置
        </h3>
        <p className="font-tw text-[11px] text-ink2 leading-relaxed">
          管理当前播放设备和输出偏好。
        </p>
      </div>
      <div className="space-y-1.5">
        <label
          htmlFor="settings-driver-interface"
          className="font-tw text-[9px] font-bold text-ink3 uppercase"
        >
          Driver Interface
        </label>
        <select
          id="settings-driver-interface"
          value={driverKind}
          onChange={(e) => setDriver(e.target.value as DriverKind)}
          className="w-full bg-paper2 border-[1.5px] border-ink p-2 font-tw text-xs text-ink focus:outline-none focus:border-stamp"
        >
          {drivers.map((driver) => (
            <option key={driver.value} value={driver.value} disabled={driver.disabled}>
              {driver.label}
            </option>
          ))}
        </select>
        <p className="font-tw text-[10px] text-ink3">{currentDriver?.hint}</p>
      </div>

      <div className="p-3 bg-paper2 border-[1.5px] border-ink space-y-2">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h4 className="font-serif text-xs font-semibold text-ink">当前输出设备</h4>
            <p className="font-tw text-[10px] text-ink2">
              {currentDevice?.name ?? (currentDeviceId || "正在读取设备…")}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void loadDevices()}
            className="stamp-btn px-2 py-1 font-tw text-[10px] font-bold"
          >
            刷新
          </button>
        </div>
        <select
          value={currentDeviceId}
          onChange={(event) => selectDevice(event.target.value)}
          aria-label="输出设备"
          className="w-full bg-card border-[1.5px] border-ink p-2 font-tw text-xs text-ink focus:outline-none focus:border-stamp"
        >
          {devices.map((device) => (
            <option key={device.id} value={device.id}>
              {device.name}
              {device.isDefault ? " · Default" : ""}
            </option>
          ))}
        </select>
      </div>

      <div className="p-3 bg-stamp-soft border-[1.5px] border-stamp space-y-1.5">
        <h4 className="font-serif text-xs font-semibold text-stamp flex items-center gap-1.5">
          <CheckCircle2 className="w-3.5 h-3.5" />
          当前输出能力
        </h4>
        <p className="font-tw text-[10px] text-ink2 leading-relaxed">
          本地解码、播放进度事件、系统共享输出和 WASAPI 独占输出已经由 Rust 音频线程驱动；DSD 当前使用 PCM Conversion，DoP、Native DSD、ASIO 与 bit-perfect 旁路尚未开放。
        </p>
      </div>
    </div>
  );
}
