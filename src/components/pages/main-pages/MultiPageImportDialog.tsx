import { useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import type { BilibiliMultiPageMode, BilibiliVideoInfo } from "@/store/player";

/** 弹窗里最多列出这么多个分 P，其余只报数量。 */
const PREVIEW_PAGES = 5;

function formatDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) return "";
  const total = Math.round(seconds);
  const minutes = Math.floor(total / 60);
  return `${minutes}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * BILI-05：粘贴的链接指向多 P 视频、又没有指定分 P 时，先问清楚导入范围。
 * 默认选中项跟随「设置 → 流媒体」；调用方在每次询问时重新挂载本组件（见 `key`）。
 */
export function MultiPageImportDialog({
  info,
  defaultMode,
  onConfirm,
  onCancel,
}: {
  info: BilibiliVideoInfo;
  defaultMode: BilibiliMultiPageMode;
  onConfirm: (mode: BilibiliMultiPageMode) => void;
  onCancel: () => void;
}) {
  const count = info.pages.length;
  // 分 P 太多时后端不接受一次全量导入：在这里就说明，而不是选了才报错。
  const limit = info.fullImportLimit;
  const overLimit = typeof limit === "number" && count > limit;
  const [mode, setMode] = useState<BilibiliMultiPageMode>(overLimit ? "first" : defaultMode);
  const preview = info.pages.slice(0, PREVIEW_PAGES);

  const options: {
    value: BilibiliMultiPageMode;
    label: string;
    hint: string;
    disabled?: boolean;
  }[] = [
    {
      value: "first",
      label: "仅导入首 P",
      hint: "只导入第 1 P，曲名用视频标题。",
    },
    {
      value: "all",
      label: "全量导入",
      hint: overLimit
        ? `分 P 超过 ${limit} 个，不支持一次全量导入；请用带 ?p= 的链接逐个导入。`
        : `导入全部 ${count} 个分 P：每个分 P 一首，曲名用分 P 标题，专辑名用视频标题。`,
      disabled: overLimit,
    },
  ];

  return (
    <Dialog open onClose={onCancel} className="max-w-md space-y-4">
      <h3 className="font-serif text-sm font-semibold text-ink">这个视频有 {count} 个分 P</h3>
      <p className="truncate font-tw text-xs text-ink2" title={info.title}>
        {info.title}
      </p>

      <ol aria-label="分 P 列表" className="space-y-0.5 border-[1.5px] border-line bg-paper2/40 p-2">
        {preview.map((page) => (
          <li key={page.page} className="flex items-baseline gap-2 font-tw text-[11px] text-ink2">
            <span className="w-7 shrink-0 font-bold text-ink3">P{page.page}</span>
            <span className="min-w-0 flex-1 truncate" title={page.title}>
              {page.title.trim() || "（无标题）"}
            </span>
            <span className="shrink-0 text-ink3">{formatDuration(page.duration)}</span>
          </li>
        ))}
        {count > preview.length ? (
          <li className="font-tw text-[11px] text-ink3">…… 其余 {count - preview.length} 个</li>
        ) : null}
      </ol>

      <fieldset className="space-y-2">
        <legend className="sr-only">导入范围</legend>
        {options.map((option) => (
          <label
            key={option.value}
            className={
              (option.disabled
                ? "cursor-not-allowed border-line opacity-60"
                : mode === option.value
                  ? "cursor-pointer border-ink"
                  : "cursor-pointer border-line hover:border-ink") +
              " flex items-start gap-2 border-[1.5px] bg-card p-2.5 transition-colors"
            }
          >
            <input
              type="radio"
              name="bilibili-multi-page"
              value={option.value}
              checked={mode === option.value}
              disabled={option.disabled}
              onChange={() => setMode(option.value)}
              className="mt-0.5 h-3.5 w-3.5 accent-ink"
            />
            <span className="min-w-0">
              <span className="block font-tw text-xs font-bold text-ink">{option.label}</span>
              <span className="mt-0.5 block font-tw text-[10px] leading-relaxed text-ink2">
                {option.hint}
              </span>
            </span>
          </label>
        ))}
      </fieldset>

      <p className="font-tw text-[10px] leading-relaxed text-ink3">
        只想要其中某一 P，用带 ?p= 的链接导入。默认选中项可在「设置 → 流媒体」里修改。
      </p>

      <div className="flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
        >
          取消
        </button>
        <button
          type="button"
          onClick={() => onConfirm(mode)}
          className="h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper hover:bg-stamp hover:border-stamp"
        >
          导入
        </button>
      </div>
    </Dialog>
  );
}
