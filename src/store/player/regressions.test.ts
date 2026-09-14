// @vitest-environment jsdom
// 2026-09-12 审查的持久化、启动及异步竞态回归。
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, renderHook } from "@testing-library/react";
import { invoke } from "@/lib/tauri";
import { migratePersistedPlayerState, usePlayerStore } from "@/store/player";
import { createPlayerPersistStorage, hydrationGate } from "@/store/player/persistStorage";
import { useAnalysisSettingsStore } from "@/store/analysisSettings";
import { useHydratePlayerStore } from "@/hooks/useHydratePlayerStore";
import { applyPendingConfigImport, parseConfigImport, stashPendingImport } from "@/lib/configTransfer";
import { bumpPlayEpoch } from "@/store/player/playEpoch";
import { runWhenIdle } from "@/lib/startup";
import type { DeleteTracksResult, Track } from "@/types/track";

vi.mock("@/lib/tauri", async (original) => ({
  ...await original<typeof import("@/lib/tauri")>(),
  isTauriRuntime: () => true,
  invoke: vi.fn(async () => undefined),
}));
vi.mock("@/lib/startup", () => ({
  runWhenIdle: vi.fn((callback: () => void) => { callback(); return () => {}; }),
}));

const invokeMock = vi.mocked(invoke);
const playerInitial = usePlayerStore.getInitialState();
const analysisInitial = useAnalysisSettingsStore.getInitialState();

