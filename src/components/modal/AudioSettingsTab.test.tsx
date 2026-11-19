// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { AudioSettingsTab } from "./AudioSettingsTab";

const actions = {
  setDriver: vi.fn(),
  selectDevice: vi.fn(),
  loadDevices: vi.fn(async () => {}),
};

describe("设置页 · 音频输出（UI-05）", () => {
  beforeEach(() => {
    Object.values(actions).forEach((action) => action.mockClear());
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      ...actions,
      driverKind: "direct",
      currentDeviceId: "dev-b",
      devices: [
        { id: "dev-a", name: "Speakers", isDefault: true },
        { id: "dev-b", name: "USB DAC", isDefault: false },
      ],
    });
  });
  afterEach(cleanup);

  it("显示当前驱动的说明与当前设备，默认设备带标记", () => {
    render(<AudioSettingsTab />);
    expect(screen.getByLabelText("Driver Interface")).toHaveValue("direct");
    expect(screen.getByText(/使用系统共享音频栈/)).toBeInTheDocument();
    expect(screen.getByLabelText("输出设备")).toHaveValue("dev-b");
    // 标题下方的当前设备名（下拉框里还有一份同名 option）
    expect(screen.getByText("USB DAC", { selector: "p" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Speakers · Default" })).toBeInTheDocument();
  });

  it("切换驱动与设备即时调用 store；未开放的 ASIO 不可选", () => {
    render(<AudioSettingsTab />);
    fireEvent.change(screen.getByLabelText("Driver Interface"), { target: { value: "wasapi" } });
    expect(actions.setDriver).toHaveBeenCalledWith("wasapi");
    fireEvent.change(screen.getByLabelText("输出设备"), { target: { value: "dev-a" } });
    expect(actions.selectDevice).toHaveBeenCalledWith("dev-a");
    expect(screen.getByRole("option", { name: "ASIO 专业驱动（未开放）" })).toBeDisabled();
  });

  it("刷新按钮按完整流程重新读取设备（不把点击事件当成选项传进去）", () => {
    render(<AudioSettingsTab />);
    fireEvent.click(screen.getByRole("button", { name: "刷新" }));
    expect(actions.loadDevices).toHaveBeenCalledTimes(1);
    expect(actions.loadDevices).toHaveBeenCalledWith();
  });

  it("设备尚未枚举出来时给出读取中的提示；设备已不在列表里时显示它的 ID", () => {
    usePlayerStore.setState({ devices: [], currentDeviceId: "" });
    const first = render(<AudioSettingsTab />);
    expect(screen.getByText("正在读取设备…")).toBeInTheDocument();
    first.unmount();

    usePlayerStore.setState({ devices: [], currentDeviceId: "wasapi:gone" });
    render(<AudioSettingsTab />);
    expect(screen.getByText("wasapi:gone")).toBeInTheDocument();
  });
});
