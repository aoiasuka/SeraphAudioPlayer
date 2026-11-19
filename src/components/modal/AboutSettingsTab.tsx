import { DownloadCloud, Loader2, Sparkles } from "lucide-react";
import { useState } from "react";
import { checkForUpdate, openReleasePage, type UpdateCheckResult } from "@/lib/update";
import { usePlayerStore } from "@/store/player";
import { SettingRow, ToggleButton } from "./settingsControls";

/** 设置弹窗里的「关于与更新」标签页：手动检查更新、启动时自动检查的开关。 */
export function AboutSettingsTab() {
  const [checking, setChecking] = useState(false);
  const [result, setResult] = useState<UpdateCheckResult | null>(null);
  const autoUpdateCheck = usePlayerStore((s) => s.autoUpdateCheck);
  const setAutoUpdateCheck = usePlayerStore((s) => s.setAutoUpdateCheck);
  const showNotification = usePlayerStore((s) => s.showNotification);

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
      console.warn("check_for_update failed", err);
      showNotification("检查更新失败，请稍后重试", "error");
    } finally {
      setChecking(false);
    }
  };

  const openDownload = async () => {
    if (!result?.releaseUrl) return;
    try {
      await openReleasePage(result.releaseUrl);
    } catch (err) {
      console.warn("open_release_page failed", err);
      showNotification("打开下载页失败", "error");
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
      <SettingRow
        title="启动时检查更新"
        description={
          <>
            每次启动后向 GitHub 查询一次是否有新版本。关闭后应用不会自动联网检查，
            只在点击上方「检查更新」时查询。
          </>
        }
      >
        <ToggleButton
          pressed={autoUpdateCheck}
          onToggle={() => setAutoUpdateCheck(!autoUpdateCheck)}
          label="启动时检查更新"
        />
      </SettingRow>
    </div>
  );
}
