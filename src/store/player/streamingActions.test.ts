// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { LOGIN_STATUS_TTL_MS, resetLoginStatusFreshnessForTest } from "./streamingActions";

vi.mock("@/lib/tauri", async (original) => ({
  ...(await original<typeof import("@/lib/tauri")>()),
  isTauriRuntime: () => true,
  invoke: vi.fn(async () => undefined),
}));

const invokeMock = vi.mocked(invoke);
const initialState = usePlayerStore.getInitialState();

function commands() {
  return invokeMock.mock.calls.map(([command]) => command);
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-11-15T00:00:00Z"));
  resetLoginStatusFreshnessForTest();
  usePlayerStore.setState(initialState, true);
  invokeMock.mockReset();
  invokeMock.mockImplementation(async (command) => {
    if (command === "bilibili_login_status") return { loggedIn: true, username: "测试" };
    if (command === "bilibili_ffmpeg_status") return { available: true };
    return undefined;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("refreshBilibiliState（UI4-02）", () => {
  it("已登录且在有效期内不再重复联网查询登录状态", async () => {
    await usePlayerStore.getState().refreshBilibiliState();
    expect(commands()).toContain("bilibili_login_status");
    expect(usePlayerStore.getState().bilibiliLoginStatus.loggedIn).toBe(true);

    invokeMock.mockClear();
    await usePlayerStore.getState().refreshBilibiliState();
    expect(commands()).not.toContain("bilibili_login_status");
    // 本地的 FFmpeg 探测照常刷新
    expect(commands()).toContain("bilibili_ffmpeg_status");
  });

  it("手动刷新（force）与有效期过后都会重新查询", async () => {
    await usePlayerStore.getState().refreshBilibiliState();
    invokeMock.mockClear();
    await usePlayerStore.getState().refreshBilibiliState({ force: true });
    expect(commands()).toContain("bilibili_login_status");

    invokeMock.mockClear();
    vi.advanceTimersByTime(LOGIN_STATUS_TTL_MS + 1000);
    await usePlayerStore.getState().refreshBilibiliState();
    expect(commands()).toContain("bilibili_login_status");
  });

  it("未登录时每次都查询", async () => {
    invokeMock.mockImplementation(async (command) =>
      command === "bilibili_login_status" ? { loggedIn: false } : { available: false }
    );
    await usePlayerStore.getState().refreshBilibiliState();
    invokeMock.mockClear();
    await usePlayerStore.getState().refreshBilibiliState();
    expect(commands()).toContain("bilibili_login_status");
  });
});

describe("流媒体导入偏好（UI4-02）", () => {
  it("写进 store，离开页面不丢", () => {
    const { setBilibiliImportPreference } = usePlayerStore.getState();
    setBilibiliImportPreference("bilibiliPreferFlac", true);
    setBilibiliImportPreference("bilibiliRemuxWithFfmpeg", true);
    const state = usePlayerStore.getState();
    expect(state.bilibiliPreferFlac).toBe(true);
    expect(state.bilibiliRemuxWithFfmpeg).toBe(true);
    expect(state.bilibiliPreferDolbyAtmos).toBe(false);
  });
});
