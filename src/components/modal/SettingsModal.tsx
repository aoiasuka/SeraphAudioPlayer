import {
  CheckCircle2,
  DownloadCloud,
  Folder,
  HardDrive,
  Loader2,
  MonitorSpeaker,
  RotateCw,
  Sliders,
  Sparkles,
  Trash2,
  X,
} from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { invoke } from "@/lib/tauri";
import { checkForUpdate, openReleasePage, type UpdateCheckResult } from "@/lib/update";
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

type SettingsTab = "audio" | "cache" | "system" | "about";

const SETTINGS_TABS: { value: SettingsTab; label: string }[] = [
  { value: "audio", label: "音频输出" },
  { value: "cache", label: "缓存管理" },
  { value: "system", label: "系统集成" },
  { value: "about", label: "关于与更新" },
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
  const smtcEnabled = usePlayerStore((s) => s.smtcEnabled);
  const setSmtcEnabled = usePlayerStore((s) => s.setSmtcEnabled);
  const rememberPlayback = usePlayerStore((s) => s.rememberPlayback);
  const setRememberPlayback = usePlayerStore((s) => s.setRememberPlayback);
  const markTracksCacheMissingByPaths = usePlayerStore(
    (s) => s.markTracksCacheMissingByPaths
  );
  const [activeTab, setActiveTab] = useState<SettingsTab>("audio");
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
        className="absolute top-4 right-4 text-ink3 hover:text-stamp"
        aria-label="关闭"
      >
        <X className="w-4 h-4" />
      </button>

      <span className="file-tab">FILE — SYSTEM / SETTINGS</span>

      {/* 功能分页：音频输出 / 缓存管理 / 系统集成 / 关于与更新 */}
      <div className="flex flex-wrap gap-1.5 border-b-[1.5px] border-line pb-3">
        {SETTINGS_TABS.map((tab) => (
          <button
            key={tab.value}
            type="button"
            onClick={() => setActiveTab(tab.value)}
            className={
              activeTab === tab.value
                ? "h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper"
                : "h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink"
            }
          >
            {tab.label}
          </button>
        ))}
      </div>

      {activeTab === "audio" && (
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
          <label className="font-tw text-[9px] font-bold text-ink3 uppercase">
            Driver Interface
          </label>
          <select
            value={driverKind}
            onChange={(e) => setDriver(e.target.value as DriverKind)}
            className="w-full bg-paper2 border-[1.5px] border-ink p-2 font-tw text-xs text-ink focus:outline-none focus:border-stamp"
          >
            {drivers.map((driver) => (
              <option
                key={driver.value}
                value={driver.value}
                disabled={driver.disabled}
              >
                {driver.label}
              </option>
            ))}
          </select>
          <p className="font-tw text-[10px] text-ink3">
            {currentDriver?.hint}
          </p>
        </div>

        <div className="p-3 bg-paper2 border-[1.5px] border-ink space-y-2">
          <div className="flex items-center justify-between gap-3">
            <div>
              <h4 className="font-serif text-xs font-semibold text-ink">
                当前输出设备
              </h4>
              <p className="font-tw text-[10px] text-ink2">
                {currentDevice?.name ?? currentDeviceId}
              </p>
            </div>
            <button
              onClick={loadDevices}
              className="stamp-btn px-2 py-1 font-tw text-[10px] font-bold"
            >
              刷新
            </button>
          </div>
          <select
            value={currentDeviceId}
            onChange={(event) => selectDevice(event.target.value)}
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
      )}

      {activeTab === "cache" && (
      <div className="space-y-4">
        <div className="space-y-2">
          <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
            <HardDrive className="w-4 h-4 text-brown" />
            缓存管理
          </h3>
          <p className="font-tw text-[11px] text-ink2 leading-relaxed">
            管理流媒体音频的本地缓存目录与容量上限。
          </p>
        </div>
        <form
          onSubmit={saveCacheSettings}
          className="p-3 bg-paper2 border-[1.5px] border-ink space-y-3"
        >
          <div className="flex items-center justify-between gap-3">
            <div>
              <h4 className="font-serif text-xs font-semibold text-ink flex items-center gap-1.5">
                <HardDrive className="w-3.5 h-3.5 text-brown" />
                缓存管理
              </h4>
              <p className="font-tw text-[10px] text-ink2">
                {cacheStatus
                  ? `${cacheStatus.fileCount} 个文件 · ${formatMb(cacheStatus.usedMb)}`
                  : "正在读取缓存状态"}
              </p>
            </div>
            <button
              type="button"
              onClick={() => void refreshCacheStatus()}
              disabled={cacheBusy}
              className="stamp-btn inline-flex h-7 items-center gap-1.5 px-2 font-tw text-[10px] font-bold disabled:opacity-50"
            >
              <RotateCw className="h-3 w-3" />
              刷新
            </button>
          </div>

          <div className="space-y-1.5">
            <label className="font-tw text-[9px] font-bold text-ink3 uppercase">
              Cache Path
            </label>
            <div className="grid grid-cols-[28px_minmax(0,1fr)] items-center gap-2 bg-card border-[1.5px] border-ink px-1.5 py-1.5">
              <button
                type="button"
                onClick={chooseCacheDir}
                disabled={cacheBusy}
                className="flex h-7 w-7 items-center justify-center text-ink2 transition-colors hover:bg-paper2 hover:text-brown disabled:cursor-not-allowed disabled:opacity-50"
                title="选择缓存文件夹"
                aria-label="选择缓存文件夹"
              >
                <Folder className="h-4 w-4" />
              </button>
              <input
                value={cacheDir}
                onChange={(event) => setCacheDir(event.target.value)}
                className="min-w-0 bg-transparent pr-2 font-tw text-xs text-ink outline-none"
                placeholder="输入缓存目录路径"
              />
            </div>
          </div>

          <div className="grid gap-3 sm:grid-cols-[140px_minmax(0,1fr)]">
            <label className="space-y-1.5">
              <span className="block font-tw text-[9px] font-bold text-ink3 uppercase">
                Max Size (MB)
              </span>
              <input
                value={maxSizeMb}
                onChange={(event) => setMaxSizeMb(event.target.value)}
                inputMode="numeric"
                className="w-full border-[1.5px] border-ink bg-card p-2 font-tw text-xs text-ink outline-none focus:border-stamp"
              />
            </label>
            <div className="space-y-2">
              <div className="flex items-center justify-between font-tw text-[10px] font-bold text-ink2">
                <span>已用 {formatMb(cacheStatus?.usedMb ?? 0)}</span>
                <span>{usagePercent.toFixed(1)}%</span>
              </div>
              <div className="h-2.5 overflow-hidden border border-ink bg-card">
                <div
                  className="h-full bg-brown transition-all"
                  style={{ width: `${usagePercent}%` }}
                />
              </div>
              <label className="flex items-center gap-2 font-tw text-[10px] font-bold text-ink2">
                <input
                  type="checkbox"
                  checked={autoCleanup}
                  onChange={(event) => setAutoCleanup(event.target.checked)}
                  className="h-3.5 w-3.5 accent-ink"
                />
                接近上限时自动清理最旧的流媒体缓存
              </label>
            </div>
          </div>

          <div className="flex flex-wrap justify-end gap-2 border-t border-line pt-3">
            <button
              type="button"
              onClick={clearAppCache}
              disabled={cacheBusy || (cacheStatus?.fileCount ?? 0) === 0}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 font-tw text-xs font-bold text-stamp transition-colors hover:bg-stamp-soft disabled:cursor-not-allowed disabled:text-ink3 disabled:hover:bg-transparent"
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
              className="border-[1.5px] border-ink bg-ink px-3 py-1.5 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:bg-line disabled:border-line disabled:text-ink2"
            >
              保存缓存设置
            </button>
          </div>
        </form>
      </div>
      )}

      {activeTab === "system" && (
      <div className="space-y-4">
        <div className="space-y-2">
          <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
            <MonitorSpeaker className="w-4 h-4 text-brown" />
            系统集成
          </h3>
          <p className="font-tw text-[11px] text-ink2 leading-relaxed">
            与 Windows 系统的集成能力。
          </p>
        </div>
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-line bg-card p-3">
          <div className="min-w-0">
            <h4 className="font-serif text-xs font-semibold text-ink">
              系统媒体控件（SMTC）
            </h4>
            <p className="mt-0.5 font-tw text-[10px] leading-relaxed text-ink2">
              键盘/蓝牙媒体键控制播放；系统音量浮窗与锁屏显示曲目标题、艺术家与封面。
              停用后系统界面不再展示本应用的播放内容，媒体键也不再生效。
            </p>
          </div>
          <button
            type="button"
            onClick={() => setSmtcEnabled(!smtcEnabled)}
            className={
              smtcEnabled
                ? "h-8 shrink-0 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
                : "h-8 shrink-0 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink"
            }
            aria-pressed={smtcEnabled}
          >
            {smtcEnabled ? "已启用" : "已停用"}
          </button>
        </div>
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-line bg-card p-3">
          <div className="min-w-0">
            <h4 className="font-serif text-xs font-semibold text-ink">
              记忆播放
            </h4>
            <p className="mt-0.5 font-tw text-[10px] leading-relaxed text-ink2">
              重启应用后自动恢复上次播放的曲目与播放位置。关闭后每次启动都从头开始，
              且不会在本地记录上次的播放进度。
            </p>
          </div>
          <button
            type="button"
            onClick={() => setRememberPlayback(!rememberPlayback)}
            className={
              rememberPlayback
                ? "h-8 shrink-0 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
                : "h-8 shrink-0 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink"
            }
            aria-pressed={rememberPlayback}
          >
            {rememberPlayback ? "已启用" : "已停用"}
          </button>
        </div>
      </div>
      )}

      {activeTab === "about" && (
        <UpdateSection showNotification={showNotification} />
      )}

      <div className="flex justify-end gap-3 pt-3 border-t border-line">
        <button
          onClick={toggleSettings}
          className="px-4 py-1.5 font-tw text-xs font-bold text-ink2 hover:text-ink transition-colors"
        >
          取消
        </button>
        <button
          onClick={apply}
          className="border-[1.5px] border-ink bg-ink px-4 py-1.5 font-tw text-xs font-bold text-paper hover:bg-stamp hover:border-stamp transition-all"
        >
          保存配置
        </button>
      </div>
    </Dialog>
  );
}

