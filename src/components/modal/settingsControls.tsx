import type { ReactNode } from "react";

/** 设置页的一行：左边标题与说明，右边放控件。各标签页共用。 */
export function SettingRow({
  title,
  description,
  children,
}: {
  title: ReactNode;
  description: ReactNode;
  children: ReactNode;
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

/**
 * 「已启用 / 已停用」样式的开关按钮。按钮上只显示状态，`label` 是给读屏的名称——
 * 同一页有多个开关时，靠它区分各自控制的是什么。
 */
export function ToggleButton({
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
