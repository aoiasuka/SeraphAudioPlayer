// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EqPage } from "@/components/pages/main-pages/EqPage";
import { DEFAULT_EQ_BANDS } from "@/lib/eqPresets";
import { useEqStore } from "@/store/eq";

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  isTauriRuntime: () => false,
  invoke: vi.fn().mockResolvedValue(undefined),
}));

describe("EQ 页削波提示（DSP-04）", () => {
  beforeEach(() => {
    useEqStore.setState({ preamp: 0, bands: DEFAULT_EQ_BANDS, activePresetId: "flat" });
  });
  afterEach(() => cleanup());

  it("平直 EQ 不显示提示", () => {
    render(<EqPage />);
    expect(screen.queryByTestId("eq-clip-warning")).toBeNull();
  });

  it("频段提升而预放大留在 0 dB 时提示，点按钮后应用建议值并消失", () => {
    useEqStore.setState({
      bands: [{ kind: "peaking", freq: 1000, gain: 6, q: 1, enabled: true }],
      activePresetId: null,
    });
    render(<EqPage />);

    const warning = screen.getByTestId("eq-clip-warning");
    expect(warning.textContent).toContain("6.0 dB");
    fireEvent.click(screen.getByRole("button", { name: "预放大设为 -6.0 dB" }));

    expect(useEqStore.getState().preamp).toBe(-6);
    expect(screen.queryByTestId("eq-clip-warning")).toBeNull();
  });
});
