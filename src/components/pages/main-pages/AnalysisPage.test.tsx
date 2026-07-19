// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { AnalysisPage } from "./AnalysisPage";
import { useAnalysisSettingsStore } from "@/store/analysisSettings";
import { usePlayerStore } from "@/store/player";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    invoke: vi.fn(async () => null),
    isTauriRuntime: () => false, // 测试走纯浏览器（模拟器）路径
  };
});

beforeAll(() => {
  // jsdom 默认没有 rAF；渲染循环用 16ms 定时器代替
  vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) =>
    window.setTimeout(() => cb(performance.now()), 16)
  );
  vi.stubGlobal("cancelAnimationFrame", (id: number) => window.clearTimeout(id));
});

describe("AnalysisPage (v0.4.6+)", () => {
  beforeEach(() => {
    useAnalysisSettingsStore.getState().resetAnalysisSettings();
  });

  afterEach(() => {
    cleanup();
  });

  it("渲染六个仪表面板", () => {
    usePlayerStore.setState({ isPlaying: false });
    render(<AnalysisPage />);

    expect(screen.getByText("LOUDNESS · 响度")).toBeInTheDocument();
    expect(screen.getByText("LEVELS · 电平表")).toBeInTheDocument();
    expect(screen.getByText("SOUND FIELD · 声场")).toBeInTheDocument();
    expect(screen.getByText("SPECTRUM · 频谱")).toBeInTheDocument();
    expect(screen.getByText("SPECTROGRAM · 频谱瀑布")).toBeInTheDocument();
    expect(screen.getByText("OSCILLOSCOPE · 示波器")).toBeInTheDocument();
    // 档案编号
    expect(screen.getByText("NO.01")).toBeInTheDocument();
    expect(screen.getByText("NO.06")).toBeInTheDocument();
  });

  it("声场与瀑布的模式切换更新按压态", () => {
    render(<AnalysisPage />);

    const polar = screen.getByRole("button", { name: "POLAR 极坐标" });
    const lissajous = screen.getByRole("button", { name: "LISSAJOUS 李萨如" });
    expect(polar).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(lissajous);
    expect(lissajous).toHaveAttribute("aria-pressed", "true");
    expect(polar).toHaveAttribute("aria-pressed", "false");

    const heat = screen.getByRole("button", { name: "HEAT 热图" });
    fireEvent.click(heat);
    expect(heat).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/色带/)).toBeInTheDocument();
  });

  it("响度目标下拉可切换", () => {
    render(<AnalysisPage />);

    const select = screen.getByLabelText("响度目标") as HTMLSelectElement;
    expect(select.value).toBe("-14");
    fireEvent.change(select, { target: { value: "-23" } });
    expect(select.value).toBe("-23");
  });

  it("电平表可切换到 VU 表盘模式并写入设置", () => {
    render(<AnalysisPage />);

    const vuTab = screen.getByRole("button", { name: "VU 表盘" });
    fireEvent.click(vuTab);
    expect(vuTab).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("VU · 0VU = -18 dBFS")).toBeInTheDocument();
    expect(useAnalysisSettingsStore.getState().levelsMode).toBe("vu");
  });

  it("面板设置浮层可隐藏示波器并持久化到设置 store", () => {
    render(<AnalysisPage />);

    fireEvent.click(screen.getByRole("button", { name: /PANELS 面板设置/ }));
    const scopeToggle = screen.getByLabelText(/NO\.06 OSCILLOSCOPE/);
    fireEvent.click(scopeToggle);

    expect(useAnalysisSettingsStore.getState().panels.scope).toBe(false);
    expect(screen.queryByText("OSCILLOSCOPE · 示波器")).not.toBeInTheDocument();

    // 恢复默认后面板回归
    fireEvent.click(screen.getByRole("button", { name: "恢复默认设置" }));
    expect(screen.getByText("OSCILLOSCOPE · 示波器")).toBeInTheDocument();
  });

  it("全部面板隐藏时展示空态提示", () => {
    const settings = useAnalysisSettingsStore.getState();
    for (const id of [
      "loudness",
      "levels",
      "field",
      "spectrum",
      "scope",
      "spectrogram",
    ] as const) {
      settings.setPanelVisible(id, false);
    }
    render(<AnalysisPage />);
    expect(screen.getByText(/全部面板已隐藏/)).toBeInTheDocument();
  });
});
