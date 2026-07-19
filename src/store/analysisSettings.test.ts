// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_ANALYSIS_SETTINGS,
  migrateAnalysisSettings,
  useAnalysisSettingsStore,
} from "./analysisSettings";

describe("analysis settings store (v0.4.8)", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useAnalysisSettingsStore.getState().resetAnalysisSettings();
  });

  it("defaults to all panels visible with bar levels and polar field", () => {
    const state = useAnalysisSettingsStore.getState();
    expect(state.panels).toEqual(DEFAULT_ANALYSIS_SETTINGS.panels);
    expect(state.levelsMode).toBe("bar");
    expect(state.fieldMode).toBe("polar");
    expect(state.scopeTrigger).toBe(true);
  });

  it("toggles panel visibility and content switches independently", () => {
    const state = useAnalysisSettingsStore.getState();
    state.setPanelVisible("scope", false);
    state.setLevelsMode("vu");
    state.setLevelsShowPeak(false);
    state.setSpectrumShowPeakHold(false);
    const next = useAnalysisSettingsStore.getState();
    expect(next.panels.scope).toBe(false);
    expect(next.panels.spectrum).toBe(true);
    expect(next.levelsMode).toBe("vu");
    expect(next.levelsShowPeak).toBe(false);
    expect(next.spectrumShowPeakHold).toBe(false);
  });

  it("persists changes into localStorage for cross-restart memory", async () => {
    useAnalysisSettingsStore.getState().setLevelsMode("vu");
    useAnalysisSettingsStore.getState().setPanelVisible("field", false);
    // zustand persist 同步写；读回校验
    const raw = window.localStorage.getItem("seraph-analysis-settings");
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!) as {
      state: { levelsMode: string; panels: Record<string, boolean> };
    };
    expect(parsed.state.levelsMode).toBe("vu");
    expect(parsed.state.panels.field).toBe(false);
  });

  it("resetAnalysisSettings restores defaults", () => {
    const state = useAnalysisSettingsStore.getState();
    state.setPanelVisible("loudness", false);
    state.setScopeSplit(true);
    state.resetAnalysisSettings();
    const next = useAnalysisSettingsStore.getState();
    expect(next.panels.loudness).toBe(true);
    expect(next.scopeSplit).toBe(false);
  });

  it("migrate falls back to defaults for corrupt fields", () => {
    const migrated = migrateAnalysisSettings({
      panels: { loudness: "yes", scope: false },
      loudnessTarget: 999,
      levelsMode: "nonsense",
      fieldMode: "lissajous",
      scopeTrigger: 0,
    });
    expect(migrated.panels.loudness).toBe(true); // 坏值回默认
    expect(migrated.panels.scope).toBe(false); // 合法值保留
    expect(migrated.loudnessTarget).toBe(-14);
    expect(migrated.levelsMode).toBe("bar");
    expect(migrated.fieldMode).toBe("lissajous");
    expect(migrated.scopeTrigger).toBe(true);
  });
});
