// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import {
  applyPendingConfigImport,
  buildConfigExport,
  parseConfigImport,
  stashPendingImport,
} from "./configTransfer";

function seedPlayerState() {
  window.localStorage.setItem(
    "seraph-player-state",
    JSON.stringify({
      state: {
        volume: 0.55,
        isMuted: false,
        previousVolume: 0.7,
        shuffleMode: true,
        loopMode: false,
        currentDeviceId: "wasapi:test",
        driverKind: "direct",
        activeView: "analysis",
        smtcEnabled: true,
        rememberPlayback: false,
        // 个人数据：不应进入导出
        liked: { "track-1": true },
        userPlaylists: [{ id: "p1", name: "歌单" }],
        recentTrackIds: ["track-1"],
        persistedCurrentTrackId: "track-1",
      },
      version: 3,
    })
  );
}

describe("config export / import (v0.4.8)", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
  });

  it("exports settings fields only, stripping personal data from player state", () => {
    seedPlayerState();
    window.localStorage.setItem(
      "seraph-eq-state",
      JSON.stringify({ state: { enabled: true, preamp: -2 }, version: 1 })
    );

    const text = buildConfigExport(new Date("2026-07-19T00:00:00Z"));
    expect(text).toBeTruthy();
    const parsed = JSON.parse(text!) as {
      kind: string;
      version: number;
      stores: Record<string, { state: Record<string, unknown> }>;
    };
    expect(parsed.kind).toBe("seraph-config");
    const playerState = parsed.stores["seraph-player-state"].state;
    expect(playerState.volume).toBe(0.55);
    expect(playerState.shuffleMode).toBe(true);
    expect(playerState).not.toHaveProperty("liked");
    expect(playerState).not.toHaveProperty("userPlaylists");
    expect(playerState).not.toHaveProperty("recentTrackIds");
    expect(parsed.stores["seraph-eq-state"].state.enabled).toBe(true);
  });

  it("returns null when nothing is persisted", () => {
    expect(buildConfigExport()).toBeNull();
  });

  it("parseConfigImport rejects invalid payloads with user-facing messages", () => {
    expect(() => parseConfigImport("not json")).toThrow("JSON");
    expect(() => parseConfigImport("{}")).toThrow("Seraph");
    expect(() =>
      parseConfigImport(JSON.stringify({ kind: "seraph-config", version: 99, stores: {} }))
    ).toThrow("版本较新");
    expect(() =>
      parseConfigImport(JSON.stringify({ kind: "seraph-config", version: 1, stores: {} }))
    ).toThrow("没有可导入");
  });

  it("stash + apply writes stores back and merges player settings over local data", () => {
    seedPlayerState();
    const stores = parseConfigImport(
      JSON.stringify({
        kind: "seraph-config",
        version: 1,
        stores: {
          "seraph-player-state": {
            state: { volume: 0.9, shuffleMode: false, driverKind: "wasapi" },
            version: 3,
          },
          "seraph-analysis-settings": {
            state: {
              levelsMode: "vu",
              panels: { scope: false },
              layoutMode: "custom",
              customLayout: { loudness: { x: 0, y: 0, w: 3, h: 5 } },
            },
            version: 2,
          },
        },
      })
    );
    stashPendingImport(stores);
    expect(applyPendingConfigImport()).toBe(true);

    const player = JSON.parse(
      window.localStorage.getItem("seraph-player-state")!
    ) as { state: Record<string, unknown>; version: number };
    // 设置字段被导入值覆盖
    expect(player.state.volume).toBe(0.9);
    expect(player.state.shuffleMode).toBe(false);
    expect(player.state.driverKind).toBe("wasapi");
    // 本机个人数据保留
    expect(player.state.liked).toEqual({ "track-1": true });
    expect(player.version).toBe(3);

    const analysis = JSON.parse(
      window.localStorage.getItem("seraph-analysis-settings")!
    ) as { state: Record<string, unknown>; version: number };
    expect(analysis.state.levelsMode).toBe("vu");
    // v0.4.9 布局字段随 envelope 透传（结构校验由 store migrate 水合时兜底）
    expect(analysis.version).toBe(2);
    expect(analysis.state.layoutMode).toBe("custom");
    expect(analysis.state.customLayout).toEqual({
      loudness: { x: 0, y: 0, w: 3, h: 5 },
    });

    // 应用后暂存清空，二次调用为空操作
    expect(applyPendingConfigImport()).toBe(false);
  });
});
