import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { AboutSettingsTab } from "@/components/modal/AboutSettingsTab";
import { AudioSettingsTab } from "@/components/modal/AudioSettingsTab";
import { CacheSettingsTab } from "@/components/modal/CacheSettingsTab";
import { LyricsSettingsTab } from "@/components/modal/LyricsSettingsTab";
import { StreamingSettingsTab } from "@/components/modal/StreamingSettingsTab";
import { SystemSettingsTab } from "@/components/modal/SystemSettingsTab";
import { useCacheSettingsForm } from "@/components/modal/useCacheSettingsForm";
import { Dialog } from "@/components/ui/dialog";
import { usePlayerStore } from "@/store/player";

type SettingsTab = "audio" | "lyrics" | "streaming" | "cache" | "system" | "about";

const SETTINGS_TABS: { value: SettingsTab; label: string }[] = [
  { value: "audio", label: "音频输出" },
  { value: "lyrics", label: "歌词设置" },
  { value: "streaming", label: "流媒体" },
  { value: "cache", label: "缓存管理" },
  { value: "system", label: "系统集成" },
  { value: "about", label: "关于与更新" },
];

/**
 * 设置弹窗的壳：标签栏、关闭流程，以及当前标签页的挂载。
 *
 * UI-05：各标签页是独立组件，各自订阅自己用到的 store 字段、持有自己的本地状态。
 * 壳只留跨标签页的东西——当前标签，以及缓存表单（设置页里唯一需要「保存」的表单：
 * 未保存的改动要在切换标签页后保留，关闭时要询问或自动保存，所以它的状态挂在这里）。
 */
export function SettingsModal() {
  const open = usePlayerStore((s) => s.settingsOpen);
  const toggleSettings = usePlayerStore((s) => s.toggleSettings);
  const [activeTab, setActiveTab] = useState<SettingsTab>("audio");
  const cache = useCacheSettingsForm(open);
  // UI-04：缓存表单有未保存改动时，✕ / Esc / 点遮罩不再直接丢弃。
  const [discardConfirmOpen, setDiscardConfirmOpen] = useState(false);

  // 歌词稿「打开歌词设置」等入口派发该事件直达指定标签页（v0.6.0）
  useEffect(() => {
    const onOpenTab = (event: Event) => {
      const detail = (event as CustomEvent<unknown>).detail;
      if (SETTINGS_TABS.some((tab) => tab.value === detail)) {
        setActiveTab(detail as SettingsTab);
      }
    };
    window.addEventListener("seraph:open-settings-tab", onOpenTab);
    return () => window.removeEventListener("seraph:open-settings-tab", onOpenTab);
  }, []);

  // M-18：驱动/设备/SMTC 等设置全部即时生效，弹窗从不存在“待保存”的输出
  // 配置——旧「保存配置/取消」双按钮语义均为假（保存只是关窗+假通知，取消
  // 不回滚）。收敛为单个「完成」：若缓存 tab 有未提交的表单改动则先自动提交。
  const finishAndClose = async () => {
    if (cache.dirty) {
      const saved = await cache.save();
      // 保存失败留在弹窗，让用户修正或明确放弃
      if (!saved) return;
    }
    toggleSettings();
  };

  const requestClose = () => {
    if (cache.dirty) {
      setDiscardConfirmOpen(true);
      return;
    }
    toggleSettings();
  };

  const discardAndClose = () => {
    setDiscardConfirmOpen(false);
    cache.discard();
    toggleSettings();
  };

  return (
    <Dialog
      open={open}
      onClose={requestClose}
      className="max-h-[88vh] max-w-2xl space-y-6 overflow-y-auto"
    >
      <button
        onClick={requestClose}
        className="absolute top-4 right-4 text-ink3 hover:text-stamp"
        aria-label="关闭"
      >
        <X className="w-4 h-4" />
      </button>

      <span className="file-tab">FILE — SYSTEM / SETTINGS</span>

      {/* 功能分页：音频输出 / 歌词设置 / 流媒体 / 缓存管理 / 系统集成 / 关于与更新 */}
      <div role="tablist" aria-label="设置分类" className="flex flex-wrap gap-1.5 border-b-[1.5px] border-line pb-3">
        {SETTINGS_TABS.map((tab) => (
          <button
            key={tab.value}
            type="button"
            role="tab"
            aria-selected={activeTab === tab.value}
            onClick={() => setActiveTab(tab.value)}
            className={
              activeTab === tab.value
                ? "h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper"
                : "h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 transition-colors hover:border-ink"
            }
          >
            {tab.label}
          </button>
        ))}
      </div>

      {activeTab === "audio" && <AudioSettingsTab />}
      {activeTab === "lyrics" && <LyricsSettingsTab />}
      {activeTab === "streaming" && <StreamingSettingsTab />}
      {activeTab === "cache" && <CacheSettingsTab form={cache} />}
      {activeTab === "system" && <SystemSettingsTab />}
      {activeTab === "about" && <AboutSettingsTab />}

      <div className="flex justify-end gap-3 pt-3 border-t border-line">
        <button
          onClick={() => void finishAndClose()}
          className="border-[1.5px] border-ink bg-ink px-4 py-1.5 font-tw text-xs font-bold text-paper hover:bg-stamp hover:border-stamp transition-all"
        >
          {cache.dirty ? "保存并关闭" : "完成"}
        </button>
      </div>

      <Dialog open={discardConfirmOpen} onClose={() => setDiscardConfirmOpen(false)} className="max-w-sm space-y-4">
        <h3 className="font-serif text-sm font-semibold text-ink">缓存设置尚未保存</h3>
        <p className="font-tw text-xs leading-5 text-ink2">
          缓存目录或容量上限有改动还没有保存。要保存后再关闭吗？
        </p>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={discardAndClose}
            className="h-8 border-[1.5px] border-line bg-card px-3 font-tw text-xs font-bold text-ink2 hover:border-ink"
          >
            放弃改动
          </button>
          <button
            type="button"
            onClick={() => {
              setDiscardConfirmOpen(false);
              void finishAndClose();
            }}
            className="h-8 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper"
          >
            保存并关闭
          </button>
        </div>
      </Dialog>
    </Dialog>
  );
}
