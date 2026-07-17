import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  isSeparator,
  showContextMenu,
  useContextMenuStore,
} from "@/store/contextMenu";

function resetStore() {
  useContextMenuStore.setState({
    open: false,
    x: 0,
    y: 0,
    entries: [],
    infoTrackId: null,
    createPlaylistTrackIds: null,
    confirmDeleteTrackId: null,
  });
}

describe("context menu store (v0.4.3)", () => {
  beforeEach(resetStore);

  it("showContextMenu 阻断默认菜单并在事件坐标处打开", () => {
    const preventDefault = vi.fn();
    showContextMenu({ preventDefault, clientX: 120, clientY: 80 }, [
      { key: "a", label: "动作", onSelect: () => {} },
    ]);

    const state = useContextMenuStore.getState();
    expect(preventDefault).toHaveBeenCalledOnce();
    expect(state.open).toBe(true);
    expect(state.x).toBe(120);
    expect(state.y).toBe(80);
    expect(state.entries).toHaveLength(1);
  });

  it("条目为空时只屏蔽默认菜单、不弹自绘菜单", () => {
    const preventDefault = vi.fn();
    showContextMenu({ preventDefault, clientX: 0, clientY: 0 }, []);

    expect(preventDefault).toHaveBeenCalledOnce();
    expect(useContextMenuStore.getState().open).toBe(false);
  });

  it("closeContextMenu 收起菜单并清空条目", () => {
    const store = useContextMenuStore.getState();
    store.openContextMenu({ x: 5, y: 5 }, [{ key: "a", label: "动作" }]);
    store.closeContextMenu();

    const state = useContextMenuStore.getState();
    expect(state.open).toBe(false);
    expect(state.entries).toEqual([]);
  });

  it("打开曲目信息 / 删除确认 / 新建歌单弹窗时自动收起菜单", () => {
    const store = useContextMenuStore.getState();

    store.openContextMenu({ x: 1, y: 1 }, [{ key: "a", label: "动作" }]);
    store.openTrackInfo("track-1");
    expect(useContextMenuStore.getState().open).toBe(false);
    expect(useContextMenuStore.getState().infoTrackId).toBe("track-1");

    store.openContextMenu({ x: 1, y: 1 }, [{ key: "a", label: "动作" }]);
    store.requestDeleteTrack("track-2");
    expect(useContextMenuStore.getState().open).toBe(false);
    expect(useContextMenuStore.getState().confirmDeleteTrackId).toBe("track-2");

    store.openContextMenu({ x: 1, y: 1 }, [{ key: "a", label: "动作" }]);
    store.openCreatePlaylistWith(["t1", "t2"]);
    expect(useContextMenuStore.getState().open).toBe(false);
    expect(useContextMenuStore.getState().createPlaylistTrackIds).toEqual([
      "t1",
      "t2",
    ]);
  });

  it("isSeparator 正确区分分隔线与动作条目", () => {
    expect(isSeparator({ type: "separator", key: "sep" })).toBe(true);
    expect(isSeparator({ key: "a", label: "动作" })).toBe(false);
  });
});
