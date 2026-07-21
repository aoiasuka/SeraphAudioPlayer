import { create } from "zustand";
import { persist } from "zustand/middleware";
import type { SoundFieldMode, SpectrogramMode } from "@/lib/analysis/render";
import { sanitizeGridLayout, type GridLayout } from "@/lib/analysis/gridLayout";

/** 声学分析六仪表面板 id（NO.01–NO.06） */
export type AnalysisPanelId =
  | "loudness"
  | "levels"
  | "field"
  | "spectrum"
  | "scope"
  | "spectrogram";

export type LevelsDisplayMode = "bar" | "vu";

/** auto = 智能补位布局（v0.4.7 语义）；custom = 用户在编辑模式拖拽定制的 12×12 网格 */
export type AnalysisLayoutMode = "auto" | "custom";

export const ANALYSIS_PANEL_IDS: AnalysisPanelId[] = [
  "loudness",
  "levels",
  "field",
  "spectrum",
  "scope",
  "spectrogram",
];

interface AnalysisSettingsData {
  /** 面板可见性（模块级开关） */
  panels: Record<AnalysisPanelId, boolean>;
  /** 响度目标制（-14 流媒体 / -16 播客 / -23 EBU / -9 母带） */
  loudnessTarget: number;
  /** 响度目标偏差标尺行 */
  loudnessShowDeviation: boolean;
  /** 电平表显示模式：分行条表 / 模拟 VU 表 */
  levelsMode: LevelsDisplayMode;
  /** 条表模式下的 PEAK / RMS 行开关 */
  levelsShowPeak: boolean;
  levelsShowRms: boolean;
  fieldMode: SoundFieldMode;
  /** 声场面板底部的相关度表 */
  fieldShowCorrelation: boolean;
  /** 频谱峰值保持虚线 */
  spectrumShowPeakHold: boolean;
  spectrogramMode: SpectrogramMode;
  /** 示波器 L/R 分离显示（关闭 = 叠加） */
  scopeSplit: boolean;
  /** 示波器零交叉触发对齐（稳定周期波形） */
  scopeTrigger: boolean;
  /** 宽屏布局模式：自动补位 / 用户自定义网格 */
  layoutMode: AnalysisLayoutMode;
  /** 自定义 12×12 网格布局（六面板 rect；隐藏面板的位置也保留） */
  customLayout: GridLayout | null;
}

interface AnalysisSettingsState extends AnalysisSettingsData {
  setPanelVisible: (id: AnalysisPanelId, visible: boolean) => void;
  setLoudnessTarget: (target: number) => void;
  setLoudnessShowDeviation: (value: boolean) => void;
  setLevelsMode: (mode: LevelsDisplayMode) => void;
  setLevelsShowPeak: (value: boolean) => void;
  setLevelsShowRms: (value: boolean) => void;
  setFieldMode: (mode: SoundFieldMode) => void;
  setFieldShowCorrelation: (value: boolean) => void;
  setSpectrumShowPeakHold: (value: boolean) => void;
  setSpectrogramMode: (mode: SpectrogramMode) => void;
  setScopeSplit: (value: boolean) => void;
  setScopeTrigger: (value: boolean) => void;
  /** 提交编辑模式的布局（同时切到 custom 模式） */
  setCustomLayout: (layout: GridLayout) => void;
  /** 回到自动布局（保留 customLayout 以便再次切回） */
  setLayoutMode: (mode: AnalysisLayoutMode) => void;
  resetAnalysisSettings: () => void;
}

const ANALYSIS_SETTINGS_VERSION = 2;

export const DEFAULT_ANALYSIS_SETTINGS: AnalysisSettingsData = {
  panels: {
    loudness: true,
    levels: true,
    field: true,
    spectrum: true,
    scope: true,
    spectrogram: true,
  },
  loudnessTarget: -14,
  loudnessShowDeviation: true,
  levelsMode: "bar",
  levelsShowPeak: true,
  levelsShowRms: true,
  fieldMode: "polar",
  fieldShowCorrelation: true,
  spectrumShowPeakHold: true,
  spectrogramMode: "ridge",
  scopeSplit: false,
  scopeTrigger: true,
  layoutMode: "auto",
  customLayout: null,
};

function asBool(value: unknown, fallback: boolean) {
  return typeof value === "boolean" ? value : fallback;
}

