// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { buildPersonalDataExport } from "@/store/player/personalData";
import { SystemSettingsTab } from "./SystemSettingsTab";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
  open: vi.fn<() => Promise<unknown>>(),
  save: vi.fn<() => Promise<unknown>>(),
}));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  invoke: (command: string, args?: Record<string, unknown>) => bridge.invoke(command, args),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: () => bridge.open(),
  save: () => bridge.save(),
}));

const setters = {
  setSmtcEnabled: vi.fn(),
  setTaskbarButtonsEnabled: vi.fn(),
  setTaskbarProgressEnabled: vi.fn(),
  setRememberPlayback: vi.fn(),
};

describe("设置页 · 系统集成（UI-05）", () => {
  beforeEach(() => {
    bridge.invoke.mockReset();
    bridge.open.mockReset();
    bridge.save.mockReset();
    Object.values(setters).forEach((setter) => setter.mockClear());
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      ...setters,
      smtcEnabled: true,
      taskbarButtonsEnabled: false,
      taskbarProgressEnabled: true,
      rememberPlayback: false,
    });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("四个开关各有自己的名称，显示当前状态，点击写入取反后的值", () => {
    render(<SystemSettingsTab />);
    const cases = [
      ["系统媒体控件", true, setters.setSmtcEnabled],
      ["任务栏播控按钮", false, setters.setTaskbarButtonsEnabled],
      ["任务栏播放进度", true, setters.setTaskbarProgressEnabled],
      ["记忆播放", false, setters.setRememberPlayback],
    ] as const;
    for (const [name, enabled, setter] of cases) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("aria-pressed", String(enabled));
      expect(button).toHaveTextContent(enabled ? "已启用" : "已停用");
      fireEvent.click(button);
      expect(setter).toHaveBeenCalledWith(!enabled);
    }
  });

  it("没有收藏和歌单时导出只提示，不弹保存框", async () => {
    render(<SystemSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "导出收藏与歌单…" }));
    await waitFor(() =>
      expect(usePlayerStore.getState().notification?.text).toBe("还没有收藏或歌单可以导出")
    );
    expect(bridge.save).not.toHaveBeenCalled();
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it("导出收藏与歌单：把内容写到所选路径；取消保存框则什么都不写", async () => {
    usePlayerStore.setState({ liked: { "track-1": true }, likedAt: { "track-1": 1 } });
    bridge.save.mockResolvedValueOnce(null);
    render(<SystemSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "导出收藏与歌单…" }));
    await waitFor(() => expect(bridge.save).toHaveBeenCalledTimes(1));
    expect(bridge.invoke).not.toHaveBeenCalled();

    bridge.save.mockResolvedValueOnce("D:\\backup\\personal.json");
    bridge.invoke.mockResolvedValue(undefined);
    fireEvent.click(screen.getByRole("button", { name: "导出收藏与歌单…" }));
    await waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("export_app_config", {
        path: "D:\\backup\\personal.json",
        content: expect.stringContaining("track-1"),
      })
    );
    await waitFor(() =>
      expect(usePlayerStore.getState().notification?.text).toBe("收藏与歌单已导出")
    );
  });

  it("导入收藏与歌单是合并：本机已有的收藏保留，文件里的追加进来", async () => {
    const file = buildPersonalDataExport({
      liked: { "track-2": true },
      likedAt: { "track-2": 2 },
      userPlaylists: [{ id: "pl-1", name: "路上", trackIds: ["track-2"], createdAt: 3 }],
    });
    expect(file).not.toBeNull();
    usePlayerStore.setState({ liked: { "track-1": true }, likedAt: { "track-1": 1 } });
    bridge.open.mockResolvedValue("D:\\backup\\personal.json");
    bridge.invoke.mockImplementation(async (command) =>
      command === "import_app_config" ? file : undefined
    );
    render(<SystemSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "导入收藏与歌单…" }));
    await waitFor(() =>
      expect(usePlayerStore.getState().notification?.text).toMatch(/^已合并：新增收藏 1 首、新增歌单 1 张/)
    );
    const state = usePlayerStore.getState();
    expect(state.liked).toEqual({ "track-1": true, "track-2": true });
    expect(state.userPlaylists.map((playlist) => playlist.id)).toEqual(["pl-1"]);
  });

  it("导入的文件不是个人数据时报错并带上原因，本机数据不动", async () => {
    usePlayerStore.setState({ liked: { "track-1": true }, likedAt: { "track-1": 1 } });
    bridge.open.mockResolvedValue("D:\\backup\\other.json");
    bridge.invoke.mockResolvedValue("{}");
    vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<SystemSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "导入收藏与歌单…" }));
    await waitFor(() => {
      const notification = usePlayerStore.getState().notification;
      expect(notification?.text).toMatch(/^导入收藏与歌单失败：.+/);
      expect(notification?.level).toBe("error");
    });
    expect(usePlayerStore.getState().liked).toEqual({ "track-1": true });
  });
});
