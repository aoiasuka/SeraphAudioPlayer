// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { migratePersistedPlayerState } from "@/store/player";

/**
 * W-03:歌单元素级消毒的回归测试。
 * 坏元素以前能水合进 store,渲染期 `item.trackIds.includes(...)` 直接 TypeError 白屏。
 */
describe("sanitizeUserPlaylists", () => {
  it("丢弃结构损坏的歌单元素而不是整体接受", () => {
    const state = migratePersistedPlayerState({
      userPlaylists: [
        null,
        "not an object",
        { name: "缺 id" },
        { id: "", name: "空 id" },
        { id: "ok-1", name: "正常", trackIds: ["a", "b"], createdAt: 123 },
        // trackIds 缺失 / 类型错误 → 补成空数组,而不是留 undefined
        { id: "ok-2", name: "缺 trackIds" },
        { id: "ok-3", trackIds: ["x", 42, null, "y"] },
      ],
    });

    const ids = state.userPlaylists.map((item) => item.id);
    expect(ids).toEqual(["ok-1", "ok-2", "ok-3"]);
    // 每个存活元素都必须能安全调用 .trackIds.includes
    for (const playlist of state.userPlaylists) {
      expect(Array.isArray(playlist.trackIds)).toBe(true);
      expect(() => playlist.trackIds.includes("a")).not.toThrow();
    }
    expect(state.userPlaylists[1].trackIds).toEqual([]);
    // 数组内的非字符串项被过滤掉
    expect(state.userPlaylists[2].trackIds).toEqual(["x", "y"]);
    // 缺 name 时给默认名,不留 undefined
    expect(state.userPlaylists[1].name).toBe("缺 trackIds");
    expect(state.userPlaylists[2].name).toBe("未命名歌单");
  });

  it("非数组输入退化为空数组", () => {
    expect(migratePersistedPlayerState({ userPlaylists: null }).userPlaylists).toEqual([]);
    expect(migratePersistedPlayerState({ userPlaylists: "x" }).userPlaylists).toEqual([]);
  });
});
