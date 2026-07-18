import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { invoke } from "@/lib/tauri";
import { buildDspSettings, useEqStore } from "@/store/eq";
import { GENRE_EQ_PRESETS, DEFAULT_EQ_BANDS } from "@/lib/eqPresets";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    invoke: vi.fn(async () => undefined) as unknown as typeof actual.invoke,
  };
});

const invokeMock = invoke as unknown as Mock;

async function flush() {
  for (let i = 0; i < 4; i += 1) {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

describe("eq store", () => {
  beforeEach(() => {
    invokeMock.mockClear();
    useEqStore.setState({
      enabled: false,
      preamp: 0,
      bands: DEFAULT_EQ_BANDS,
      crossfeed: { enabled: false, amount: 0.3, cutoffHz: 700 },
      applyToDsd: false,
      userPresets: [],
      activePresetId: "flat",
    });
  });

  it("buildDspSettings 组装出后端 camelCase 结构", () => {
    const settings = buildDspSettings({
      enabled: true,
      preamp: -3,
      bands: [],
      crossfeed: { enabled: true, amount: 0.4, cutoffHz: 800 },
      applyToDsd: true,
    });
    expect(settings).toEqual({
      enabled: true,
      preamp: -3,
      bands: [],
      crossfeed: { enabled: true, amount: 0.4, cutoffHz: 800 },
      applyToDsd: true,
    });
  });

  it("setEnabled 下发 set_dsp_settings 到引擎", async () => {
    useEqStore.getState().setEnabled(true);
    await flush();
    expect(invokeMock).toHaveBeenCalledWith(
      "set_dsp_settings",
      expect.objectContaining({
        settings: expect.objectContaining({ enabled: true }),
      })
    );
  });

  it("改动频段增益后 activePresetId 置 null（进入自定义）", () => {
    useEqStore.getState().setBandGain(0, 6);
    const state = useEqStore.getState();
    expect(state.bands[0].gain).toBe(6);
    expect(state.activePresetId).toBe(null);
  });

  it("增益超范围被钳制到 ±24dB", () => {
    useEqStore.getState().setBandGain(0, 999);
    expect(useEqStore.getState().bands[0].gain).toBe(24);
    useEqStore.getState().setPreamp(-999);
    expect(useEqStore.getState().preamp).toBe(-24);
  });

  it("applyPreset 套用曲风预设并记录 preset id", () => {
    const rock = GENRE_EQ_PRESETS.find((preset) => preset.id === "rock")!;
    useEqStore.getState().applyPreset(rock.bands, rock.preamp, rock.id);
    const state = useEqStore.getState();
    expect(state.activePresetId).toBe("rock");
    expect(state.bands).toHaveLength(10);
    expect(state.preamp).toBe(rock.preamp);
  });

  it("addBand / removeBand 调整频段数", () => {
    const initial = useEqStore.getState().bands.length;
    useEqStore.getState().addBand();
    expect(useEqStore.getState().bands).toHaveLength(initial + 1);
    useEqStore.getState().removeBand(0);
    expect(useEqStore.getState().bands).toHaveLength(initial);
  });

  it("saveUserPreset 保存当前配置为用户预设", () => {
    useEqStore.getState().setBandGain(2, 4);
    useEqStore.getState().saveUserPreset("我的预设");
    const presets = useEqStore.getState().userPresets;
    expect(presets).toHaveLength(1);
    expect(presets[0].name).toBe("我的预设");
    expect(presets[0].bands[2].gain).toBe(4);
  });

  it("deleteUserPreset 移除用户预设", () => {
    useEqStore.getState().saveUserPreset("临时");
    const id = useEqStore.getState().userPresets[0].id;
    useEqStore.getState().deleteUserPreset(id);
    expect(useEqStore.getState().userPresets).toHaveLength(0);
  });

  it("resetBands 回到平直并标记 flat 预设", () => {
    useEqStore.getState().setBandGain(0, 10);
    useEqStore.getState().resetBands();
    const state = useEqStore.getState();
    expect(state.preamp).toBe(0);
    expect(state.activePresetId).toBe("flat");
    expect(state.bands.every((band) => band.gain === 0)).toBe(true);
  });
});
