// @vitest-environment jsdom
// BUG-06：设置类命令失败时必须回滚界面状态并提示，不能先报成功、失败只写日志。
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { resetPlaybackQueueSync } from "./queueSync";

vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn(async () => undefined),
}));

const invokeMock = vi.mocked(invoke);

async function flush() {
  for (let i = 0; i < 5; i += 1) await new Promise((resolve) => setTimeout(resolve, 0));
}

beforeEach(() => {
  resetPlaybackQueueSync();
  invokeMock.mockReset();
  invokeMock.mockImplementation(async () => undefined);
  usePlayerStore.setState({
    ...usePlayerStore.getInitialState(),
    devices: [
      { id: "device-a", name: "A", isDefault: true },
      { id: "device-b", name: "B", isDefault: false },
    ],
    currentDeviceId: "device-a",
    smtcEnabled: true,
    taskbarButtonsEnabled: true,
    taskbarProgressEnabled: true,
    taskbarLyricsEnabled: false,
    taskbarLyricsClickThrough: false,
    notification: null,
  });
});

it("切换输出设备成功后才提示已切换", async () => {
  usePlayerStore.getState().selectDevice("device-b");
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-b");
  expect(usePlayerStore.getState().notification?.text).toBe("输出设备已切换到: B");
});

it("切换输出设备失败：回滚到原设备并提示原因", async () => {
  invokeMock.mockImplementation(async (command) => {
    if (command === "select_output_device") throw { code: "internal", message: "设备被占用" };
    return undefined;
  });
  usePlayerStore.getState().selectDevice("device-b");
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-a");
  expect(usePlayerStore.getState().notification?.text).toContain("切换输出设备失败");
  expect(usePlayerStore.getState().notification?.text).toContain("设备被占用");
});

it.each([
  ["setSmtcEnabled", "set_smtc_enabled", "smtcEnabled", false],
  ["setTaskbarButtonsEnabled", "set_taskbar_features", "taskbarButtonsEnabled", false],
  ["setTaskbarProgressEnabled", "set_taskbar_features", "taskbarProgressEnabled", false],
  ["setTaskbarLyricsEnabled", "set_taskbar_lyrics_enabled", "taskbarLyricsEnabled", true],
  ["setTaskbarLyricsClickThrough", "set_taskbar_lyrics_click_through", "taskbarLyricsClickThrough", true],
] as const)("%s 失败时回滚开关并提示", async (action, command, key, next) => {
  invokeMock.mockImplementation(async (cmd) => {
    if (cmd === command) throw "boom";
    return undefined;
  });
  const before = usePlayerStore.getState()[key];
  usePlayerStore.getState()[action](next);
  await flush();
  expect(usePlayerStore.getState()[key]).toBe(before);
  expect(usePlayerStore.getState().notification?.text).toContain("失败");
});

it("随机 / 循环模式同步失败时回滚并提示，成功后才提示", async () => {
  usePlayerStore.getState().toggleLoop();
  await flush();
  expect(usePlayerStore.getState().loopMode).toBe(true);
  expect(usePlayerStore.getState().notification?.text).toBe("单曲循环已开启");

  invokeMock.mockImplementation(async (command) => {
    if (command === "sync_playback_queue") throw "queue unavailable";
    return undefined;
  });
  usePlayerStore.getState().toggleShuffle();
  await flush();
  expect(usePlayerStore.getState().shuffleMode).toBe(false);
  expect(usePlayerStore.getState().notification?.text).toContain("切换播放顺序失败");
});

function deferred() {
  let resolve!: () => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<undefined>((done, fail) => {
    resolve = () => done(undefined);
    reject = fail;
  });
  return { promise, resolve, reject };
}

