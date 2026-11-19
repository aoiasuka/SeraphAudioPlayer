import { useEffect, useState, type FormEvent } from "react";
import { Plus, Trash2 } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { MAX_EXCLUDE_PATTERN_CHARS, MAX_EXCLUDE_RULES } from "@/lib/lyrics/exclude";
import { invoke, isTauriRuntime, normalizeIpcError } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import type { LyricsExcludeRule } from "@/types/track";

/** 「制作信息预设」一键加入的正则（Rust regex 语法），默认不启用，只是按钮。 */
export const CREDITS_EXCLUDE_PRESET_PATTERN =
  // LIB5-03：含繁体、日文与常见英文写法；与 Rust 侧 `is_credit_line` 保持同一模式
  "^(作[词詞曲]|[编編]曲|[制製]作人?|混音|母[带帶]|[监監][制製]|出品|[录錄]音|和[声聲]|吉他|[贝貝]斯|鼓|[键鍵][盘盤]|弦[乐樂]|[发發]行|OP|SP|[词詞]|曲|(lyrics|words|music|composed|arranged|produced|mixed|mastered|written)( by)?)\\s*[:：]";

/** 命中预览最多列出的行数 */
const HIDDEN_PREVIEW_LIMIT = 30;

/** 歌词设置里的「歌词排除配置」子弹窗：规则增删、制作信息预设、当前曲目命中预览。 */
export function ExcludeRulesDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
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
        if (!disposed) setRegexError(normalizeIpcError(err).message);
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
  const hiddenLines = (currentTrack?.lyrics?.lines ?? []).filter((line) => line.hidden === true);
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
                key={`${line.startMs}-${index}`}
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
