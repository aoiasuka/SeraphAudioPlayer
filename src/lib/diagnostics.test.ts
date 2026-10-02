// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import { describeForLog, installDiagnosticsForwarding } from "./diagnostics";

const originalWarn = console.warn;
const originalError = console.error;

afterEach(() => {
  console.warn = originalWarn;
  console.error = originalError;
});

it("describeForLog：IpcError 对象、Error、普通值都转成可读文本", () => {
  expect(describeForLog({ code: "io", message: "磁盘已满" })).toBe("磁盘已满 (io)");
  expect(describeForLog(new TypeError("boom"))).toContain("TypeError: boom");
  expect(describeForLog("plain")).toBe("plain");
  expect(describeForLog({ a: 1 })).toBe('{"a":1}');
  const circular: Record<string, unknown> = {};
  circular.self = circular;
  expect(describeForLog(circular)).toBe("[object Object]");
});

it("REL-06：console.warn / error 与未捕获异常转发到后端日志，并保留原始控制台输出", () => {
  const send = vi.fn(async () => undefined);
  const printed = vi.fn();
  console.warn = printed;
  console.error = printed;
  const uninstall = installDiagnosticsForwarding("main", send);

  console.warn("同步失败", { code: "internal", message: "窗口创建失败" });
  expect(printed).toHaveBeenCalledTimes(1);
  expect(send).toHaveBeenLastCalledWith({ level: "warn", scope: "main", message: "同步失败 窗口创建失败 (internal)" });

  window.dispatchEvent(new ErrorEvent("error", { error: new Error("render exploded"), message: "render exploded" }));
  expect(send).toHaveBeenLastCalledWith(expect.objectContaining({ level: "error", scope: "main:uncaught" }));

  const rejection = new Event("unhandledrejection") as Event & { reason?: unknown };
  rejection.reason = "late failure";
  window.dispatchEvent(rejection);
  expect(send).toHaveBeenLastCalledWith({ level: "error", scope: "main:unhandledrejection", message: "late failure" });

  uninstall();
  send.mockClear();
  console.warn("after uninstall");
  expect(send).not.toHaveBeenCalled();
});

it("发送过程中再次打印日志不会递归转发", () => {
  const send = vi.fn(async () => {
    console.warn("inside send");
  });
  console.warn = vi.fn();
  const uninstall = installDiagnosticsForwarding("main", send);
  console.warn("outer");
  expect(send).toHaveBeenCalledTimes(1);
  uninstall();
});
