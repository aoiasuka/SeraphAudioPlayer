import { create } from "zustand";
import { persist } from "zustand/middleware";
import { invoke, ipcErrorReason } from "@/lib/tauri";
import { DEFAULT_EQ_BANDS } from "@/lib/eqPresets";
import {
  MAX_EQ_BANDS,
  sanitizeCrossfeed,
  sanitizeEqBandPatch,
  sanitizeEqBands,
  sanitizeEqPreamp,
  sanitizeUserPresets,
} from "@/lib/eqSanitize";
import { usePlayerStore } from "@/store/player";
import type {
  CrossfeedSettings,
  DspSettings,
  EqBand,
  EqPreset,
} from "@/types/dsp";

const EQ_PERSIST_VERSION = 1;

const DEFAULT_CROSSFEED: CrossfeedSettings = {
  enabled: false,
  amount: 0.3,
  cutoffHz: 700,
};

interface EqState {
  enabled: boolean;
  preamp: number;
  bands: EqBand[];
  crossfeed: CrossfeedSettings;
  applyToDsd: boolean;
  userPresets: EqPreset[];
  /** 当前选中的预设 id（内置或用户），自定义改动后置 null */
  activePresetId: string | null;

  setEnabled: (enabled: boolean) => void;
  setApplyToDsd: (value: boolean) => void;
  setPreamp: (db: number) => void;
  setBandGain: (index: number, gain: number) => void;
  updateBand: (index: number, patch: Partial<EqBand>) => void;
  addBand: () => void;
  removeBand: (index: number) => void;
  setBands: (bands: EqBand[], preamp: number, presetId?: string | null) => void;
  resetBands: () => void;
  setCrossfeed: (patch: Partial<CrossfeedSettings>) => void;
  saveUserPreset: (name: string) => void;
  deleteUserPreset: (id: string) => void;
  applyPreset: (bands: EqBand[], preamp: number, presetId: string | null) => void;
  syncToEngine: () => void;
}

export function buildDspSettings(state: {
  enabled: boolean;
  preamp: number;
  bands: EqBand[];
  crossfeed: CrossfeedSettings;
  applyToDsd: boolean;
}): DspSettings {
  return {
    enabled: state.enabled,
    preamp: state.preamp,
    bands: state.bands,
    crossfeed: state.crossfeed,
    applyToDsd: state.applyToDsd,
  };
}

// 下发去抖：拖动 slider 时高频变化，合并到 ~60ms 一次 IPC。
let pushTimer: ReturnType<typeof setTimeout> | null = null;
/** 上一次已经提示过的失败原因；下发成功后清空（FE3-03）。 */
let lastPushFailure: string | null = null;

function pushToEngine(settings: DspSettings) {
  if (pushTimer) clearTimeout(pushTimer);
  pushTimer = setTimeout(() => {
    pushTimer = null;
    void invoke("set_dsp_settings", { settings })
      .then(() => {
        lastPushFailure = null;
      })
      .catch((err) => {
        const reason = ipcErrorReason(err) || "未知错误";
        console.warn("set_dsp_settings failed", reason);
        // FE3-03：此前只写控制台——后端拒绝一份设置时，界面上的曲线与实际出声的不一致，
        // 用户无从知道。同一个原因只提示一次：拖动滑块时每 60 ms 下发一次，不能刷屏；
        // 下发成功过之后再失败会重新提示。
        if (reason === lastPushFailure) return;
        lastPushFailure = reason;
        usePlayerStore
          .getState()
          .showNotification(`均衡器设置未生效，实际音效可能与界面不一致：${reason}`, "error");
      });
  }, 60);
}

// EQ4-03：手动添加、APO 解析、消毒、后端兜底共用同一个上限
const MAX_BANDS = MAX_EQ_BANDS;

