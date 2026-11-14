// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { NotificationHistory } from "@/components/layout/NotificationHistory";
import { Notification } from "@/components/modal/Notification";
import { usePlayerStore } from "@/store/player";

beforeEach(() => {
  vi.useFakeTimers();
  usePlayerStore.setState({ notification: null, notificationHistory: [] });
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

it("UI4-03：错误提示可以手动关闭", () => {
  render(<Notification />);
  act(() => usePlayerStore.getState().showNotification("出错了", "error"));
  fireEvent.click(screen.getByRole("button", { name: "关闭提示" }));
  expect(usePlayerStore.getState().notification).toBeNull();
});

it("UI4-03：被后一个错误顶掉的错误仍能在最近通知里回看", () => {
  const { showNotification } = usePlayerStore.getState();
  showNotification("第一个错误", "error");
  showNotification("第二个错误", "error");
  showNotification("已导入 3 首");
  expect(usePlayerStore.getState().notificationHistory.map((item) => item.text)).toEqual([
    "已导入 3 首",
    "第二个错误",
    "第一个错误",
  ]);

  render(<NotificationHistory />);
  fireEvent.click(screen.getByRole("button", { name: /最近通知/ }));
  const panel = screen.getByRole("dialog", { name: "最近通知" });
  expect(panel).toHaveTextContent("第一个错误");
  expect(panel).toHaveTextContent("2 条错误");
  fireEvent.click(screen.getByRole("button", { name: "清空" }));
  expect(usePlayerStore.getState().notificationHistory).toEqual([]);
});

it("UI4-03：历史有条数上限", () => {
  const { showNotification } = usePlayerStore.getState();
  for (let index = 0; index < 80; index += 1) showNotification(`第 ${index} 条`);
  const history = usePlayerStore.getState().notificationHistory;
  expect(history).toHaveLength(50);
  expect(history[0].text).toBe("第 79 条");
});
