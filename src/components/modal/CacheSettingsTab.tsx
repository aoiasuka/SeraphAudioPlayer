import { Folder, HardDrive, Loader2, RotateCw, Trash2 } from "lucide-react";
import { useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { formatBytes, formatMb, type CacheSettingsForm } from "./useCacheSettingsForm";

/**
 * 设置弹窗里的「缓存管理」标签页。表单状态由壳通过 `form` 传入
 * （见 `useCacheSettingsForm`），这里只负责界面与「清理缓存」的二次确认。
 */
export function CacheSettingsTab({ form }: { form: CacheSettingsForm }) {
  const { status, busy } = form;
  // UI-01：清理缓存是一次点击删光全部已下载音频，先确认。
  const [clearConfirmOpen, setClearConfirmOpen] = useState(false);
  const usagePercent = Math.min(status?.usagePercent ?? 0, 100);
  // CACHE-03：换目录不搬文件，旧目录继续受管——改路径时把这一点说在前面。
  const dirChanged =
    status !== null &&
    form.cacheDir.trim() !== "" &&
    form.cacheDir.trim() !== status.settings.cacheDir;
  const previousDirs = status?.previousDirs ?? [];

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void form.save();
  };

  const confirmClear = () => {
    setClearConfirmOpen(false);
    void form.clear();
  };

  return (
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
      <form onSubmit={submit} className="p-3 bg-paper2 border-[1.5px] border-ink space-y-3">
        <div className="flex items-center justify-between gap-3">
          <div>
            <h4 className="font-serif text-xs font-semibold text-ink flex items-center gap-1.5">
              <HardDrive className="w-3.5 h-3.5 text-brown" />
              缓存管理
            </h4>
            <p className="font-tw text-[10px] text-ink2">
              {status
                ? `${status.fileCount} 个文件 · ${formatMb(status.usedMb)}`
                : "正在读取缓存状态"}
            </p>
          </div>
          <button
            type="button"
            onClick={() => void form.refresh()}
            disabled={busy}
            className="stamp-btn inline-flex h-7 items-center gap-1.5 px-2 font-tw text-[10px] font-bold disabled:opacity-50"
          >
            <RotateCw className="h-3 w-3" />
            刷新
          </button>
        </div>

        {status?.unavailableReason ? (
          <p
            role="alert"
            className="border-[1.5px] border-stamp bg-stamp-soft p-2 font-tw text-[10px] leading-relaxed text-stamp"
          >
            {status.unavailableReason}
          </p>
        ) : null}

        <div className="space-y-1.5">
          <label
            htmlFor="settings-cache-path"
            className="font-tw text-[9px] font-bold text-ink3 uppercase"
          >
            Cache Path
          </label>
          <div className="grid grid-cols-[28px_minmax(0,1fr)] items-center gap-2 bg-card border-[1.5px] border-ink px-1.5 py-1.5">
            <button
              type="button"
              onClick={() => void form.chooseDir()}
              disabled={busy}
              className="flex h-7 w-7 items-center justify-center text-ink2 transition-colors hover:bg-paper2 hover:text-brown disabled:cursor-not-allowed disabled:opacity-50"
              title="选择缓存文件夹"
              aria-label="选择缓存文件夹"
            >
              <Folder className="h-4 w-4" />
            </button>
            <input
              id="settings-cache-path"
              value={form.cacheDir}
              onChange={(event) => form.setCacheDir(event.target.value)}
              className="min-w-0 bg-transparent pr-2 font-tw text-xs text-ink outline-none"
              placeholder="输入缓存目录路径"
            />
          </div>
          {dirChanged ? (
            <p className="font-tw text-[10px] leading-relaxed text-ink3">
              保存后新的下载落到新目录。原目录里已下载的文件不会移动：仍可播放，
              并继续计入用量、参与自动清理与「清理缓存」。
            </p>
          ) : null}
        </div>

        {previousDirs.length > 0 ? (
          <div className="space-y-1.5">
            <span className="block font-tw text-[9px] font-bold text-ink3 uppercase">
              Previous Cache Paths
            </span>
            <p className="font-tw text-[10px] leading-relaxed text-ink2">
              更换缓存目录之前下载的文件还留在下面这些旧目录里：照常播放，已计入上方用量，
              自动清理与「清理缓存」同样作用于它们。清空后的旧目录会自动从这里消失。
            </p>
            <ul aria-label="旧缓存目录" className="space-y-1">
              {previousDirs.map((dir) => (
                <li key={dir.path} className="border-[1.5px] border-line bg-card px-2 py-1.5">
                  <p className="truncate font-tw text-xs text-ink" title={dir.path}>
                    {dir.path}
                  </p>
                  <p
                    className={
                      dir.unavailableReason
                        ? "font-tw text-[10px] leading-relaxed text-stamp"
                        : "font-tw text-[10px] text-ink2"
                    }
                  >
                    {dir.unavailableReason ??
                      `${dir.fileCount} 个文件 · ${formatBytes(dir.usedBytes)}`}
                  </p>
                </li>
              ))}
            </ul>
          </div>
        ) : null}

        <div className="grid gap-3 sm:grid-cols-[140px_minmax(0,1fr)]">
          <label className="space-y-1.5">
            <span className="block font-tw text-[9px] font-bold text-ink3 uppercase">
              Max Size (MB)
            </span>
            <input
              value={form.maxSizeMb}
              onChange={(event) => form.setMaxSizeMb(event.target.value)}
              inputMode="numeric"
              className="w-full border-[1.5px] border-ink bg-card p-2 font-tw text-xs text-ink outline-none focus:border-stamp"
            />
          </label>
          <div className="space-y-2">
            <div className="flex items-center justify-between font-tw text-[10px] font-bold text-ink2">
              <span>已用 {formatMb(status?.usedMb ?? 0)}</span>
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
                checked={form.autoCleanup}
                onChange={(event) => form.setAutoCleanup(event.target.checked)}
                className="h-3.5 w-3.5 accent-ink"
              />
              接近上限时自动清理最久未播放的流媒体缓存
            </label>
          </div>
        </div>

        <div className="flex flex-wrap justify-end gap-2 border-t border-line pt-3">
          <button
            type="button"
            onClick={() => setClearConfirmOpen(true)}
            disabled={busy || (status?.fileCount ?? 0) === 0}
            className="inline-flex items-center gap-1.5 px-3 py-1.5 font-tw text-xs font-bold text-stamp transition-colors hover:bg-stamp-soft disabled:cursor-not-allowed disabled:text-ink3 disabled:hover:bg-transparent"
          >
            {busy ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
            清理缓存
          </button>
          <button
            type="submit"
            disabled={busy}
            className="border-[1.5px] border-ink bg-ink px-3 py-1.5 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:bg-line disabled:border-line disabled:text-ink2"
          >
            保存缓存设置
          </button>
        </div>
      </form>

      <Dialog
        open={clearConfirmOpen}
        onClose={() => setClearConfirmOpen(false)}
        className="max-w-sm space-y-4"
      >
        <h3 className="font-serif text-sm font-semibold text-ink">清理全部缓存</h3>
        <p className="font-tw text-xs leading-5 text-ink2">
          将删除 {status?.fileCount ?? 0} 个已下载的流媒体音频文件（共{" "}
          {formatMb(status?.usedMb ?? 0)}）。曲目记录会保留，但再次播放需要重新下载。
          此操作不可撤销。
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => setClearConfirmOpen(false)}
            className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
          >
            取消
          </button>
          <button
            type="button"
            onClick={confirmClear}
            className="h-8 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper"
          >
            确认清理
          </button>
        </div>
      </Dialog>
    </div>
  );
}
