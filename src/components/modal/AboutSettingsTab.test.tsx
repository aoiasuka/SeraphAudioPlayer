// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateCheckResult } from "@/lib/update";
import { usePlayerStore } from "@/store/player";
import { AboutSettingsTab } from "./AboutSettingsTab";

const update = vi.hoisted(() => ({
  checkForUpdate: vi.fn<() => Promise<unknown>>(),
  openReleasePage: vi.fn<(url: string) => Promise<void>>(),
}));
vi.mock("@/lib/update", () => update);

const result = (overrides: Partial<UpdateCheckResult> = {}): UpdateCheckResult => ({
  currentVersion: "0.6.4",
  latestVersion: "0.6.4",
  updateAvailable: false,
  releaseUrl: "https://github.com/aoiasuka/SeraphAudioPlayer/releases/tag/v0.6.4",
  releaseNotes: null,
  ...overrides,
});

const setAutoUpdateCheck = vi.fn();

describe("设置页 · 关于与更新（UI-05）", () => {
  beforeEach(() => {
    update.checkForUpdate.mockReset();
    update.openReleasePage.mockReset();
    setAutoUpdateCheck.mockClear();
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      setAutoUpdateCheck,
      autoUpdateCheck: true,
    });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("已是最新版本：提示一次，不出现下载按钮", async () => {
    update.checkForUpdate.mockResolvedValue(result());
    render(<AboutSettingsTab />);
    expect(screen.getByText("检查是否有新版本可用")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    expect(await screen.findByText("已是最新版本")).toBeInTheDocument();
    expect(screen.getByText(/Seraph Audio Player v0\.6\.4/)).toBeInTheDocument();
    expect(usePlayerStore.getState().notification?.text).toBe("已是最新版本 v0.6.4");
    expect(screen.queryByRole("button", { name: "前往下载" })).toBeNull();
  });

  it("有新版本：显示版本号，「前往下载」打开后端给出的发布页", async () => {
    const releaseUrl = "https://github.com/aoiasuka/SeraphAudioPlayer/releases/tag/v0.7.0";
    update.checkForUpdate.mockResolvedValue(
      result({ latestVersion: "0.7.0", updateAvailable: true, releaseUrl })
    );
    update.openReleasePage.mockResolvedValue();
    render(<AboutSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    expect(await screen.findByText(/发现新版本 v0\.7\.0/)).toBeInTheDocument();
    // 有新版本时不再弹「已是最新」的通知
    expect(usePlayerStore.getState().notification).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "前往下载" }));
    await waitFor(() => expect(update.openReleasePage).toHaveBeenCalledWith(releaseUrl));
  });

  it("检查失败给出错误提示，按钮恢复可用", async () => {
    update.checkForUpdate.mockRejectedValue({ code: "network", message: "offline" });
    vi.spyOn(console, "warn").mockImplementation(() => {});
    render(<AboutSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await waitFor(() => {
      const notification = usePlayerStore.getState().notification;
      expect(notification?.text).toBe("检查更新失败，请稍后重试");
      expect(notification?.level).toBe("error");
    });
    expect(screen.getByRole("button", { name: "检查更新" })).toBeEnabled();
  });

  it("「启动时检查更新」开关写入取反后的值", () => {
    render(<AboutSettingsTab />);
    const toggle = screen.getByRole("button", { name: "启动时检查更新" });
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(toggle);
    expect(setAutoUpdateCheck).toHaveBeenCalledWith(false);
  });
});