it.each([
  ["setSmtcEnabled", "smtcEnabled", false],
  ["setTaskbarLyricsEnabled", "taskbarLyricsEnabled", true],
  ["setTaskbarLyricsClickThrough", "taskbarLyricsClickThrough", true],
] as const)("%s 的 A→B→A 迟到失败不能回滚最后意图", async (action, key, next) => {
  const first = deferred();
  invokeMock.mockImplementationOnce(() => first.promise);
  usePlayerStore.getState()[action](next);
  usePlayerStore.getState()[action](!next);
  usePlayerStore.getState()[action](next);
  await flush();
  first.reject("旧请求失败");
  await flush();
  expect(usePlayerStore.getState()[key]).toBe(next);
  expect(usePlayerStore.getState().notification?.text).not.toContain("失败");
});

it("设备 A→B→A→B 时迟到失败不能回滚最新 B", async () => {
  const first = deferred();
  invokeMock.mockImplementationOnce(() => first.promise);
  usePlayerStore.getState().selectDevice("device-b");
  usePlayerStore.getState().selectDevice("device-a");
  usePlayerStore.getState().selectDevice("device-b");
  await flush();
  first.reject("旧设备失败");
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-b");
});

it.each(["toggleShuffle", "toggleLoop"] as const)("%s 的迟到失败不得回滚再次选中的模式", async (action) => {
  const first = deferred();
  invokeMock.mockImplementationOnce(() => first.promise);
  usePlayerStore.getState()[action]();
  usePlayerStore.getState()[action]();
  usePlayerStore.getState()[action]();
  await flush();
  first.reject("旧模式失败");
  await flush();
  expect(usePlayerStore.getState()[action === "toggleShuffle" ? "shuffleMode" : "loopMode"]).toBe(true);
});

it.each(["setTaskbarButtonsEnabled", "setTaskbarProgressEnabled"] as const)("%s 与另一字段交叉更新时串行发送，失败后用修正快照继续", async (action) => {
  const first = deferred();
  invokeMock.mockImplementationOnce(() => first.promise);
  usePlayerStore.getState()[action](false);
  const other = action === "setTaskbarButtonsEnabled" ? "setTaskbarProgressEnabled" : "setTaskbarButtonsEnabled";
  usePlayerStore.getState()[other](false);
  expect(invokeMock).toHaveBeenCalledTimes(1);
  first.reject("首项失败");
  await flush();
  const buttonsFirst = action === "setTaskbarButtonsEnabled";
  expect(invokeMock).toHaveBeenLastCalledWith("set_taskbar_features", {
    buttons: buttonsFirst, progress: !buttonsFirst,
  });
  expect(usePlayerStore.getState().taskbarButtonsEnabled).toBe(buttonsFirst);
  expect(usePlayerStore.getState().taskbarProgressEnabled).toBe(!buttonsFirst);
});

it.each(["stop", "set_output_driver"])("切换驱动的 %s 失败会还原 driverKind，但不盲目恢复播放", async (failedCommand) => {
  usePlayerStore.setState({ driverKind: "direct", isPlaying: true, currentTime: 42 });
  invokeMock.mockImplementation(async (command) => {
    if (command === failedCommand) throw "驱动失败";
    return undefined;
  });
  usePlayerStore.getState().setDriver("wasapi");
  await flush();
  expect(usePlayerStore.getState().driverKind).toBe("direct");
  expect(usePlayerStore.getState().isPlaying).toBe(false);
});

it("旧驱动失败不覆盖新驱动意图或弹出过期错误", async () => {
  const first = deferred();
  usePlayerStore.setState({ driverKind: "direct" });
  invokeMock.mockImplementationOnce(() => first.promise);
  usePlayerStore.getState().setDriver("wasapi");
  usePlayerStore.getState().setDriver("direct");
  usePlayerStore.getState().setDriver("wasapi");
  await flush();
  first.reject("旧驱动失败");
  await flush();
  expect(usePlayerStore.getState().driverKind).toBe("wasapi");
  expect(usePlayerStore.getState().notification).toBeNull();
});

