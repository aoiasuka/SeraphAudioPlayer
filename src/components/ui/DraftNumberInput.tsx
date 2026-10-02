import { useState, type InputHTMLAttributes } from "react";

type DraftNumberInputProps = Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "value" | "onChange" | "type" | "defaultValue"
> & {
  value: number;
  /** 草稿确认（失焦 / 回车）且能解析为有限数时调用一次；钳制交给调用方。 */
  onCommit: (value: number) => void;
  /** 非编辑态的显示格式，缺省 `String(value)`。 */
  format?: (value: number) => string;
};

/**
 * 草稿式数字输入框（BUG-04）：受控 `type="number"` + 每键钳制会让「0.707」在第一个
 * 「0」就被钳到下限、清空立即回弹、负号开头被丢弃。这里编辑期间只保存文本草稿，
 * 失焦或回车才解析提交；Esc 放弃草稿。用 `inputMode="decimal"` 的文本框而不是
 * `type="number"`——后者在输入「-」「0.」这类中间态时 value 读出来是空串。
 */
export function DraftNumberInput({
  value,
  onCommit,
  format = (current) => String(current),
  onBlur,
  onKeyDown,
  ...rest
}: DraftNumberInputProps) {
  const [draft, setDraft] = useState<string | null>(null);

  const commit = () => {
    if (draft === null) return;
    const text = draft.trim();
    const parsed = text === "" ? Number.NaN : Number(text);
    setDraft(null);
    if (Number.isFinite(parsed)) onCommit(parsed);
  };

  return (
    <input
      {...rest}
      type="text"
      inputMode="decimal"
      value={draft ?? format(value)}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={(event) => {
        commit();
        onBlur?.(event);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          commit();
        } else if (event.key === "Escape") {
          // 只放弃草稿；不在编辑态时让事件照常冒泡（弹窗等依赖 Esc 关闭）
          if (draft !== null) {
            event.stopPropagation();
            setDraft(null);
          }
        }
        onKeyDown?.(event);
      }}
    />
  );
}