function track(id: string, overrides: Partial<Track> = {}): Track {
  return {
    id, title: id, artist: "测试", album: "测试", cover: "", format: "FLAC",
    bitdepth: "16-bit", sampleRate: "44.1 kHz", bitrate: "", channels: "Stereo",
    size: "1 MB", path: `C:/audit/${id}.flac`, duration: 180, glowColor: "#fff",
    lyrics: [], ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

async function flush() {
  for (let i = 0; i < 40; i++) await Promise.resolve();
}

function importConfig(stores: Record<string, unknown>) {
  stashPendingImport(parseConfigImport(JSON.stringify({
    kind: "seraph-config", version: 1, stores,
  })));
  expect(applyPendingConfigImport()).toBe(true);
}

beforeEach(() => {
  vi.useFakeTimers();
  invokeMock.mockReset();
  invokeMock.mockResolvedValue(undefined);
  vi.mocked(runWhenIdle).mockImplementation((callback) => { callback(); return () => {}; });
  hydrationGate.ready = false;
  usePlayerStore.persist.clearStorage();
  usePlayerStore.setState(playerInitial, true);
  useAnalysisSettingsStore.setState(analysisInitial, true);
  localStorage.clear();
  sessionStorage.clear();
  bumpPlayEpoch();
});

afterEach(() => {
  cleanup();
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe("BUG-01：任务栏设置必须独立持久化", () => {
  it.each([
    ["taskbarButtonsEnabled", false],
    ["taskbarProgressEnabled", false],
    ["taskbarLyricsEnabled", true],
    ["taskbarLyricsClickThrough", true],
    ["taskbarLyricsPosition", 0.25],
  ] as const)("单独修改 %s 后应写入磁盘", (field, value) => {
    const storage = createPlayerPersistStorage();
    hydrationGate.ready = true;
    const state = migratePersistedPlayerState({});
    storage.setItem("audit-player", { state, version: 3 });
    vi.advanceTimersByTime(301);
    storage.setItem("audit-player", { state: { ...state, [field]: value }, version: 3 });
    vi.advanceTimersByTime(301);
    window.dispatchEvent(new Event("pagehide"));
    expect(JSON.parse(localStorage.getItem("audit-player")!).state[field]).toBe(value);
  });
});

it("BUG-02：当前版本配置导入也必须校验 player 的枚举与范围", async () => {
  localStorage.setItem("seraph-player-state", JSON.stringify({
    state: migratePersistedPlayerState({}), version: 3,
  }));
  importConfig({ "seraph-player-state": {
    version: 3, state: { activeView: "invalid-view", volume: -5 },
  } });
  await usePlayerStore.persist.rehydrate();
  expect.soft(usePlayerStore.getState().activeView).toBe("local");
  expect.soft(usePlayerStore.getState().volume).toBeGreaterThanOrEqual(0);
});

it("BUG-02：当前版本分析配置的 panels=null 应恢复默认值", async () => {
  importConfig({ "seraph-analysis-settings": { version: 2, state: { panels: null } } });
  await useAnalysisSettingsStore.persist.rehydrate();
  expect(useAnalysisSettingsStore.getState().panels).not.toBeNull();
});

it("BUG-03：删除正在重缓存的曲目后，迟到响应不得覆盖下一首", async () => {
  const pending = deferred<Track>();
  const a = track("bilibili-BV1234567890-1", {
    sourceId: "BV1234567890", sourceUrl: "https://www.bilibili.com/video/BV1234567890",
    cacheMissing: true,
  });
  const b = track("local-b");
  usePlayerStore.setState({ playlist: [a, b], currentTrackIndex: 0, isPlaying: false });
  invokeMock.mockImplementation(async (command) => {
    if (command === "import_bilibili_audio_with_options") return pending.promise;
    if (command === "delete_tracks") return { deletedIds: [a.id], deletedFiles: 0, failures: [] };
    return undefined;
  });
  usePlayerStore.getState().loadTrack(0, { forcePlay: true });
  await usePlayerStore.getState().deleteTrack(a.id);
  expect(usePlayerStore.getState().playlist.map((item) => item.id)).toEqual([b.id]);
  pending.resolve({ ...a, cacheMissing: false });
  await flush();
  expect(usePlayerStore.getState().playlist.map((item) => item.id)).toEqual([b.id]);
});

it("BUG-04：旧设备枚举请求不得覆盖用户随后选择的设备", async () => {
  const pendingDriver = deferred<undefined>();
  const devices = [
    { id: "device-a", name: "A", isDefault: true },
    { id: "device-b", name: "B", isDefault: false },
  ];
  usePlayerStore.setState({ devices, currentDeviceId: "device-a" });
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_devices") return devices;
    if (command === "set_output_driver") return pendingDriver.promise;
    return undefined;
  });
  usePlayerStore.getState().loadDevices();
  await flush();
  usePlayerStore.getState().selectDevice("device-b");
  pendingDriver.resolve(undefined);
  await flush();
  expect(usePlayerStore.getState().currentDeviceId).toBe("device-b");
  const selects = invokeMock.mock.calls.filter(([command]) => command === "select_output_device");
  expect(selects.at(-1)?.[1]).toEqual({ deviceId: "device-b" });
});

it("BUG-03：删除请求等待期间也不能启动迟到的重缓存播放", async () => {
  const download = deferred<Track>();
  const deletion = deferred<DeleteTracksResult>();
  const a = track("pending-a", { sourceId: "BV1234567890", cacheMissing: true });
  const b = track("pending-b");
  usePlayerStore.setState({ playlist: [a, b], currentTrackIndex: 0 });
  invokeMock.mockImplementation(async (command) => {
    if (command === "import_bilibili_audio_with_options") return download.promise;
    if (command === "delete_tracks") return deletion.promise;
    return undefined;
  });
  usePlayerStore.getState().loadTrack(0, { forcePlay: true });
  const removed = usePlayerStore.getState().deleteTrack(a.id);
  download.resolve({ ...a, cacheMissing: false });
  await flush();
  const playsBeforeDeleteReply = invokeMock.mock.calls.filter(([command]) => command === "play");
  deletion.resolve({ deletedIds: [a.id], deletedFiles: 0, failures: [] });
  await removed;
  expect(playsBeforeDeleteReply).toEqual([]);
  expect(usePlayerStore.getState().playlist.map((item) => item.id)).toEqual([b.id]);
});

it("BUG-04：较早的设备枚举最后返回时不得覆盖最新列表", async () => {
  const oldDevices = deferred<unknown>();
  const newest = [{ id: "new-device", name: "新设备", isDefault: true }];
  let requests = 0;
  invokeMock.mockImplementation(async (command) => {
    if (command === "list_devices") return ++requests === 1 ? oldDevices.promise : newest;
    return undefined;
  });
  const first = usePlayerStore.getState().loadDevices();
  await usePlayerStore.getState().loadDevices();
  oldDevices.resolve([{ id: "old-device", name: "旧设备", isDefault: true }]);
  await first;
  expect(usePlayerStore.getState().devices.map((device) => device.id)).toEqual(["new-device"]);
  expect(usePlayerStore.getState().currentDeviceId).toBe("new-device");
});

it("BUG-05：切驱动期间的新选曲及定位不得被旧续播从零覆盖", async () => {
  const pendingStop = deferred<undefined>();
  const a = track("driver-a"), b = track("driver-b");
  usePlayerStore.setState({ playlist: [a, b], currentTrackIndex: 0, isPlaying: true });
  invokeMock.mockImplementation(async (command) => command === "stop" ? pendingStop.promise : undefined);
  usePlayerStore.getState().setDriver("wasapi");
  usePlayerStore.getState().loadTrack(1);
  usePlayerStore.getState().seek(40);
  pendingStop.resolve(undefined);
  await flush();
  expect(invokeMock.mock.calls.filter(([command]) => command === "play")).toEqual([]);
});

it("BUG-05：续播配置等待期间的最新定位在实际起播时生效", async () => {
  const applying = deferred<undefined>();
  let driverCalls = 0;
  usePlayerStore.setState({ playlist: [track("seek-during-driver")], isPlaying: true });
  invokeMock.mockImplementation(async (command) => {
    if (command === "set_output_driver" && ++driverCalls === 2) return applying.promise;
    return undefined;
  });
  usePlayerStore.getState().setDriver("wasapi");
  await flush();
  usePlayerStore.getState().seek(40);
  applying.resolve(undefined);
  await flush();
  const plays = invokeMock.mock.calls.filter(([command]) => command === "play");
  expect(plays).toHaveLength(1);
  expect(plays[0][1]).toMatchObject({ trackId: "seek-during-driver", startSeconds: 40 });
});

it("BUG-06：水合静音设置时必须同步引擎音量，供系统媒体键直接起播", async () => {
  localStorage.setItem("seraph-player-state", JSON.stringify({
    version: 3,
    state: migratePersistedPlayerState({ volume: 0, isMuted: true }),
  }));
  invokeMock.mockImplementation(async (command) => {
    if (command === "get_playlist") return [track("muted-start")];
    if (command === "list_devices") return [{ id: "device-a", name: "A", isDefault: true }];
    return undefined;
  });
  renderHook(() => useHydratePlayerStore());
  await flush();
  expect(usePlayerStore.getState().isMuted).toBe(true);
  expect(invokeMock).toHaveBeenCalledWith("set_volume", { volume: 0 });
});

it("BUG-06：静音同步不等待空闲曲库加载", async () => {
  vi.mocked(runWhenIdle).mockImplementation(() => () => {});
  localStorage.setItem("seraph-player-state", JSON.stringify({
    version: 3, state: migratePersistedPlayerState({ volume: 0, isMuted: true }),
  }));
  renderHook(() => useHydratePlayerStore());
  await flush();
  expect(invokeMock).toHaveBeenCalledWith("set_volume", { volume: 0 });
  expect(invokeMock.mock.calls.some(([command]) => command === "get_playlist")).toBe(false);
});
