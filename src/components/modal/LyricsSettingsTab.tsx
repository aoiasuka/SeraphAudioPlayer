import { useEffect, useState, type FormEvent } from "react";
import { Folder, Link2, Loader2, MicVocal, Plug, Plus, RotateCcw, Trash2 } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { Slider } from "@/components/ui/slider";
import { MAX_EXCLUDE_PATTERN_CHARS, MAX_EXCLUDE_RULES } from "@/lib/lyrics/exclude";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import {
  AMLL_LINKS,
  AMLL_TTML_DB_CUSTOM_PRESETS,
  AMLL_TTML_DB_PRESET_URL,
  DEFAULT_AMLL_TTML_DB_URL,
  isValidAmllTtmlDbUrl,
  LYRICS_SOURCE_OPTIONS,
  previewAmllTtmlUrl,
} from "@/lib/lyrics/settings";
import { usePlayerStore } from "@/store/player";
import type { AmllTtmlDbTestKind, AmllTtmlDbTestResult } from "@/store/player/types";
import type { LyricsExcludeRule, LyricsSourcePriority } from "@/types/track";

/** 「制作信息预设」一键加入的正则（Rust regex 语法），默认不启用，只是按钮。 */
export const CREDITS_EXCLUDE_PRESET_PATTERN =
  "^(作词|作曲|编曲|制作人|混音|母带|监制|出品|录音|和声|吉他|贝斯|鼓|键盘|弦乐|发行|OP|SP|词|曲)\\s*[:：]";

/** 命中预览最多列出的行数 */
const HIDDEN_PREVIEW_LIMIT = 30;

/** test_amll_ttml_db 各结果分类的中文说明 */
const AMLL_TEST_KIND_TEXT: Record<AmllTtmlDbTestKind, string> = {
  invalid_url: "地址无效",
  unreachable: "地址不可达（网络或超时）",
  not_found: "目标未收录（示例歌曲不存在，地址本身可达）",
  http_error: "服务返回错误",
  html: "返回的是网页而不是 TTML（通常是镜像 404 页）",
  invalid_xml: "返回内容不是有效的 TTML XML",
  unsupported_ttml: "是 TTML 但解析不出歌词行",
  ok: "连接正常",
};

function describeAmllTest(result: AmllTtmlDbTestResult) {
  if (result.kind === "ok") return `连接正常，解析到 ${result.lines} 行`;
  return AMLL_TEST_KIND_TEXT[result.kind] ?? result.message;
}

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
      // eslint-disable-next-line no-console
      console.warn("Tauri dialog unavailable", err);
      showNotification("无法打开文件夹选择窗口");
    }
  };

  const [urlDialogOpen, setUrlDialogOpen] = useState(false);
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
        title="恢复歌词设置默认值"
        description="把歌词源优先级、繁体转换、TTML 开关、AMLL 地址、排除规则、译文/音译显示与本地歌词目录全部恢复为默认；不影响任务栏歌词条设置。"
      >
        <button
          type="button"
          onClick={() => {
            if (!window.confirm("确定要把歌词设置全部恢复为默认值吗？排除规则会被清空。")) return;
            resetLyricsSettings();
          }}
          className="inline-flex h-8 shrink-0 items-center gap-1.5 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-stamp hover:text-stamp"
        >
          <RotateCcw className="h-3.5 w-3.5" />
          恢复默认
        </button>
      </SettingRow>

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

function SettingRow({
  title,
  description,
  children,
}: {
  title: React.ReactNode;
  description: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3 border-[1.5px] border-line bg-card p-3">
      <div className="min-w-0">
        <h4 className="font-serif text-xs font-semibold text-ink">{title}</h4>
        <p className="mt-0.5 font-tw text-[10px] leading-relaxed text-ink2">{description}</p>
      </div>
      {children}
    </div>
  );
}

function ToggleButton({
  pressed,
  onToggle,
  label,
  onLabel = "已启用",
  offLabel = "已停用",
}: {
  pressed: boolean;
  onToggle: () => void;
  label: string;
  onLabel?: string;
  offLabel?: string;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      aria-pressed={pressed}
      aria-label={label}
      className={
        pressed
          ? "h-8 shrink-0 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
          : "h-8 shrink-0 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink"
      }
    >
      {pressed ? onLabel : offLabel}
    </button>
  );
}

function AmllUrlDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const amllUrl = usePlayerStore((s) => s.amllTtmlDbUrl);
  const amllCustom = usePlayerStore((s) => s.amllTtmlDbCustom);
  const setAmllTtmlDbUrl = usePlayerStore((s) => s.setAmllTtmlDbUrl);
  const [draft, setDraft] = useState(amllUrl);
  const [custom, setCustom] = useState(amllCustom);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<AmllTtmlDbTestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const canTest = isTauriRuntime();

  useEffect(() => {
    if (open) {
      setDraft(amllUrl);
      setCustom(amllCustom);
      setTestResult(null);
      setTestError(null);
    }
  }, [open, amllUrl, amllCustom]);

  const effectiveDraft = custom ? draft : AMLL_TTML_DB_PRESET_URL;
  const valid = isValidAmllTtmlDbUrl(effectiveDraft, custom);
  const preview = valid ? previewAmllTtmlUrl(effectiveDraft) : "";

  // 草稿改动后旧的测试结论不再成立
  useEffect(() => {
    setTestResult(null);
    setTestError(null);
  }, [effectiveDraft, custom]);

  const runTest = async () => {
    if (!valid || testing || !canTest) return;
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    try {
      const result = await invoke<AmllTtmlDbTestResult>("test_amll_ttml_db", {
        template: effectiveDraft,
        custom,
      });
      setTestResult(result);
    } catch (err) {
      setTestError(err instanceof Error ? err.message : String(err));
    } finally {
      setTesting(false);
    }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (setAmllTtmlDbUrl(effectiveDraft, custom)) onClose();
  };

  return (
    <Dialog open={open} onClose={onClose} className="max-w-lg space-y-4">
      <span className="file-tab">FILE — LYRICS / AMLL TTML DB</span>
      <div>
        <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
          <Link2 className="w-4 h-4 text-brown" />
          AMLL TTML DB 地址
        </h3>
        <p className="mt-1 font-tw text-[11px] leading-relaxed text-ink2">
          逐字歌词按平台歌曲 ID 从该地址读取。自定义地址可用占位符：
          <code className="mx-0.5 bg-paper2 px-1">{"{dir}"}</code>= 目录（ncm-lyrics / qq-lyrics），
          <code className="mx-0.5 bg-paper2 px-1">{"{id}"}</code>或<code className="mx-0.5 bg-paper2 px-1">%s</code>= 歌曲 ID；
          不含占位符时按仓库根地址处理。
        </p>
      </div>
      <form onSubmit={submit} className="space-y-3">
        <div className="flex gap-1.5" role="radiogroup" aria-label="地址模式">
          {([
            [false, "预设", "使用内置社区镜像 amlldb.bikonoo.com，重定向逐跳复验"],
            [true, "自定义地址", "任意公网 HTTPS 域名；禁止重定向，拒绝内网/IP"],
          ] as const).map(([value, label, hint]) => (
            <button
              key={String(value)}
              type="button"
              role="radio"
              aria-checked={custom === value}
              title={hint}
              onClick={() => {
                setCustom(value);
                if (value && !draft) setDraft(AMLL_TTML_DB_PRESET_URL);
              }}
              className={
                custom === value
                  ? "h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper"
                  : "h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
              }
            >
              {label}
            </button>
          ))}
        </div>
        {custom ? (
          <>
            <div className="flex flex-wrap gap-1.5">
              {AMLL_TTML_DB_CUSTOM_PRESETS.map((mirror) => (
                <button
                  key={mirror.url}
                  type="button"
                  onClick={() => setDraft(mirror.url)}
                  className={
                    draft === mirror.url
                      ? "h-7 border-[1.5px] border-ink bg-ink px-2 font-tw text-[10px] font-bold text-paper"
                      : "h-7 border-[1.5px] border-line bg-card px-2 font-tw text-[10px] font-bold text-ink2 hover:border-ink"
                  }
                >
                  {mirror.label}
                </button>
              ))}
            </div>
            <label className="block space-y-1.5">
              <span className="block font-tw text-[9px] font-bold text-ink3 uppercase">URL Template</span>
              <input
                value={draft}
                onChange={(event) => setDraft(event.target.value)}
                spellCheck={false}
                aria-label="AMLL TTML DB 地址"
                className="w-full border-[1.5px] border-ink bg-card p-2 font-tw text-xs text-ink outline-none focus:border-stamp"
              />
            </label>
          </>
        ) : (
          <div className="space-y-1.5">
            <span className="block font-tw text-[9px] font-bold text-ink3 uppercase">Preset URL</span>
            <code className="block break-all border-[1.5px] border-line bg-paper2 p-2 font-tw text-xs text-ink">
              {AMLL_TTML_DB_PRESET_URL}
            </code>
          </div>
        )}
        <p className={`break-all font-tw text-[10px] ${valid ? "text-ink3" : "text-stamp"}`}>
          {valid
            ? `示例请求：${preview}`
            : "地址无效：必须是 https:// 公网域名，不接受 IP 直连、localhost 或内网主机名。"}
        </p>
        <p className="font-tw text-[10px] leading-relaxed text-ink3">
          相关：
          <a className="mx-1 underline hover:text-ink" href={AMLL_LINKS.repo} target="_blank" rel="noreferrer">核心仓库</a>·
          <a className="mx-1 underline hover:text-ink" href={AMLL_LINKS.apiDocs} target="_blank" rel="noreferrer">官方 API 文档</a>·
          <a className="mx-1 underline hover:text-ink" href={AMLL_LINKS.search} target="_blank" rel="noreferrer">在线检索</a>·
          <a className="mx-1 underline hover:text-ink" href={AMLL_LINKS.tool} target="_blank" rel="noreferrer">TTML 制作工具</a>
        </p>
        {testResult || testError ? (
          <div
            role="status"
            className={`border-[1.5px] px-2.5 py-2 ${
              testResult?.ok ? "border-ink bg-paper2" : "border-stamp bg-stamp-soft"
            }`}
          >
            <p className={`font-tw text-[11px] font-bold ${testResult?.ok ? "text-ink" : "text-stamp"}`}>
              {testResult ? describeAmllTest(testResult) : `测试失败：${testError}`}
            </p>
            {testResult?.url ? (
              <p className="mt-0.5 break-all font-tw text-[10px] text-ink3" title={testResult.url}>
                请求：{testResult.url}
              </p>
            ) : null}
          </div>
        ) : null}
        <div className="flex justify-between gap-2 border-t border-line pt-3">
          <button
            type="button"
            onClick={() => {
              setCustom(false);
              setDraft(DEFAULT_AMLL_TTML_DB_URL);
            }}
            className="inline-flex h-8 items-center gap-1.5 px-2 font-tw text-xs font-bold text-ink2 hover:text-ink"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            恢复默认
          </button>
          <div className="flex gap-2">
            <button
              type="button"
              onClick={onClose}
              className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
            >
              取消
            </button>
            <button
              type="button"
              onClick={() => void runTest()}
              disabled={!valid || testing || !canTest}
              title={canTest ? "向该地址请求一首示例歌曲的 TTML 并尝试解析" : "仅桌面端可测试连接"}
              className="inline-flex h-8 items-center gap-1.5 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink disabled:cursor-not-allowed disabled:opacity-50"
            >
              {testing ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Plug className="h-3.5 w-3.5" />}
              测试连接
            </button>
            <button
              type="submit"
              disabled={!valid}
              className="h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper hover:bg-stamp hover:border-stamp disabled:bg-line disabled:border-line disabled:text-ink2"
            >
              保存
            </button>
          </div>
        </div>
      </form>
    </Dialog>
  );
}

function ExcludeRulesDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const rules = usePlayerStore((s) => s.lyricsExcludeRules);
  const setRules = usePlayerStore((s) => s.setLyricsExcludeRules);
  const showNotification = usePlayerStore((s) => s.showNotification);
  const currentTrack = usePlayerStore((s) => s.currentTrack());
  const [kind, setKind] = useState<LyricsExcludeRule["kind"]>("keyword");
  const [pattern, setPattern] = useState("");
  const [regexError, setRegexError] = useState<string | null>(null);

  const trimmed = pattern.trim();

  // 正则按 Rust regex 语法校验（IPC）；浏览器开发态没有后端时退化为 JS RegExp 近似检查
  useEffect(() => {
    if (kind !== "regex" || !trimmed) {
      setRegexError(null);
      return;
    }
    let disposed = false;
    const probe: LyricsExcludeRule = { id: "probe", kind: "regex", pattern: trimmed };
    if (!isTauriRuntime()) {
      try {
        new RegExp(trimmed, "i");
        setRegexError(null);
      } catch (err) {
        setRegexError(err instanceof Error ? err.message : "无效的正则表达式");
      }
      return;
    }
    void invoke<{ id: string; error: string | null }[]>("validate_lyrics_exclude_rules", {
      rules: [probe],
    })
      .then((statuses) => {
        if (disposed) return;
        setRegexError(statuses?.[0]?.error ?? null);
      })
      .catch((err) => {
        if (!disposed) setRegexError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      disposed = true;
    };
  }, [kind, trimmed]);

  const canAdd =
    trimmed.length > 0 &&
    trimmed.length <= MAX_EXCLUDE_PATTERN_CHARS &&
    !regexError &&
    rules.length < MAX_EXCLUDE_RULES;

  const addRule = (event: FormEvent) => {
    event.preventDefault();
    if (!canAdd) return;
    setRules([
      ...rules,
      { id: `${kind}:${trimmed}:${Date.now()}`, kind, pattern: trimmed },
    ]);
    setPattern("");
  };

  const removeRule = (id: string) => setRules(rules.filter((rule) => rule.id !== id));

  // 「制作信息预设」：一键加入一条常见制作信息行正则；已有相同 pattern 则提示不重复
  const hasCreditsPreset = rules.some((rule) => rule.pattern === CREDITS_EXCLUDE_PRESET_PATTERN);
  const addCreditsPreset = () => {
    if (hasCreditsPreset) {
      showNotification("制作信息预设已存在");
      return;
    }
    if (rules.length >= MAX_EXCLUDE_RULES) return;
    setRules([
      ...rules,
      { id: `regex:credits-preset:${Date.now()}`, kind: "regex", pattern: CREDITS_EXCLUDE_PRESET_PATTERN },
    ]);
  };

  // 当前曲目命中预览：hidden 标记由后端在回传时打好，规则变更后会重拉，这里不自己匹配
  const hiddenLines = currentTrack?.lyrics.filter((line) => line.hidden === true) ?? [];
  const hiddenOverflow = Math.max(0, hiddenLines.length - HIDDEN_PREVIEW_LIMIT);

  return (
    <Dialog open={open} onClose={onClose} className="max-w-lg space-y-4">
      <span className="file-tab">FILE — LYRICS / EXCLUDE</span>
      <div>
        <h3 className="font-serif text-base font-bold text-ink">歌词排除配置</h3>
        <p className="mt-1 font-tw text-[11px] leading-relaxed text-ink2">
          命中规则的歌词行不会显示（原文、译文、音译任一命中即排除）。只影响显示，不修改已保存的歌词；
          关键词不区分大小写，正则按 Rust regex 语法（不支持环视与反向引用），由后端编译匹配。常见用法：排除「作词」「作曲」「制作人」等制作信息行。
        </p>
      </div>
      <form onSubmit={addRule} className="flex gap-1.5">
        <select
          value={kind}
          onChange={(event) => setKind(event.target.value as LyricsExcludeRule["kind"])}
          aria-label="规则类型"
          className="h-8 shrink-0 border-[1.5px] border-ink bg-paper2 px-2 font-tw text-xs text-ink focus:outline-none focus:border-stamp"
        >
          <option value="keyword">关键词</option>
          <option value="regex">正则</option>
        </select>
        <input
          value={pattern}
          onChange={(event) => setPattern(event.target.value)}
          placeholder={kind === "keyword" ? "例如：作词" : "例如：^(作词|作曲|编曲)[：:]"}
          aria-label="规则内容"
          maxLength={MAX_EXCLUDE_PATTERN_CHARS}
          className="h-8 min-w-0 flex-1 border-[1.5px] border-ink bg-card px-2.5 font-tw text-xs text-ink outline-none focus:border-stamp placeholder:text-ink3"
        />
        <button
          type="submit"
          disabled={!canAdd}
          aria-label="添加规则"
          className="stamp-btn inline-flex h-8 w-8 shrink-0 items-center justify-center disabled:cursor-not-allowed disabled:opacity-50"
        >
          <Plus className="h-3.5 w-3.5" />
        </button>
      </form>
      {regexError ? (
        <p className="font-tw text-[10px] text-stamp">正则无效：{regexError}</p>
      ) : rules.length >= MAX_EXCLUDE_RULES ? (
        <p className="font-tw text-[10px] text-stamp">最多 {MAX_EXCLUDE_RULES} 条规则。</p>
      ) : null}
      <div className="flex items-center justify-between gap-2">
        <span className="font-tw text-[10px] text-ink3">预设：</span>
        <button
          type="button"
          onClick={addCreditsPreset}
          disabled={!hasCreditsPreset && rules.length >= MAX_EXCLUDE_RULES}
          title={CREDITS_EXCLUDE_PRESET_PATTERN}
          className="h-7 border-[1.5px] border-line bg-card px-2 font-tw text-[10px] font-bold text-ink2 hover:border-ink disabled:cursor-not-allowed disabled:opacity-50"
        >
          制作信息预设
        </button>
      </div>
      <div className="max-h-[40vh] space-y-1.5 overflow-y-auto pr-1">
        {rules.length === 0 ? (
          <div className="flex h-24 items-center justify-center border-[1.5px] border-dashed border-line font-tw text-xs text-ink3">
            尚无排除规则
          </div>
        ) : (
          rules.map((rule) => (
            <div
              key={rule.id}
              className="flex items-center gap-2 border-[1.5px] border-line bg-card px-2.5 py-1.5"
            >
              <span className="shrink-0 border border-brown bg-paper2 px-1.5 py-0.5 font-tw text-[9px] font-bold text-brown">
                {rule.kind === "regex" ? "正则" : "关键词"}
              </span>
              <code className="min-w-0 flex-1 truncate font-tw text-xs text-ink" title={rule.pattern}>
                {rule.pattern}
              </code>
              <button
                type="button"
                onClick={() => removeRule(rule.id)}
                aria-label={`删除规则 ${rule.pattern}`}
                className="shrink-0 text-ink3 hover:text-stamp"
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </div>
          ))
        )}
      </div>
      <div className="space-y-1.5 border-t border-line pt-3">
        <h4 className="font-tw text-[10px] font-bold uppercase tracking-[0.14em] text-ink3">
          当前曲目命中预览
        </h4>
        {!currentTrack ? (
          <p className="font-tw text-[11px] text-ink3">当前没有播放曲目</p>
        ) : hiddenLines.length === 0 ? (
          <p className="font-tw text-[11px] text-ink3">当前曲目没有被隐藏的行</p>
        ) : (
          <ul className="max-h-[20vh] space-y-0.5 overflow-y-auto pr-1" aria-label="被隐藏的歌词行">
            {hiddenLines.slice(0, HIDDEN_PREVIEW_LIMIT).map((line, index) => (
              <li
                key={`${line.time}-${index}`}
                className="truncate font-tw text-[11px] text-ink2 line-through decoration-stamp/70"
                title={line.text}
              >
                {line.text}
              </li>
            ))}
            {hiddenOverflow > 0 ? (
              <li className="font-tw text-[10px] text-ink3">…还有 {hiddenOverflow} 行</li>
            ) : null}
          </ul>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line pt-3">
        <button
          type="button"
          onClick={onClose}
          className="h-8 border-[1.5px] border-ink bg-ink px-4 font-tw text-xs font-bold text-paper hover:bg-stamp hover:border-stamp"
        >
          完成
        </button>
      </div>
    </Dialog>
  );
}
