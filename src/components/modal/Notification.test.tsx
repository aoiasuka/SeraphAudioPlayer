// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { Notification } from "@/components/modal/Notification";
import { usePlayerStore } from "@/store/player";

beforeEach(() => {
  vi.useFakeTimers();
  usePlayerStore.setState({ notification: null });
});

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  usePlayerStore.getState().dismissNotification();
  usePlayerStore.getState().dismissNotification();
});

it("REL-08：错误提示不会被紧随其后的普通提示覆盖，错误消失后再显示普通提示", () => {
  const { showNotification, dismissNotification } = usePlayerStore.getState();
  showNotification("切换输出设备失败：设备被占用", "error");
  showNotification("已导入 3 首");
  expect(usePlayerStore.getState().notification).toMatchObject({
    text: "切换输出设备失败：设备被占用",
    level: "error",
  });
  dismissNotification();
  expect(usePlayerStore.getState().notification).toMatchObject({ text: "已导入 3 首", level: "info" });
});

it("普通提示之间照旧后者覆盖前者；新的错误总会立即显示", () => {
  const { showNotification } = usePlayerStore.getState();
  showNotification("甲");
  showNotification("乙");
  expect(usePlayerStore.getState().notification?.text).toBe("乙");
  showNotification("丙失败", "error");
  expect(usePlayerStore.getState().notification?.text).toBe("丙失败");
});

it("错误提示用 role=alert 并停留更久；普通提示用 status", () => {
  render(<Notification />);
  act(() => usePlayerStore.getState().showNotification("出错了", "error"));
  expect(screen.getByRole("alert")).toHaveTextContent("出错了");
  act(() => vi.advanceTimersByTime(3500));
  expect(usePlayerStore.getState().notification?.text).toBe("出错了");
  act(() => vi.advanceTimersByTime(4000));
  expect(usePlayerStore.getState().notification).toBeNull();

  act(() => usePlayerStore.getState().showNotification("好了"));
  expect(screen.getByRole("status")).toHaveTextContent("好了");
});
