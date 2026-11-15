// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { SettingsModal } from "./SettingsModal";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  open: vi.fn<() => Promise<unknown>>(),
}));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  isTauriRuntime: () => true,
  invoke: (command: string, args?: Record<string, unknown>) => bridge.invoke(command, args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => bridge.open(),
  save: vi.fn(),
}));

const status = (overrides: Record<string, unknown> = {}) => ({
  settings: { cacheDir: "D:\\cache", maxSizeMb: 5120, autoCleanup: true },
  usedMb: 2048,
  usagePercent: 40,
  fileCount: 37,
  ...overrides,
});

async function openCacheTab() {
  render(<SettingsModal />);
  fireEvent.click(screen.getByRole("tab", { name: "缓存管理" }));
  await screen.findByText(/37 个文件/);
}

describe("设置页（issues-2）", () => {
  beforeEach(() => {
    bridge.invoke.mockReset();
    bridge.open.mockReset();
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "get_cache_status") return status();
      if (command === "clear_cache") {
        return { removedFiles: 37, removedBytes: 1024, removedPaths: [] };
      }
      return undefined;
    });
    usePlayerStore.setState({ ...usePlayerStore.getInitialState(), settingsOpen: true });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("UI-01：清理缓存先弹确认并写明文件数与占用，取消不调用后端", async () => {
    await openCacheTab();
    fireEvent.click(screen.getByRole("button", { name: "清理缓存" }));
    const dialog = screen.getByRole("dialog", { name: "清理全部缓存" });
    expect(dialog).toHaveTextContent("37 个已下载的流媒体音频文件");
    expect(dialog).toHaveTextContent("2.00 GB");
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));
    expect(bridge.invoke).not.toHaveBeenCalledWith("clear_cache", undefined);

    fireEvent.click(screen.getByRole("button", { name: "清理缓存" }));
    fireEvent.click(
      within(screen.getByRole("dialog", { name: "清理全部缓存" })).getByRole("button", {
        name: "确认清理",
      })
    );
    await waitFor(() => expect(bridge.invoke).toHaveBeenCalledWith("clear_cache", undefined));
  });

  it("UI-04：缓存表单有改动时 ✕ 不直接丢弃，放弃后才关闭", async () => {
    await openCacheTab();
    fireEvent.change(screen.getByDisplayValue("5120"), { target: { value: "1024" } });
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(usePlayerStore.getState().settingsOpen).toBe(true);
    const dialog = screen.getByRole("dialog", { name: "缓存设置尚未保存" });
    fireEvent.click(within(dialog).getByRole("button", { name: "放弃改动" }));
    expect(usePlayerStore.getState().settingsOpen).toBe(false);
    expect(bridge.invoke).not.toHaveBeenCalledWith("update_cache_settings", expect.anything());
  });

  it("UI-04：没有改动时 ✕ 直接关闭", async () => {
    await openCacheTab();
    fireEvent.click(screen.getByRole("button", { name: "关闭" }));
    expect(usePlayerStore.getState().settingsOpen).toBe(false);
  });

  it("CACHE-01：缓存盘不可用时显示原因", async () => {
    bridge.invoke.mockImplementation(async (command) =>
      command === "get_cache_status"
        ? status({ fileCount: 0, usedMb: 0, unavailableReason: "缓存目录 E:\\c 所在的磁盘当前不可用" })
        : undefined
    );
    render(<SettingsModal />);
    fireEvent.click(screen.getByRole("tab", { name: "缓存管理" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("所在的磁盘当前不可用");
  });

  it("UI-02：导入配置被后端以结构化错误拒绝时显示 message 而不是 [object Object]", async () => {
    bridge.open.mockResolvedValue("C:\\cfg.json");
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "get_cache_status") return status();
      if (command === "import_app_config") {
        throw { code: "invalid_input", message: "配置文件过大（4096 KB），上限 2 MB" };
      }
      return undefined;
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<SettingsModal />);
    fireEvent.click(screen.getByRole("tab", { name: "系统集成" }));
    fireEvent.click(screen.getByRole("button", { name: "导入配置…" }));
    await waitFor(() => {
      const notification = usePlayerStore.getState().notification;
      expect(notification?.text).toBe("导入配置失败：配置文件过大（4096 KB），上限 2 MB");
      expect(notification?.level).toBe("error");
    });
  });
});