/** 宽松迁移：逐字段类型校验，坏值回落默认（导入的配置也走这里兜底） */
export function migrateAnalysisSettings(persisted: unknown): AnalysisSettingsData {
  const state =
    persisted && typeof persisted === "object"
      ? (persisted as Record<string, unknown>)
      : {};
  const defaults = DEFAULT_ANALYSIS_SETTINGS;
  const rawPanels =
    state.panels && typeof state.panels === "object"
      ? (state.panels as Record<string, unknown>)
      : {};
  const panels = Object.fromEntries(
    ANALYSIS_PANEL_IDS.map((id) => [id, asBool(rawPanels[id], true)])
  ) as Record<AnalysisPanelId, boolean>;
  const target = state.loudnessTarget;
  // 自定义布局：结构/数值非法整体判废并回 auto（resolveLayout 对轻微重叠自愈）
  const customLayout = sanitizeGridLayout(state.customLayout);
  const layoutMode: AnalysisLayoutMode =
    state.layoutMode === "custom" && customLayout !== null ? "custom" : "auto";
  return {
    panels,
    loudnessTarget:
      typeof target === "number" && Number.isFinite(target) && target >= -36 && target <= 0
        ? target
        : defaults.loudnessTarget,
    loudnessShowDeviation: asBool(state.loudnessShowDeviation, true),
    levelsMode: state.levelsMode === "vu" ? "vu" : "bar",
    levelsShowPeak: asBool(state.levelsShowPeak, true),
    levelsShowRms: asBool(state.levelsShowRms, true),
    fieldMode: state.fieldMode === "lissajous" ? "lissajous" : "polar",
    fieldShowCorrelation: asBool(state.fieldShowCorrelation, true),
    spectrumShowPeakHold: asBool(state.spectrumShowPeakHold, true),
    spectrogramMode: state.spectrogramMode === "heat" ? "heat" : "ridge",
    scopeSplit: asBool(state.scopeSplit, false),
    scopeTrigger: asBool(state.scopeTrigger, true),
    layoutMode,
    customLayout,
  };
}

export const useAnalysisSettingsStore = create<AnalysisSettingsState>()(
  persist(
    (set) => ({
      ...DEFAULT_ANALYSIS_SETTINGS,

      setPanelVisible: (id, visible) =>
        set((state) => ({ panels: { ...state.panels, [id]: visible } })),
      setLoudnessTarget: (loudnessTarget) => set({ loudnessTarget }),
      setLoudnessShowDeviation: (loudnessShowDeviation) =>
        set({ loudnessShowDeviation }),
      setLevelsMode: (levelsMode) => set({ levelsMode }),
      setLevelsShowPeak: (levelsShowPeak) => set({ levelsShowPeak }),
      setLevelsShowRms: (levelsShowRms) => set({ levelsShowRms }),
      setFieldMode: (fieldMode) => set({ fieldMode }),
      setFieldShowCorrelation: (fieldShowCorrelation) =>
        set({ fieldShowCorrelation }),
      setSpectrumShowPeakHold: (spectrumShowPeakHold) =>
        set({ spectrumShowPeakHold }),
      setSpectrogramMode: (spectrogramMode) => set({ spectrogramMode }),
      setScopeSplit: (scopeSplit) => set({ scopeSplit }),
      setScopeTrigger: (scopeTrigger) => set({ scopeTrigger }),
      setCustomLayout: (customLayout) =>
        set({ customLayout, layoutMode: "custom" }),
      setLayoutMode: (layoutMode) => set({ layoutMode }),
      resetAnalysisSettings: () =>
        set({
          ...DEFAULT_ANALYSIS_SETTINGS,
          panels: { ...DEFAULT_ANALYSIS_SETTINGS.panels },
        }),
    }),
    {
      name: "seraph-analysis-settings",
      version: ANALYSIS_SETTINGS_VERSION,
      migrate: migrateAnalysisSettings,
      partialize: (state): AnalysisSettingsData => ({
        panels: state.panels,
        loudnessTarget: state.loudnessTarget,
        loudnessShowDeviation: state.loudnessShowDeviation,
        levelsMode: state.levelsMode,
        levelsShowPeak: state.levelsShowPeak,
        levelsShowRms: state.levelsShowRms,
        fieldMode: state.fieldMode,
        fieldShowCorrelation: state.fieldShowCorrelation,
        spectrumShowPeakHold: state.spectrumShowPeakHold,
        spectrogramMode: state.spectrogramMode,
        scopeSplit: state.scopeSplit,
        scopeTrigger: state.scopeTrigger,
        layoutMode: state.layoutMode,
        customLayout: state.customLayout,
      }),
    }
  )
);
