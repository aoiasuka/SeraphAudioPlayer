import { ListMusic } from "lucide-react";
import { usePlayerStore } from "@/store/player";
import type { BilibiliMultiPageMode } from "@/store/player";
import { SettingRow } from "./settingsControls";

const MULTI_PAGE_OPTIONS: { value: BilibiliMultiPageMode; label: string }[] = [
  { value: "first", label: "仅导入第 1 P" },
  { value: "all", label: "导入全部 P" },
];

/**
 * 设置弹窗里的「流媒体」标签页：B 站导入的默认行为（即时生效）。
 * 音质偏好（杜比 / FLAC / 混流）在流媒体页的工具条上，不在这里重复。
 */
export function StreamingSettingsTab() {
  const multiPageMode = usePlayerStore((s) => s.bilibiliMultiPageMode);
  const setMultiPageMode = usePlayerStore((s) => s.setBilibiliMultiPageMode);

  return (
    <div className="space-y-4">
      <div className="space-y-2">
        <h3 className="font-serif text-base font-bold text-ink flex items-center gap-2">
          <ListMusic className="w-4 h-4 text-brown" />
          流媒体
        </h3>
        <p className="font-tw text-[11px] text-ink2 leading-relaxed">
          哔哩哔哩音频导入的默认行为。
        </p>
      </div>
      <SettingRow
        title="多 P 视频 · 无特定分 P 时"
        description={
          <>
            一个视频下有多个分 P、而链接没有指定分 P（不带 <code>?p=</code>）时的默认做法。
            粘贴单个链接会先询问，这里决定询问时默认选中哪一项；收藏夹批量导入不询问，
            直接按这里的设置。带 <code>?p=</code> 的链接始终只导入指定的那一 P。
          </>
        }
      >
        <div
          role="radiogroup"
          aria-label="无特定分 P 时的默认导入范围"
          className="flex shrink-0"
        >
          {MULTI_PAGE_OPTIONS.map((option, index) => {
            const checked = multiPageMode === option.value;
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={checked}
                onClick={() => setMultiPageMode(option.value)}
                className={
                  (checked
                    ? "border-ink bg-ink text-paper"
                    : "border-line bg-card text-ink2 hover:border-ink") +
                  " h-8 border-[1.5px] px-3 font-tw text-xs font-bold transition-colors" +
                  (index > 0 ? " -ml-[1.5px]" : "")
                }
              >
                {option.label}
              </button>
            );
          })}
        </div>
      </SettingRow>
    </div>
  );
}