export const useEqStore = create<EqState>()(
  persist(
    (set, get) => {
      const commit = () => {
        const state = get();
        pushToEngine(buildDspSettings(state));
      };

      return {
        enabled: false,
        preamp: 0,
        bands: DEFAULT_EQ_BANDS,
        crossfeed: DEFAULT_CROSSFEED,
        applyToDsd: false,
        userPresets: [],
        activePresetId: "flat",

        setEnabled: (enabled) => {
          set({ enabled });
          commit();
        },

        setApplyToDsd: (value) => {
          set({ applyToDsd: value });
          commit();
        },

        setPreamp: (db) => {
          set({ preamp: clamp(db, -24, 24), activePresetId: null });
          commit();
        },

        setBandGain: (index, gain) => {
          set((state) => ({
            bands: state.bands.map((band, i) =>
              i === index ? { ...band, gain: clamp(gain, -24, 24) } : band
            ),
            activePresetId: null,
          }));
          commit();
        },

        updateBand: (index, patch) => {
          // H-1：高级编辑输入框清空时 parseFloat("") 会送来 NaN，
          // 消毒后无效字段等价于“本次不修改”，绝不让坏值进入持久化。
          const clean = sanitizeEqBandPatch(patch);
          if (Object.keys(clean).length === 0) return;
          set((state) => ({
            bands: state.bands.map((band, i) =>
              i === index ? { ...band, ...clean } : band
            ),
            activePresetId: null,
          }));
          commit();
        },

        addBand: () => {
          set((state) => {
            if (state.bands.length >= MAX_BANDS) return state;
            const newBand: EqBand = {
              kind: "peaking",
              freq: 1000,
              gain: 0,
              q: 1.0,
              enabled: true,
            };
            return { bands: [...state.bands, newBand], activePresetId: null };
          });
          commit();
        },

        removeBand: (index) => {
          set((state) => ({
            bands: state.bands.filter((_, i) => i !== index),
            activePresetId: null,
          }));
          commit();
        },

        setBands: (bands, preamp, presetId = null) => {
          // H-1：导入路径（JSON/APO/预设）统一消毒——无效频段丢弃、数值钳制，
          // 全部无效则回落默认，杜绝 NaN → 持久化 → 重启白屏死循环。
          set({
            bands: sanitizeEqBands(bands) ?? DEFAULT_EQ_BANDS,
            preamp: sanitizeEqPreamp(preamp),
            activePresetId: presetId,
          });
          commit();
        },

        resetBands: () => {
          set({ bands: DEFAULT_EQ_BANDS, preamp: 0, activePresetId: "flat" });
          commit();
        },

        setCrossfeed: (patch) => {
          set((state) => ({ crossfeed: { ...state.crossfeed, ...patch } }));
          commit();
        },

        applyPreset: (bands, preamp, presetId) => {
          set({
            bands: sanitizeEqBands(bands) ?? DEFAULT_EQ_BANDS,
            preamp: sanitizeEqPreamp(preamp),
            activePresetId: presetId,
          });
          commit();
        },

        saveUserPreset: (name) => {
          const trimmed = name.trim();
          if (!trimmed) return;
          const createdAt = Date.now();
          const preset: EqPreset = {
            id: `eq-${createdAt}-${Math.random().toString(36).slice(2, 8)}`,
            name: trimmed,
            preamp: get().preamp,
            bands: get().bands,
            createdAt,
          };
          set((state) => ({
            userPresets: [...state.userPresets, preset],
            activePresetId: preset.id,
          }));
        },

        deleteUserPreset: (id) => {
          set((state) => ({
            userPresets: state.userPresets.filter((preset) => preset.id !== id),
            activePresetId:
              state.activePresetId === id ? null : state.activePresetId,
          }));
        },

        syncToEngine: () => {
          pushToEngine(buildDspSettings(get()));
        },
      };
    },
    {
      name: "seraph-eq-state",
      version: EQ_PERSIST_VERSION,
      skipHydration: true,
      partialize: (state) => ({
        enabled: state.enabled,
        preamp: state.preamp,
        bands: state.bands,
        crossfeed: state.crossfeed,
        applyToDsd: state.applyToDsd,
        userPresets: state.userPresets,
        activePresetId: state.activePresetId,
      }),
      // H-1 根治层：无论坏数据从哪条路径进入 localStorage（配置备份整包
      // 写入、历史版本残留、外部手改），水合时全部消毒——坏 preamp/band
      // 曾在重启后让 toFixed 抛 TypeError 卸载整棵 React 树（启动白屏）。
      merge: (persisted, current) => {
        const raw = (
          persisted && typeof persisted === "object" ? persisted : {}
        ) as Record<string, unknown>;
        return {
          ...current,
          enabled: raw.enabled === true,
          preamp: sanitizeEqPreamp(raw.preamp),
          bands: sanitizeEqBands(raw.bands) ?? DEFAULT_EQ_BANDS,
          crossfeed: sanitizeCrossfeed(raw.crossfeed, DEFAULT_CROSSFEED),
          applyToDsd: raw.applyToDsd === true,
          userPresets: sanitizeUserPresets(raw.userPresets),
          activePresetId:
            typeof raw.activePresetId === "string" ? raw.activePresetId : null,
        };
      },
      onRehydrateStorage: () => (state) => {
        // 水合完成后把持久化的 DSP 配置同步到引擎，重启保持一致。
        if (state) {
          // 引擎初始为默认（禁用）；有配置则下发。延迟到下一 tick，确保 store 就绪。
          setTimeout(() => {
            usePlayerStore.getState(); // 确保 player store 也已初始化
            state.syncToEngine();
          }, 0);
        }
      },
    }
  )
);

function clamp(value: number, min: number, max: number) {
  if (!Number.isFinite(value)) return 0;
  return Math.max(min, Math.min(max, value));
}
