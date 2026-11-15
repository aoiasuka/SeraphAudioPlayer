// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  buildPersonalDataExport,
  mergePersonalData,
  parsePersonalDataImport,
} from "./personalData";
import {
  corruptBackupKey,
  createPlayerPersistStorage,
  hydrationGate,
  resetPersistProblemsForTest,
  subscribePersistProblems,
} from "./persistStorage";
import { migratePersistedPlayerState } from "@/store/player";

const playlist = (id: string, trackIds: string[]) => ({
  id,
  name: id,
  trackIds,
  createdAt: 1,
});

describe("FE2-01 个人数据导出 / 导入", () => {
  it("导出只带真收藏，导入能读回", () => {
    const text = buildPersonalDataExport({
      liked: { a: true, b: false },
      likedAt: { a: 10, b: 20 },
      userPlaylists: [playlist("p1", ["a"])],
    });
    const parsed = parsePersonalDataImport(text!);
    expect(parsed.liked).toEqual({ a: true });
    expect(parsed.likedAt).toEqual({ a: 10 });
    expect(parsed.userPlaylists).toEqual([playlist("p1", ["a"])]);
  });

  it("没有收藏也没有歌单时不导出", () => {
    expect(
      buildPersonalDataExport({ liked: { a: false }, likedAt: {}, userPlaylists: [] })
    ).toBeNull();
  });

  it("拒绝配置文件、坏 JSON 和空内容", () => {
    expect(() => parsePersonalDataImport("{")).toThrow("不是有效的 JSON");
    expect(() =>
      parsePersonalDataImport(JSON.stringify({ kind: "seraph-config", stores: {} }))
    ).toThrow("kind 不匹配");
    expect(() =>
      parsePersonalDataImport(JSON.stringify({ kind: "seraph-personal-data" }))
    ).toThrow("没有可导入");
    expect(() =>
      parsePersonalDataImport(
        JSON.stringify({ kind: "seraph-personal-data", version: 99, liked: { a: true } })
      )
    ).toThrow("版本较新");
  });

  it("畸形元素被消毒而不是带进 store", () => {
    const parsed = parsePersonalDataImport(
      JSON.stringify({
        kind: "seraph-personal-data",
        liked: { a: true, b: "yes" },
        likedAt: { a: "x" },
        userPlaylists: [null, { id: "" }, { id: "p", trackIds: ["a", 3] }],
      })
    );
    expect(parsed.liked).toEqual({ a: true });
    expect(parsed.likedAt).toEqual({});
    expect(parsed.userPlaylists).toEqual([
      { id: "p", name: "未命名歌单", trackIds: ["a"], createdAt: 0 },
    ]);
  });

  it("合并只追加：本机收藏、收藏时间和歌单顺序都保留", () => {
    const merged = mergePersonalData(
      {
        liked: { a: true },
        likedAt: { a: 1 },
        userPlaylists: [playlist("p1", ["a", "b"])],
      },
      {
        liked: { a: true, c: true },
        likedAt: { a: 99, c: 5 },
        userPlaylists: [playlist("p1", ["b", "c", "c"]), playlist("p2", ["x"])],
      }
    );
    expect(merged.liked).toEqual({ a: true, c: true });
    expect(merged.likedAt).toEqual({ a: 1, c: 5 });
    expect(merged.userPlaylists).toEqual([
      playlist("p1", ["a", "b", "c"]),
      playlist("p2", ["x"]),
    ]);
    expect(merged).toMatchObject({ addedLikes: 1, addedPlaylists: 1, mergedPlaylists: 1 });
  });
});

describe("FE2-01 持久化故障不再静默", () => {
  beforeEach(() => {
    localStorage.clear();
    resetPersistProblemsForTest();
    hydrationGate.ready = false;
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("存档解析失败时先另存原文再重置，并报告一次", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    localStorage.setItem("fe2-state", "{broken");
    const storage = createPlayerPersistStorage();
    expect(storage.getItem("fe2-state")).toBeNull();
    expect(localStorage.getItem("fe2-state")).toBeNull();
    expect(localStorage.getItem(corruptBackupKey("fe2-state"))).toBe("{broken");
    // 监听晚于水合挂上时补发
    const seen: string[] = [];
    subscribePersistProblems((problem) => seen.push(problem));
    expect(seen).toEqual(["corrupt-backed-up"]);
  });

  it("写入失败时报告一次，后续失败不重复", () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const seen: string[] = [];
    subscribePersistProblems((problem) => seen.push(problem));
    const storage = createPlayerPersistStorage();
    hydrationGate.ready = true;
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    const state = migratePersistedPlayerState({});
    storage.setItem("fe2-state", { state, version: 3 });
    vi.advanceTimersByTime(301);
    storage.setItem("fe2-state", { state: { ...state, volume: 0.1 }, version: 3 });
    vi.advanceTimersByTime(301);
    expect(seen).toEqual(["write-failed"]);
  });
});
