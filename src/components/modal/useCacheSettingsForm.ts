import { useCallback, useEffect, useState } from "react";
import { invoke, normalizeIpcError } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

export interface CacheSettings {
  cacheDir: string;
  maxSizeMb: number;
  autoCleanup: boolean;
}

/** CACHE-03：换过缓存目录后仍留有文件的旧目录。 */
export interface PreviousCacheDirStatus {
  path: string;
  usedBytes: number;
  fileCount: number;
  /** 目录当前管不到（磁盘没接、目录或标记不在了）时的说明；此时不计入用量、不会被清理。 */
  unavailableReason?: string;
}

export interface CacheStatus {
  settings: CacheSettings;
  /** 用量与文件数是当前目录加上各旧目录的合计 */
  usedMb: number;
  usagePercent: number;
  fileCount: number;
  /** CACHE-01：缓存目录所在磁盘当前不可用时的说明；设置本身保留。 */
  unavailableReason?: string;
  /** CACHE-03：各旧缓存目录的用量明细；没有旧目录时后端不带这个字段。 */
  previousDirs?: PreviousCacheDirStatus[];
}

interface CacheCleanupResult {
  removedFiles: number;
  removedBytes: number;
  removedPaths: string[];
}

export function formatMb(value: number) {
  if (!Number.isFinite(value)) return "0 MB";
  if (value >= 1024) return `${(value / 1024).toFixed(2)} GB`;
  return `${value.toFixed(1)} MB`;
}

export function formatBytes(value: number) {
  return formatMb(value / 1024 / 1024);
}

/**
 * 缓存设置表单的状态与动作。
 *
 * 由设置弹窗的壳持有（而不是缓存标签页自己）：缓存是设置页里唯一需要「保存」的
 * 表单，未保存的改动要在切换标签页后保留，关闭弹窗时壳还要据此询问或自动保存。
 * `open` 由假变真时重新读取后端状态。
 */
export function useCacheSettingsForm(open: boolean) {
  const showNotification = usePlayerStore((s) => s.showNotification);
  const markTracksCacheMissingByPaths = usePlayerStore(
    (s) => s.markTracksCacheMissingByPaths
  );
  const [status, setStatus] = useState<CacheStatus | null>(null);
  const [cacheDir, setCacheDir] = useState("");
  const [maxSizeMb, setMaxSizeMb] = useState("5120");
  const [autoCleanup, setAutoCleanup] = useState(true);
  const [busy, setBusy] = useState(false);

  /** 用后端返回的状态覆盖表单（读取、保存成功之后） */
  const adopt = useCallback((next: CacheStatus) => {
    setStatus(next);
    setCacheDir(next.settings.cacheDir);
    setMaxSizeMb(String(next.settings.maxSizeMb));
    setAutoCleanup(next.settings.autoCleanup);
  }, []);

  const refresh = useCallback(async () => {
    try {
      adopt(await invoke<CacheStatus>("get_cache_status"));
    } catch (err) {
      console.warn("Tauri command failed: get_cache_status", err);
      showNotification("读取缓存设置失败", "error");
    }
  }, [adopt, showNotification]);

  useEffect(() => {
    if (!open) return;
    void refresh();
  }, [open, refresh]);

  const dirty =
    status !== null &&
    (cacheDir !== status.settings.cacheDir ||
      maxSizeMb !== String(status.settings.maxSizeMb) ||
      autoCleanup !== status.settings.autoCleanup);

  /** 提交表单。返回是否保存成功；失败时已提示原因，表单保持原样供修正。 */
  const save = async (): Promise<boolean> => {
    if (busy) return false;

    const parsedMax = Number(maxSizeMb);
    if (!Number.isFinite(parsedMax) || parsedMax < 128) {
      showNotification("缓存上限不能低于 128 MB");
      return false;
    }

    setBusy(true);
    try {
      adopt(
        await invoke<CacheStatus>("update_cache_settings", {
          settings: {
            cacheDir,
            maxSizeMb: Math.round(parsedMax),
            autoCleanup,
          },
        })
      );
      showNotification("缓存设置已保存");
      return true;
    } catch (err) {
      console.warn("Tauri command failed: update_cache_settings", err);
      showNotification(`保存缓存设置失败：${normalizeIpcError(err).message}`, "error");
      return false;
    } finally {
      setBusy(false);
    }
  };

  /** 丢弃未保存的改动，表单回到上次读取到的设置 */
  const discard = () => {
    if (status) adopt(status);
  };

  /** 清理全部缓存（调用方负责先向用户确认） */
  const clear = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await invoke<CacheCleanupResult>("clear_cache");
      markTracksCacheMissingByPaths(result.removedPaths);
      await refresh();
      showNotification(
        `已清理 ${result.removedFiles} 个缓存文件，释放 ${formatBytes(result.removedBytes)}`
      );
    } catch (err) {
      console.warn("Tauri command failed: clear_cache", err);
      showNotification(`清理缓存失败：${normalizeIpcError(err).message}`, "error");
    } finally {
      setBusy(false);
    }
  };

  const chooseDir = async () => {
    if (busy) return;
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const selected = await openDialog({
        directory: true,
        multiple: false,
        title: "选择缓存文件夹",
        defaultPath: cacheDir || undefined,
      });
      if (typeof selected === "string" && selected.trim()) {
        setCacheDir(selected);
      }
    } catch (err) {
      console.warn("Tauri dialog unavailable", err);
      showNotification("无法打开文件夹选择窗口");
    }
  };

  return {
    status,
    cacheDir,
    setCacheDir,
    maxSizeMb,
    setMaxSizeMb,
    autoCleanup,
    setAutoCleanup,
    busy,
    dirty,
    refresh,
    save,
    discard,
    clear,
    chooseDir,
  };
}

export type CacheSettingsForm = ReturnType<typeof useCacheSettingsForm>;
