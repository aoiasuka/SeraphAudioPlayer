// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const runtime = () => { (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {}; };
beforeEach(() => { vi.resetModules(); });
afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  vi.doUnmock("@tauri-apps/api/core");
  vi.doUnmock("@tauri-apps/api/event");
  vi.restoreAllMocks();
});

it("浏览器预览的 stub 不会遮蔽随后建立的桌面通信", async () => {
  const realInvoke = vi.fn(async () => "played");
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: realInvoke }));
  const { invoke } = await import("./tauri");
  await expect(invoke("play")).resolves.toBeUndefined();
  runtime();
  await expect(invoke("play")).resolves.toBe("played");
  expect(realInvoke).toHaveBeenCalledTimes(1);
});

it("桌面 API 加载失败明确报错，后续成功加载后可继续调用", async () => {
  runtime();
  vi.doMock("@tauri-apps/api/core", () => { throw new Error("load failed"); });
  const { invoke } = await import("./tauri");
  await expect(invoke("play")).rejects.toThrow("桌面通信初始化失败");
  const realInvoke = vi.fn(async () => "recovered");
  vi.doMock("@tauri-apps/api/core", () => ({ invoke: realInvoke }));
  await expect(invoke("play")).resolves.toBe("recovered");
});

it("桌面事件加载失败不能伪装为订阅成功，恢复后保留窗口目标", async () => {
  runtime();
  vi.doMock("@tauri-apps/api/event", () => { throw new Error("load failed"); });
  const { listen } = await import("./tauri");
  await expect(listen("event", () => undefined)).rejects.toThrow("桌面事件初始化失败");
  const unsubscribe = vi.fn();
  const realListen = vi.fn(async () => unsubscribe);
  vi.doMock("@tauri-apps/api/event", () => ({ listen: realListen }));
  await expect(listen("event", () => undefined, "main")).resolves.toBe(unsubscribe);
  expect(realListen).toHaveBeenCalledWith("event", expect.any(Function), { target: { kind: "AnyLabel", label: "main" } });
});
