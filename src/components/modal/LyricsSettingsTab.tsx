import { useState } from "react";
import { Folder, MicVocal, RotateCcw } from "lucide-react";
import { AmllUrlDialog } from "@/components/modal/AmllUrlDialog";
import { ExcludeRulesDialog } from "@/components/modal/ExcludeRulesDialog";
import { SettingRow, ToggleButton } from "@/components/modal/settingsControls";
import { Dialog } from "@/components/ui/dialog";
import { Slider } from "@/components/ui/slider";
import { LYRICS_SOURCE_OPTIONS } from "@/lib/lyrics/settings";
import { usePlayerStore } from "@/store/player";
import type { LyricsSourcePriority } from "@/types/track";

/** 设置弹窗里的「歌词设置」标签页。开关即时生效并持久化，没有“保存”步骤。 */
export function LyricsSettingsTab() {
  const lyricsSourcePriority = usePlayerStore((s) => s.lyricsSourcePriority);
  const setLyricsSourcePriority = usePlayerStore((s) => s.setLyricsSourcePriority);
  const preferTraditional = usePlayerStore((s) => s.preferTraditionalLyrics);
  const setPreferTraditional = usePlayerStore((s) => s.setPreferTraditionalLyrics);
  const ttmlEnabled = usePlayerStore((s) => s.ttmlLyricsEnabled);
  const setTtmlEnabled = usePlayerStore((s) => s.setTtmlLyricsEnabled);
  const amllUrl = usePlayerStore((s) => s.amllTtmlDbUrl);
  const amllCustom = usePlayerStore((s) => s.amllTtmlDbCustom);
  const excludeRules = usePlayerStore((s) => s.lyricsExcludeRules);
  const showTranslation = usePlayerStore((s) => s.showLyricsTranslation);
  const setShowTranslation = usePlayerStore((s) => s.setShowLyricsTranslation);
  const showRoman = usePlayerStore((s) => s.showLyricsRoman);
  const setShowRoman = usePlayerStore((s) => s.setShowLyricsRoman);
  const showCredits = usePlayerStore((s) => s.showLyricsCredits);
  const setShowCredits = usePlayerStore((s) => s.setShowLyricsCredits);
  const ignoreFileOffset = usePlayerStore((s) => s.ignoreLyricsFileOffset);
  const setIgnoreFileOffset = usePlayerStore((s) => s.setIgnoreLyricsFileOffset);
  const taskbarLyricsEnabled = usePlayerStore((s) => s.taskbarLyricsEnabled);
  const setTaskbarLyricsEnabled = usePlayerStore((s) => s.setTaskbarLyricsEnabled);
  const taskbarLyricsClickThrough = usePlayerStore((s) => s.taskbarLyricsClickThrough);
  const setTaskbarLyricsClickThrough = usePlayerStore((s) => s.setTaskbarLyricsClickThrough);
  const taskbarLyricsPosition = usePlayerStore((s) => s.taskbarLyricsPosition);
  const setTaskbarLyricsPosition = usePlayerStore((s) => s.setTaskbarLyricsPosition);
  const lyricsFolder = usePlayerStore((s) => s.lyricsFolder);
  const setLyricsFolder = usePlayerStore((s) => s.setLyricsFolder);
  const showNotification = usePlayerStore((s) => s.showNotification);
  const resetLyricsSettings = usePlayerStore((s) => s.resetLyricsSettings);

  const chooseLyricsFolder = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        title: "选择本地歌词目录",
        defaultPath: lyricsFolder || undefined,
      });
      if (typeof selected === "string" && selected.trim()) setLyricsFolder(selected);
    } catch (err) {

      console.warn("Tauri dialog unavailable", err);
      showNotification("无法打开文件夹选择窗口");
    }
  };

  const [urlDialogOpen, setUrlDialogOpen] = useState(false);
  const [resetConfirmOpen, setResetConfirmOpen] = useState(false);
  const [excludeDialogOpen, setExcludeDialogOpen] = useState(false);

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
          <MicVocal className="w-4 h-4 text-brown" />
          歌词设置
        </h3>
        <p className="font-tw text-[11px] text-ink2 leading-relaxed">
          在线歌词来源、逐字歌词、译文显示与任务栏歌词条。
        </p>
      </div>

      <SettingRow
        title="歌词源优先级"
        description="设置在线匹配时歌词获取的优先顺序；指定源的结果排在最前，其它源仍作兜底。"
      >
        <select
          value={lyricsSourcePriority}
          onChange={(event) =>
            setLyricsSourcePriority(event.target.value as LyricsSourcePriority)
          }
          aria-label="歌词源优先级"
          className="h-8 shrink-0 border-[1.5px] border-ink bg-paper2 px-2 font-tw text-xs text-ink focus:outline-none focus:border-stamp"
        >
          {LYRICS_SOURCE_OPTIONS.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </SettingRow>

      <SettingRow
        title="更喜欢繁体中文"
        description="将简体中文的歌词文本和翻译内容转换为繁体中文。只影响之后获取或导入的歌词，已保存的歌词不改写。"
      >
        <ToggleButton
          pressed={preferTraditional}
          onToggle={() => setPreferTraditional(!preferTraditional)}
          label="繁体中文"
        />
      </SettingRow>

      <SettingRow
        title={
          <>
            启用在线 TTML 歌词
            <span className="ml-1.5 border border-brown bg-paper2 px-1 py-px font-tw text-[9px] font-bold text-brown align-middle">
              Beta
            </span>
          </>
        }
        description="在线匹配时同时从 AMLL TTML DB 获取歌词（如有）。TTML 歌词支持逐字、翻译、音译等功能，将会在下一次匹配时生效。"
      >
        <ToggleButton
          pressed={ttmlEnabled}
          onToggle={() => setTtmlEnabled(!ttmlEnabled)}
          label="在线 TTML 歌词"
        />
      </SettingRow>

      <SettingRow
        title="AMLL TTML DB 地址"
        description={
          <>
            AMLL TTML DB 地址，请确保地址正确，否则将导致歌词获取失败。默认使用社区镜像，也可切换为自定义模板地址。
            <span className="mt-1 block truncate font-tw text-[10px] text-ink3" title={amllUrl}>
              {amllCustom ? "自定义" : "预设"}：{amllUrl}
            </span>
          </>
        }
      >
        <button
          type="button"
          onClick={() => setUrlDialogOpen(true)}
          className="stamp-btn h-8 shrink-0 px-3 font-tw text-xs font-bold"
        >
          配置
        </button>
      </SettingRow>

      <SettingRow
        title="歌词排除配置"
        description={`可配置排除歌词，包含关键词或匹配正则表达式的歌词行将不会显示。${
          excludeRules.length > 0 ? `当前 ${excludeRules.length} 条规则。` : ""
        }`}
      >
        <button
          type="button"
          onClick={() => setExcludeDialogOpen(true)}
          className="stamp-btn h-8 shrink-0 px-3 font-tw text-xs font-bold"
        >
          配置
        </button>
      </SettingRow>

      <SettingRow
        title="本地歌词目录"
        description={
          <>
            切歌时若曲库没有歌词，按「艺术家 - 曲名」在该目录匹配 .lrc / .qrc / .krc / .yrc / .ttml；
            文件名括号里的网易云歌曲 ID（如 LDDC 导出的 <code className="bg-paper2 px-1">王力宏 - 唯一 (65923804).lrc</code>）会用于 AMLL 逐字歌词直取。
            <span className="mt-1 block truncate font-tw text-[10px] text-ink3" title={lyricsFolder}>
              {lyricsFolder ? `当前：${lyricsFolder}` : "未设置"}
            </span>
          </>
        }
      >
        <div className="flex shrink-0 gap-1.5">
          {lyricsFolder ? (
            <button
              type="button"
              onClick={() => setLyricsFolder("")}
              className="h-8 border-[1.5px] border-line bg-card px-2.5 font-tw text-xs font-bold text-ink2 hover:border-ink"
            >
              清除
            </button>
          ) : null}
          <button
            type="button"
            onClick={() => void chooseLyricsFolder()}
            className="stamp-btn inline-flex h-8 items-center gap-1.5 px-3 font-tw text-xs font-bold"
          >
            <Folder className="h-3.5 w-3.5" />
            选择目录
          </button>
        </div>
      </SettingRow>

      <SettingRow title="显示译文" description="歌词稿与沉浸页显示 TTML 译文或双语歌词的译文行。">
        <ToggleButton
          pressed={showTranslation}
          onToggle={() => setShowTranslation(!showTranslation)}
          label="显示译文"
        />
      </SettingRow>

      <SettingRow title="显示音译" description="TTML 歌词带罗马音/音译时，在原文下方显示。">
        <ToggleButton
          pressed={showRoman}
          onToggle={() => setShowRoman(!showRoman)}
          label="显示音译"
        />
      </SettingRow>

      <SettingRow
        title="显示制作信息"
        description="作词、作曲、编曲这类制作信息行（网易云逐字歌词开头常见）单独成块、弱化显示；关闭后三处歌词显示都不再出现它们，无需另写排除规则。"
      >
        <ToggleButton
          pressed={showCredits}
          onToggle={() => setShowCredits(!showCredits)}
          label="显示制作信息"
        />
      </SettingRow>

      <SettingRow
        title="忽略歌词文件里的 offset"
        description="LRC 文件头的 [offset:] 标签会整体提前或延后歌词；开启后按文件里写的原始时间显示（适用于 offset 写错、与本机不符的文件）。只影响显示，导出仍写曲库原始时间。"
      >
        <ToggleButton
          pressed={ignoreFileOffset}
          onToggle={() => setIgnoreFileOffset(!ignoreFileOffset)}
          label="忽略文件 offset"
        />
      </SettingRow>

      <SettingRow
        title="恢复歌词设置默认值"
        description="把歌词源优先级、繁体转换、TTML 开关、AMLL 地址、排除规则、译文/音译/制作信息显示、文件 offset 与本地歌词目录全部恢复为默认；不影响任务栏歌词条设置。"
      >
        <button
          type="button"
          onClick={() => setResetConfirmOpen(true)}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-stamp hover:text-stamp"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          恢复默认
        </button>
      </SettingRow>
      {/* FE-09：用项目自己的弹窗做二次确认，不再弹系统原生 confirm */}
      <Dialog open={resetConfirmOpen} onClose={() => setResetConfirmOpen(false)} className="max-w-sm space-y-4">
        <h3 className="font-serif text-sm font-semibold text-ink">恢复歌词设置默认值</h3>
        <p className="font-tw text-xs leading-5 text-ink2">
          确定要把歌词设置全部恢复为默认值吗？排除规则会被清空。
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={() => setResetConfirmOpen(false)}
            className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => {
              setResetConfirmOpen(false);
              resetLyricsSettings();
            }}
            className="h-8 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper"
          >
            确认恢复
          </button>
        </div>
      </Dialog>

      <h4 className="pt-2 font-tw text-[10px] tracking-[2px] text-ink3 uppercase">
        [ Taskbar / 任务栏歌词条 ]
      </h4>

      <SettingRow
        title="任务栏歌词条"
        description="在任务栏上贴一张档案纸签，实时显示当前曲目与歌词，悬停可控制播放，可拖拽调整位置。独立小窗口有少量内存开销，默认关闭。"
      >
        <ToggleButton
          pressed={taskbarLyricsEnabled}
          onToggle={() => setTaskbarLyricsEnabled(!taskbarLyricsEnabled)}
          label="任务栏歌词条"
          onLabel="已开启"
          offLabel="已关闭"
        />
      </SettingRow>

      <div className="border-[1.5px] border-line bg-card p-3 space-y-2">
        <div className="flex items-baseline justify-between gap-3">
          <h4 className="font-serif text-xs font-semibold text-ink">歌词条位置</h4>
          <span className="font-tw text-[10px] font-bold tabular-nums text-ink2">
            {Math.round(taskbarLyricsPosition * 100)}%
          </span>
        </div>
        <p className="font-tw text-[10px] leading-relaxed text-ink2">
          歌词条沿任务栏的落位：0% 最靠左端、100% 最靠右端（任务栏竖排时对应上端与下端）。直接拖动歌词条也会同步更新这里。
        </p>
        <Slider
          value={taskbarLyricsPosition}
          min={0}
          max={1}
          step={0.01}
          onChange={(event) => setTaskbarLyricsPosition(Number(event.target.value))}
          aria-label="任务栏歌词条位置"
          className="w-full"
        />
      </div>

      <SettingRow
        title="歌词条仅显示模式（鼠标穿透）"
        description="歌词条完全不响应鼠标，点击直接落到任务栏；播控、拖拽与 ✕ 均不可用，恢复交互只能回到本开关关闭。适合只想看歌词、不想挡任务栏操作的场景。"
      >
        <ToggleButton
          pressed={taskbarLyricsClickThrough}
          onToggle={() => setTaskbarLyricsClickThrough(!taskbarLyricsClickThrough)}
          label="鼠标穿透"
          onLabel="已开启"
          offLabel="已关闭"
        />
      </SettingRow>

      <AmllUrlDialog open={urlDialogOpen} onClose={() => setUrlDialogOpen(false)} />
      <ExcludeRulesDialog
        open={excludeDialogOpen}
        onClose={() => setExcludeDialogOpen(false)}
      />
    </div>
  );
}