function UpdateSection({
  showNotification,
}: {
  showNotification: (message: string) => void;
}) {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateCheckResult | null>(null);

  const runCheck = async () => {
    if (checking) return;
    setChecking(true);
    try {
      const checked = await checkForUpdate();
      setResult(checked);
      if (!checked.updateAvailable) {
        showNotification(`已是最新版本 v${checked.currentVersion}`);
      }
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("check_for_update failed", err);
      showNotification("检查更新失败，请稍后重试");
    } finally {
      setChecking(false);
    }
  };

  const openDownload = async () => {
    if (!result?.releaseUrl) return;
    try {
      await openReleasePage(result.releaseUrl);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("open_release_page failed", err);
      showNotification("打开下载页失败");
    }
  };

  return (
    <div className="space-y-2">
      <h3 className="font-tw text-[10px] tracking-[2px] text-ink3 uppercase">
        [ 04 // About / 关于与更新 ]
      </h3>
      <div className="flex items-center justify-between gap-3 border-[1.5px] border-line bg-card p-3">
        <div className="min-w-0">
          <p className="font-serif text-xs font-semibold text-ink">
            Seraph Audio Player
            {result ? ` v${result.currentVersion}` : ""}
          </p>
          <p className="mt-0.5 font-tw text-[10px] text-ink2">
            {result === null
              ? "检查是否有新版本可用"
              : result.updateAvailable
                ? `发现新版本 v${result.latestVersion}，可前往下载页获取安装包`
                : `已是最新版本`}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {result?.updateAvailable ? (
            <button
              type="button"
              onClick={() => void openDownload()}
              className="inline-flex h-8 items-center gap-1.5 border-[1.5px] border-ink bg-ink px-2.5 font-tw text-[11px] font-bold text-paper transition-colors hover:bg-stamp"
            >
              <DownloadCloud className="h-3.5 w-3.5" />
              前往下载
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => void runCheck()}
            disabled={checking}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-2.5 font-tw text-[11px] font-bold disabled:cursor-not-allowed disabled:opacity-50"
          >
            {checking ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Sparkles className="h-3.5 w-3.5" />
            )}
            检查更新
          </button>
        </div>
      </div>
    </div>
  );
}
