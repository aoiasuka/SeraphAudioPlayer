import {
  CheckCircle2,
  Folder,
  HardDrive,
  Loader2,
  RotateCw,
  Sliders,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import type { DriverKind } from "@/types/track";

const drivers: { value: DriverKind; label: string; hint: string }[] = [
  {
    value: "wasapi",
    label: "WASAPI 独占",
    hint: "以当前 PCM 输出格式打开 Windows WASAPI 独占设备，失败时提示，不静默降级。",
  },
  {
    value: "asio",
    label: "ASIO 专业驱动",
    hint: "尚未实现，选择后播放会提示暂不支持。",
  },
  {
    value: "direct",
    label: "系统共享输出",
    hint: "使用系统共享音频栈，兼容性最高，适合普通扬声器和蓝牙设备。",
  },
];

interface CacheSettings {
  cacheDir: string;
  maxSizeMb: number;
  autoCleanup: boolean;
}

interface CacheStatus {
  settings: CacheSettings;
  usedMb: number;
  usagePercent: number;
  fileCount: number;
}

interface CacheCleanupResult {
  removedFiles: number;
  removedBytes: number;
  removedPaths: string[];
}

function formatMb(value: number) {
  if (!Number.isFinite(value)) return "0 MB";
  if (value >= 1024) return `${(value / 1024).toFixed(2)} GB`;
  return `${value.toFixed(1)} MB`;
}

function formatBytes(value: number) {
  return formatMb(value / 1024 / 1024);
}

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
  const markTracksCacheMissingByPaths = usePlayerStore(
    (s) => s.markTracksCacheMissingByPaths
  );
  const [cacheStatus, setCacheStatus] = useState<CacheStatus | null>(null);
  const [cacheDir, setCacheDir] = useState("");
  const [maxSizeMb, setMaxSizeMb] = useState("5120");
  const [autoCleanup, setAutoCleanup] = useState(true);
  const [cacheBusy, setCacheBusy] = useState(false);

  const currentDevice = devices.find((device) => device.id === currentDeviceId);
  const currentDriver = drivers.find((driver) => driver.value === driverKind);
  const usagePercent = Math.min(cacheStatus?.usagePercent ?? 0, 100);

  const refreshCacheStatus = async () => {
    try {
      const status = await invoke<CacheStatus>("get_cache_status");
      setCacheStatus(status);
      setCacheDir(status.settings.cacheDir);
      setMaxSizeMb(String(status.settings.maxSizeMb));
      setAutoCleanup(status.settings.autoCleanup);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: get_cache_status", err);
      showNotification("读取缓存设置失败");
    }
  };

  useEffect(() => {
    if (!open) return;
    void refreshCacheStatus();
  }, [open]);

  const apply = () => {
    toggleSettings();
    showNotification(`已保存输出配置: ${currentDriver?.label ?? driverKind}`);
  };

  const saveCacheSettings = async (event: FormEvent) => {
    event.preventDefault();
    if (cacheBusy) return;

    const parsedMax = Number(maxSizeMb);
    if (!Number.isFinite(parsedMax) || parsedMax < 128) {
      showNotification("缓存上限不能低于 128 MB");
      return;
    }

    setCacheBusy(true);
    try {
      const status = await invoke<CacheStatus>("update_cache_settings", {
        settings: {
          cacheDir,
          maxSizeMb: Math.round(parsedMax),
          autoCleanup,
        },
      });
      setCacheStatus(status);
      setCacheDir(status.settings.cacheDir);
      setMaxSizeMb(String(status.settings.maxSizeMb));
      setAutoCleanup(status.settings.autoCleanup);
      showNotification("缓存设置已保存");
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: update_cache_settings", err);
      showNotification(`保存缓存设置失败：${String(err)}`);
    } finally {
      setCacheBusy(false);
    }
  };

  const clearAppCache = async () => {
    if (cacheBusy) return;
    setCacheBusy(true);
    try {
      const result = await invoke<CacheCleanupResult>("clear_cache");
      markTracksCacheMissingByPaths(result.removedPaths);
      await refreshCacheStatus();
      showNotification(
        `已清理 ${result.removedFiles} 个缓存文件，释放 ${formatBytes(result.removedBytes)}`
      );
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: clear_cache", err);
      showNotification(`清理缓存失败：${String(err)}`);
    } finally {
      setCacheBusy(false);
    }
  };

  const chooseCacheDir = async () => {
    if (cacheBusy) return;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择缓存文件夹",
        defaultPath: cacheDir || undefined,
      });
      if (typeof selected === "string" && selected.trim()) {
        setCacheDir(selected);
      }
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri dialog unavailable", err);
      showNotification("无法打开文件夹选择窗口");
    }
  };

  return (
    <Dialog
      open={open}
      onClose={toggleSettings}
      className="max-h-[88vh] max-w-2xl space-y-6 overflow-y-auto"
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

        <form
          onSubmit={saveCacheSettings}
          className="p-3 bg-white/60 rounded-lg border border-black/[0.04] space-y-3"
        >
          <div className="flex items-center justify-between gap-3">
            <div>
              <h4 className="text-xs font-semibold text-slate-800 flex items-center gap-1.5">
                <HardDrive className="w-3.5 h-3.5 text-cyan-700" />
                缓存管理
              </h4>
              <p className="text-[10px] text-slate-500">
                {cacheStatus
                  ? `${cacheStatus.fileCount} 个文件 · ${formatMb(cacheStatus.usedMb)}`
                  : "正在读取缓存状态"}
              </p>
            </div>
            <button
              type="button"
              onClick={() => void refreshCacheStatus()}
              disabled={cacheBusy}
              className="inline-flex h-7 items-center gap-1.5 rounded bg-cyan-600/10 px-2 text-[10px] font-semibold text-cyan-700 transition-colors hover:bg-cyan-600/15 disabled:opacity-50"
            >
              <RotateCw className="h-3 w-3" />
              刷新
            </button>
          </div>

          <div className="space-y-1.5">
            <label className="text-[9px] font-bold text-slate-400 uppercase">
              Cache Path
            </label>
            <div className="grid grid-cols-[28px_minmax(0,1fr)] items-center gap-2 rounded-lg bg-[#f1f5f9] border border-black/5 px-1.5 py-1.5">
              <button
                type="button"
                onClick={chooseCacheDir}
                disabled={cacheBusy}
                className="flex h-7 w-7 items-center justify-center rounded-md text-slate-400 transition-colors hover:bg-white hover:text-cyan-700 disabled:cursor-not-allowed disabled:opacity-50"
                title="选择缓存文件夹"
                aria-label="选择缓存文件夹"
              >
                <Folder className="h-4 w-4" />
              </button>
              <input
                value={cacheDir}
                onChange={(event) => setCacheDir(event.target.value)}
                className="min-w-0 bg-transparent pr-2 text-xs text-slate-700 outline-none"
                placeholder="输入缓存目录路径"
              />
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-[140px_minmax(0,1fr)]">
            <label className="space-y-1.5">
              <span className="block text-[9px] font-bold text-slate-400 uppercase">
                Max Size (MB)
              </span>
              <input
                value={maxSizeMb}
                onChange={(event) => setMaxSizeMb(event.target.value)}
                inputMode="numeric"
                className="w-full rounded-lg border border-black/5 bg-[#f1f5f9] p-2 text-xs text-slate-700 outline-none focus:border-cyan-600"
              />
            </label>
            <div className="space-y-2">
              <div className="flex items-center justify-between text-[10px] font-semibold text-slate-500">
                <span>已用 {formatMb(cacheStatus?.usedMb ?? 0)}</span>
                <span>{usagePercent.toFixed(1)}%</span>
              </div>
              <div className="h-2 overflow-hidden rounded-full bg-slate-200">
                <div
                  className="h-full rounded-full bg-cyan-600 transition-all"
                  style={{ width: `${usagePercent}%` }}
                />
              </div>
              <label className="flex items-center gap-2 text-[10px] font-semibold text-slate-600">
                <input
                  type="checkbox"
                  checked={autoCleanup}
                  onChange={(event) => setAutoCleanup(event.target.checked)}
                  className="h-3.5 w-3.5 accent-cyan-700"
                />
                接近上限时自动清理最旧的流媒体缓存
              </label>
            </div>
          </div>

          <div className="flex flex-wrap justify-end gap-2 border-t border-black/5 pt-3">
            <button
              type="button"
              onClick={clearAppCache}
              disabled={cacheBusy || (cacheStatus?.fileCount ?? 0) === 0}
              className="inline-flex items-center gap-1.5 rounded px-3 py-1.5 text-xs font-semibold text-rose-600 transition-colors hover:bg-rose-500/10 disabled:cursor-not-allowed disabled:text-slate-300 disabled:hover:bg-transparent"
            >
              {cacheBusy ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <Trash2 className="h-3.5 w-3.5" />
              )}
              清理缓存
            </button>
            <button
              type="submit"
              disabled={cacheBusy}
              className="rounded bg-slate-800 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-slate-900 disabled:bg-slate-300"
            >
              保存缓存设置
            </button>
          </div>
        </form>

        <div className="p-3 bg-cyan-50 rounded-lg border border-cyan-500/10 space-y-1.5">
          <h4 className="text-xs font-semibold text-cyan-800 flex items-center gap-1.5">
            <CheckCircle2 className="w-3.5 h-3.5" />
            输出链路已接入
          </h4>
          <p className="text-[10px] text-slate-500 leading-relaxed">
            本地解码、播放进度事件和系统输出设备已经由 Rust 音频线程驱动；DSD 当前使用 PCM Conversion，DoP 与 Native DSD 尚未开启。
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
