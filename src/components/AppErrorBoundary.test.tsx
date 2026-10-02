// @vitest-environment jsdom
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AppErrorBoundary } from "./AppErrorBoundary";

const showMock = vi.fn(async () => undefined);
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ show: showMock }),
}));

function Boom(): never {
  throw new Error("首帧渲染失败");
}

beforeEach(() => {
  showMock.mockClear();
  (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__ = {};
  vi.spyOn(console, "error").mockImplementation(() => undefined);
});

afterEach(() => {
  delete (window as unknown as { __TAURI_INTERNALS__?: object }).__TAURI_INTERNALS__;
  vi.restoreAllMocks();
});

it("首帧就崩溃时兜底页仍会显示窗口（主窗口 visible:false，App 的 reveal effect 不会执行）", async () => {
  render(
    <AppErrorBoundary>
      <Boom />
    </AppErrorBoundary>
  );
  expect(screen.getByText("界面渲染出现异常")).toBeTruthy();
  await waitFor(() => expect(showMock).toHaveBeenCalled());
});
