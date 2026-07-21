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

    const polar = screen.getByRole("button", { name: "POLAR" });
    const lissajous = screen.getByRole("button", { name: "LISSAJOUS" });
    expect(polar).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(lissajous);
    expect(lissajous).toHaveAttribute("aria-pressed", "true");
    expect(polar).toHaveAttribute("aria-pressed", "false");

    const heat = screen.getByRole("button", { name: "HEAT" });
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

    const vuTab = screen.getByRole("button", { name: "VU" });
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

describe("AnalysisPage 布局编辑 (v0.4.9)", () => {
  beforeEach(() => {
    useAnalysisSettingsStore.getState().resetAnalysisSettings();
    // 宽屏（≥1536px）才开放布局编辑
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      configurable: true,
      value: (query: string) => ({
        matches: true,
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
      }),
    });
    // jsdom 无布局：编辑器网格按 1200×900 换算（格 100×75）
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      x: 0,
      y: 0,
      left: 0,
      top: 0,
      right: 1200,
      bottom: 900,
      width: 1200,
      height: 900,
      toJSON: () => ({}),
    } as DOMRect);
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    delete (window as { matchMedia?: unknown }).matchMedia;
  });

  it("宽屏点击 LAYOUT 进入图纸编辑模式", () => {
    render(<AnalysisPage />);
    fireEvent.click(screen.getByRole("button", { name: "LAYOUT 布局" }));
    expect(screen.getByTestId("analysis-layout-editor")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "完成 ✓" })).toBeInTheDocument();
    // 仪表面板被图框替换
    expect(screen.queryByText("LOUDNESS · 响度")).not.toBeInTheDocument();
  });

  it("拖拽交换位置后「完成」提交 custom 布局到 store", () => {
    render(<AnalysisPage />);
    fireEvent.click(screen.getByRole("button", { name: "LAYOUT 布局" }));

    // 示波器（y4，标题约在 310px）拖到顶部 → 与频谱交换
    const handle = screen.getByTestId("layout-handle-scope");
    fireEvent.pointerDown(handle, {
      pointerId: 1,
      button: 0,
      clientX: 400,
      clientY: 310,
    });
    fireEvent.pointerMove(handle, { pointerId: 1, clientX: 400, clientY: 20 });
    fireEvent.pointerUp(handle, { pointerId: 1 });

    fireEvent.click(screen.getByRole("button", { name: "完成 ✓" }));
    const state = useAnalysisSettingsStore.getState();
    expect(state.layoutMode).toBe("custom");
    expect(state.customLayout!.scope).toMatchObject({ x: 3, y: 0 });
    expect(state.customLayout!.spectrum).toMatchObject({ x: 3, y: 3 });
    // 提交后回到仪表视图
    expect(screen.getByText("OSCILLOSCOPE · 示波器")).toBeInTheDocument();
  });

  it("取消编辑不写入 store", () => {
    render(<AnalysisPage />);
    fireEvent.click(screen.getByRole("button", { name: "LAYOUT 布局" }));
    fireEvent.click(screen.getByRole("button", { name: "取消" }));
    const state = useAnalysisSettingsStore.getState();
    expect(state.layoutMode).toBe("auto");
    expect(state.customLayout).toBeNull();
    expect(screen.getByText("LOUDNESS · 响度")).toBeInTheDocument();
  });
});
