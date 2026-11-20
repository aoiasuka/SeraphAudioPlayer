// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

  it("UI-05：缓存表单的未保存改动在切换标签页后保留，从别的标签页也能「保存并关闭」", async () => {
    bridge.invoke.mockImplementation(async (command, args) => {
      if (command === "get_cache_status") return status();
      if (command === "update_cache_settings") {
        const settings = (args as { settings: Record<string, unknown> }).settings;
        return status({ settings });
      }
      return undefined;
    });
    await openCacheTab();
    fireEvent.change(screen.getByDisplayValue("5120"), { target: { value: "2048" } });
    expect(screen.getByRole("button", { name: "保存并关闭" })).toBeInTheDocument();

    // 缓存标签页卸载（切到音频输出），表单状态挂在壳上，不随它丢失
    fireEvent.click(screen.getByRole("tab", { name: "音频输出" }));
    expect(screen.queryByDisplayValue("2048")).toBeNull();
    expect(screen.getByRole("button", { name: "保存并关闭" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "缓存管理" }));
    expect(screen.getByDisplayValue("2048")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("tab", { name: "系统集成" }));
    fireEvent.click(screen.getByRole("button", { name: "保存并关闭" }));
    await waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("update_cache_settings", {
        settings: { cacheDir: "D:\\cache", maxSizeMb: 2048, autoCleanup: true },
      })
    );
    await waitFor(() => expect(usePlayerStore.getState().settingsOpen).toBe(false));
  });

  it("UI-05：缓存上限不合法时「保存并关闭」不提交也不关闭", async () => {
    await openCacheTab();
    fireEvent.change(screen.getByDisplayValue("5120"), { target: { value: "64" } });
    fireEvent.click(screen.getByRole("button", { name: "保存并关闭" }));
    await waitFor(() =>
      expect(usePlayerStore.getState().notification?.text).toBe("缓存上限不能低于 128 MB")
    );
    expect(bridge.invoke).not.toHaveBeenCalledWith("update_cache_settings", expect.anything());
    expect(usePlayerStore.getState().settingsOpen).toBe(true);
  });

  it("UI-05：保存失败时提示后端原因，弹窗留着让用户修正", async () => {
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "get_cache_status") return status();
      if (command === "update_cache_settings") {
        throw { code: "invalid_input", message: "目录 D:\\music 已包含其他文件且缺少缓存标记" };
      }
      return undefined;
    });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    await openCacheTab();
    fireEvent.change(screen.getByDisplayValue("D:\\cache"), { target: { value: "D:\\music" } });
    fireEvent.click(screen.getByRole("button", { name: "保存缓存设置" }));
    await waitFor(() => {
      const notification = usePlayerStore.getState().notification;
      expect(notification?.text).toBe("保存缓存设置失败：目录 D:\\music 已包含其他文件且缺少缓存标记");
      expect(notification?.level).toBe("error");
    });
    expect(usePlayerStore.getState().settingsOpen).toBe(true);
    // 表单保持用户填的值，仍处于未保存状态
    expect(screen.getByDisplayValue("D:\\music")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "保存并关闭" })).toBeInTheDocument();
  });

  it("CACHE-03：旧缓存目录列出各自的用量，管不到的显示原因；合计仍按后端给的总数", async () => {
    bridge.invoke.mockImplementation(async (command) =>
      command === "get_cache_status"
        ? status({
            previousDirs: [
              { path: "E:\\old-cache", usedBytes: 1536 * 1024 * 1024, fileCount: 30 },
              {
                path: "F:\\usb-cache",
                usedBytes: 0,
                fileCount: 0,
                unavailableReason: "所在磁盘当前不可用，接回后继续计入用量与清理",
              },
            ],
          })
        : undefined
    );
    await openCacheTab();
    const list = screen.getByRole("list", { name: "旧缓存目录" });
    const items = within(list).getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("E:\\old-cache");
    expect(items[0]).toHaveTextContent("30 个文件 · 1.50 GB");
    expect(items[1]).toHaveTextContent("F:\\usb-cache");
    expect(items[1]).toHaveTextContent("所在磁盘当前不可用");
    // 清理确认框里的数字是含旧目录的合计
    fireEvent.click(screen.getByRole("button", { name: "清理缓存" }));
    expect(screen.getByRole("dialog", { name: "清理全部缓存" })).toHaveTextContent(
      "37 个已下载的流媒体音频文件"
    );
  });

  it("CACHE-03：没有旧目录时不出现这一块；改了缓存目录时说明原目录的文件不会移动", async () => {
    await openCacheTab();
    expect(screen.queryByRole("list", { name: "旧缓存目录" })).toBeNull();
    expect(screen.queryByText(/原目录里已下载的文件不会移动/)).toBeNull();
    fireEvent.change(screen.getByLabelText("Cache Path"), { target: { value: "E:\\new-cache" } });
    expect(screen.getByText(/原目录里已下载的文件不会移动/)).toBeInTheDocument();
    // 改回原值提示消失
    fireEvent.change(screen.getByLabelText("Cache Path"), { target: { value: "D:\\cache" } });
    expect(screen.queryByText(/原目录里已下载的文件不会移动/)).toBeNull();
  });

  it("UI-05：每个标签页只挂载自己的内容，事件可以直达指定标签页", async () => {
    render(<SettingsModal />);
    expect(screen.getByRole("tab", { name: "音频输出" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("音频输出设置")).toBeInTheDocument();
    expect(screen.queryByText("与 Windows 系统的集成能力。")).toBeNull();

    act(() => {
      window.dispatchEvent(new CustomEvent("seraph:open-settings-tab", { detail: "lyrics" }));
    });
    expect(screen.getByRole("tab", { name: "歌词设置" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("歌词源优先级")).toBeInTheDocument();
    expect(screen.queryByText("音频输出设置")).toBeNull();

    // 不认识的标签名不切换
    act(() => {
      window.dispatchEvent(new CustomEvent("seraph:open-settings-tab", { detail: "nope" }));
    });
    expect(screen.getByRole("tab", { name: "歌词设置" })).toHaveAttribute("aria-selected", "true");

    fireEvent.click(screen.getByRole("tab", { name: "关于与更新" }));
    expect(screen.getByRole("button", { name: "检查更新" })).toBeInTheDocument();
    expect(screen.queryByText("歌词源优先级")).toBeNull();
  });
});