it.each([false, true])("连续设备请求首项成功=%s：最新失败回到最后确认的设备", async (firstSucceeds) => {
  const first = deferred();
  const second = deferred();
  invokeMock.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
  usePlayerStore.getState().selectDevice("device-b");
  usePlayerStore.getState().selectDevice("device-c");
  expect.soft(invokeMock).toHaveBeenCalledTimes(1);
  if (firstSucceeds) first.resolve(); else first.reject("第一项失败");
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-c");
  second.reject("第二项失败");
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe(firstSucceeds ? "device-b" : "device-a");
});

it.each([
  ["setSmtcEnabled", "smtcEnabled"],
  ["setTaskbarLyricsEnabled", "taskbarLyricsEnabled"],
  ["setTaskbarLyricsClickThrough", "taskbarLyricsClickThrough"],
  ["setTaskbarButtonsEnabled", "taskbarButtonsEnabled"],
  ["setTaskbarProgressEnabled", "taskbarProgressEnabled"],
] as const)("%s 连续双失败保留初始确认值，旧成功新失败保留旧成功值", async (action, key) => {
  for (const firstSucceeds of [false, true]) {
    const before = usePlayerStore.getState()[key];
    const first = deferred();
    const second = deferred();
    invokeMock.mockClear();
    invokeMock.mockImplementationOnce(() => first.promise).mockImplementationOnce(() => second.promise);
    usePlayerStore.getState()[action](!before);
    usePlayerStore.getState()[action](before);
    expect.soft(invokeMock).toHaveBeenCalledTimes(1);
    if (firstSucceeds) first.resolve(); else first.reject("第一项失败");
    await flush();
    expect(usePlayerStore.getState()[key]).toBe(before);
    second.reject("第二项失败");
    await flush();
    expect.soft(usePlayerStore.getState()[key]).toBe(firstSucceeds ? !before : before);
  }
});

it.each([
  { firstSucceeds: false, failedCommand: "stop" },
  { firstSucceeds: true, failedCommand: "stop" },
  { firstSucceeds: false, failedCommand: "set_output_driver" },
  { firstSucceeds: true, failedCommand: "set_output_driver" },
])("连续驱动请求 $failedCommand 首项成功=$firstSucceeds：失败回到最后确认的驱动", async ({ firstSucceeds, failedCommand }) => {
  const first = deferred();
  const second = deferred();
  let requests = 0;
  usePlayerStore.setState({ driverKind: "direct", isPlaying: false });
  invokeMock.mockImplementation((command) => {
    if (command === failedCommand) return ++requests === 1 ? first.promise : second.promise;
    return Promise.resolve(undefined);
  });
  usePlayerStore.getState().setDriver("wasapi");
  await flush();
  usePlayerStore.getState().setDriver("direct");
  await flush();
  expect.soft(requests).toBe(1);
  if (firstSucceeds) first.resolve(); else first.reject("第一项失败");
  await flush();
  expect(usePlayerStore.getState().driverKind).toBe("direct");
  second.reject("第二项失败");
  await flush();
  expect(usePlayerStore.getState().driverKind).toBe(firstSucceeds ? "wasapi" : "direct");
  expect(usePlayerStore.getState().isPlaying).toBe(false);
});

it.each([{ devices: [] }, { devices: [{ id: "speaker", name: "扬声器", isDefault: true }] }])("仅枚举设备时不改变当前选择、不下发输出配置", async ({ devices }) => {
  invokeMock.mockResolvedValue(devices);
  await usePlayerStore.getState().loadDevices({ enumerateOnly: true });
  expect(usePlayerStore.getState().devices).toEqual(devices.map((device) => ({ ...device, legacyIds: [] })));
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-a");
  expect(invokeMock.mock.calls.map(([command]) => command)).toEqual(["list_devices"]);
});

it("失败回滚不覆盖期间的新选择", async () => {
  let rejectFirst!: (reason: unknown) => void;
  invokeMock.mockImplementationOnce(
    () => new Promise((_, reject) => { rejectFirst = reject; })
  );
  usePlayerStore.getState().setSmtcEnabled(false);
  usePlayerStore.getState().setSmtcEnabled(true);
  rejectFirst("late failure");
  await flush();
  expect(usePlayerStore.getState().smtcEnabled).toBe(true);
});
