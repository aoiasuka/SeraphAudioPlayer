import { MonitorSpeaker } from "lucide-react";
import {
  buildConfigExport,
  parseConfigImport,
  stashPendingImport,
} from "@/lib/configTransfer";
import { invoke, normalizeIpcError } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import {
  buildPersonalDataExport,
  mergePersonalData,
  parsePersonalDataImport,
} from "@/store/player/personalData";
import { SettingRow, ToggleButton } from "./settingsControls";

/**
 * 设置弹窗里的「系统集成」标签页：Windows 集成开关（即时生效）、
 * 应用配置与个人数据的导出 / 导入。
 */
export function SystemSettingsTab() {
  const smtcEnabled = usePlayerStore((s) => s.smtcEnabled);
  const setSmtcEnabled = usePlayerStore((s) => s.setSmtcEnabled);
  const taskbarButtonsEnabled = usePlayerStore((s) => s.taskbarButtonsEnabled);
  const setTaskbarButtonsEnabled = usePlayerStore((s) => s.setTaskbarButtonsEnabled);
  const taskbarProgressEnabled = usePlayerStore((s) => s.taskbarProgressEnabled);
  const setTaskbarProgressEnabled = usePlayerStore((s) => s.setTaskbarProgressEnabled);
  const rememberPlayback = usePlayerStore((s) => s.rememberPlayback);
  const setRememberPlayback = usePlayerStore((s) => s.setRememberPlayback);
  const showNotification = usePlayerStore((s) => s.showNotification);

  const exportConfig = async () => {
    try {
      const content = buildConfigExport();
      if (!content) {
        showNotification("没有可导出的设置");
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        title: "导出应用配置",
        defaultPath: "seraph-config.json",
        filters: [{ name: "Seraph 配置", extensions: ["json"] }],
      });
      if (!target) return;
      await invoke("export_app_config", { path: target, content });
      showNotification("配置已导出");
    } catch (err) {
      console.warn("export_app_config failed", err);
      showNotification(`导出配置失败：${normalizeIpcError(err).message}`, "error");
    }
  };

  const importConfig = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        title: "导入应用配置",
        filters: [{ name: "Seraph 配置", extensions: ["json"] }],
      });
      if (typeof selected !== "string" || !selected.trim()) return;
      const text = await invoke<string>("import_app_config", { path: selected });
      const stores = parseConfigImport(text);
      stashPendingImport(stores);
      showNotification("配置已导入，界面即将重载…");
      window.setTimeout(() => window.location.reload(), 600);
    } catch (err) {
      // UI-02：import_app_config 的错误是 { code, message } 普通对象，
      // String(err) 会显示成 [object Object]。
      console.warn("import_app_config failed", err);
      showNotification(`导入配置失败：${normalizeIpcError(err).message}`, "error");
    }
  };

  const exportPersonalData = async () => {
    try {
      const { liked, likedAt, userPlaylists } = usePlayerStore.getState();
      const content = buildPersonalDataExport({ liked, likedAt, userPlaylists });
      if (!content) {
        showNotification("还没有收藏或歌单可以导出");
        return;
      }
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        title: "导出收藏与歌单",
        defaultPath: "seraph-personal-data.json",
        filters: [{ name: "Seraph 个人数据", extensions: ["json"] }],
      });
      if (!target) return;
      await invoke("export_app_config", { path: target, content });
      showNotification("收藏与歌单已导出");
    } catch (err) {
      console.warn("export personal data failed", err);
      showNotification(`导出收藏与歌单失败：${normalizeIpcError(err).message}`, "error");
    }
  };

  const importPersonalData = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        title: "导入收藏与歌单",
        filters: [{ name: "Seraph 个人数据", extensions: ["json"] }],
      });
      if (typeof selected !== "string" || !selected.trim()) return;
      const text = await invoke<string>("import_app_config", { path: selected });
      const imported = parsePersonalDataImport(text);
      // 读文件期间用户可能又改了收藏：合并时取最新状态，只追加不覆盖。
      const { liked, likedAt, userPlaylists } = usePlayerStore.getState();
      const merged = mergePersonalData({ liked, likedAt, userPlaylists }, imported);
      usePlayerStore.setState({
        liked: merged.liked,
        likedAt: merged.likedAt,
        userPlaylists: merged.userPlaylists,
      });
      showNotification(
        `已合并：新增收藏 ${merged.addedLikes} 首、新增歌单 ${merged.addedPlaylists} 张、补全歌单 ${merged.mergedPlaylists} 张`
      );
    } catch (err) {
      console.warn("import personal data failed", err);
      showNotification(`导入收藏与歌单失败：${normalizeIpcError(err).message}`, "error");
    }
  };

  return (
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
      <SettingRow
        title="系统媒体控件（SMTC）"
        description={
          <>
            键盘/蓝牙媒体键控制播放；系统音量浮窗与锁屏显示曲目标题、艺术家与封面。
            停用后系统界面不再展示本应用的播放内容，媒体键也不再生效。
          </>
        }
      >
        <ToggleButton
          pressed={smtcEnabled}
          onToggle={() => setSmtcEnabled(!smtcEnabled)}
          label="系统媒体控件"
        />
      </SettingRow>
      <SettingRow
        title="任务栏播控按钮"
        description={
          <>
            鼠标悬停任务栏图标时，缩略图下方显示上一首 / 播放暂停 / 下一首按钮，
            不切换窗口即可控制播放。
          </>
        }
      >
        <ToggleButton
          pressed={taskbarButtonsEnabled}
          onToggle={() => setTaskbarButtonsEnabled(!taskbarButtonsEnabled)}
          label="任务栏播控按钮"
        />
      </SettingRow>
      <SettingRow
        title="任务栏播放进度"
        description="在任务栏图标上以进度条形式显示当前曲目的播放进度，暂停时变为黄色。"
      >
        <ToggleButton
          pressed={taskbarProgressEnabled}
          onToggle={() => setTaskbarProgressEnabled(!taskbarProgressEnabled)}
          label="任务栏播放进度"
        />
      </SettingRow>
      <SettingRow
        title="记忆播放"
        description={
          <>
            重启应用后自动恢复上次播放的曲目与播放位置。关闭后每次启动都从头开始，
            且不会在本地记录上次的播放进度。
          </>
        }
      >
        <ToggleButton
          pressed={rememberPlayback}
          onToggle={() => setRememberPlayback(!rememberPlayback)}
          label="记忆播放"
        />
      </SettingRow>
      <div className="border-[1.5px] border-line bg-card p-3 space-y-2">
        <h4 className="font-serif text-xs font-semibold text-ink">
          配置备份 · 导出 / 导入
        </h4>
        <p className="font-tw text-[10px] leading-relaxed text-ink2">
          把播放偏好、EQ 与 DSP、声学分析设置一键导出为 JSON 文件；导入后界面
          自动重载并应用。不包含曲库、收藏、歌单与缓存数据——导入不会覆盖本机
          的个人数据。
        </p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void exportConfig()}
            className="h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
          >
            导出配置…
          </button>
          <button
            type="button"
            onClick={() => void importConfig()}
            className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink hover:text-ink"
          >
            导入配置…
          </button>
        </div>
      </div>
      <div className="border-[1.5px] border-line bg-card p-3 space-y-2">
        <h4 className="font-serif text-xs font-semibold text-ink">
          收藏与歌单 · 导出 / 导入
        </h4>
        <p className="font-tw text-[10px] leading-relaxed text-ink2">
          收藏和自建歌单只保存在本机的界面存储里，清理 WebView2 数据或重装系统
          会丢失，建议定期导出备份。导入是合并：本机已有的收藏和歌单不会被删除
          或覆盖。曲目按曲库里的 ID 对应，换了曲库路径的曲目需要重新收藏。
        </p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => void exportPersonalData()}
            className="h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
          >
            导出收藏与歌单…
          </button>
          <button
            type="button"
            onClick={() => void importPersonalData()}
            className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink hover:text-ink"
          >
            导入收藏与歌单…
          </button>
        </div>
      </div>
    </div>
  );
}
