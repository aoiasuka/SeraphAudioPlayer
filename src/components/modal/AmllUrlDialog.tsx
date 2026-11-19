import { useEffect, useState, type FormEvent } from "react";
import { Link2, Loader2, Plug, RotateCcw } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { invoke, isTauriRuntime, normalizeIpcError } from "@/lib/tauri";
import {
  AMLL_LINKS,
  AMLL_TTML_DB_CUSTOM_PRESETS,
  AMLL_TTML_DB_PRESET_URL,
  DEFAULT_AMLL_TTML_DB_URL,
  isValidAmllTtmlDbUrl,
  previewAmllTtmlUrl,
} from "@/lib/lyrics/settings";
import { usePlayerStore } from "@/store/player";
import type { AmllTtmlDbTestKind, AmllTtmlDbTestResult } from "@/store/player/types";

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

/** 歌词设置里的「AMLL TTML DB 地址」子弹窗：预设 / 自定义地址、测试连接。 */
export function AmllUrlDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
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
      setTestError(normalizeIpcError(err).message);
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
