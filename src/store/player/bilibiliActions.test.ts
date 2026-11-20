import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { invoke } from "@/lib/tauri";
import { migratePersistedPlayerState, usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { mergeBatchProgress } from "./bilibiliActions";
import type { BilibiliBatchImportResult } from "./types";

vi.mock("@/lib/tauri", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/tauri")>();
  return {
    ...actual,
    invoke: vi.fn(async () => undefined) as unknown as typeof actual.invoke,
  };
});

const invokeMock = invoke as unknown as Mock;
const initialState = usePlayerStore.getInitialState();
const BVID = "BV1xx411c7mD";
const OPTIONS = { preferFlac: false, preferDolbyAtmos: false, remuxWithFfmpeg: false };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** 后端给出的分 P 曲目：第 1 P 的来源 ID 是 BV 号，其余是 BV…-pN。 */
function pageTrack(page: number, overrides: Partial<Track> = {}): Track {
  return {
    id: `bilibili-${BVID.toLowerCase()}-${100 + page}`,
    title: `第 ${page} 首`,
    artist: "UP",
    album: "合集标题",
    trackNumber: page,
    cover: "",
    format: "M4A",
    bitdepth: "M4A Bilibili",
    sampleRate: "48 kHz",
    bitrate: "192 kbps",
    channels: "Stereo",
    size: "3.0 MB",
    path: `C:/cache/${BVID}-${100 + page}.m4a`,
    sourceUrl: `https://www.bilibili.com/video/${BVID}?p=${page}`,
    sourceId: page === 1 ? BVID : `${BVID}-p${page}`,
    cacheMissing: false,
    duration: 180,
    glowColor: "#fff",
    glow1: "#fff",
    glow2: "#000",
    lyrics: lyricDocument([]),
    ...overrides,
  };
}

function result(overrides: Partial<BilibiliBatchImportResult>): BilibiliBatchImportResult {
  return { tracks: [], failed: [], cancelled: false, ...overrides };
}

function lastCall(command: string) {
  return invokeMock.mock.calls.filter(([name]) => name === command).at(-1)?.[1] as
    | Record<string, unknown>
    | undefined;
}

beforeEach(() => {
  usePlayerStore.setState(initialState, true);
  invokeMock.mockReset();
  invokeMock.mockImplementation(async () => undefined);
});

describe("B 站分 P 导入（BILI-05）", () => {
  it("probeBilibiliVideo 返回视频概况；失败时提示并返回 null", async () => {
    const info = {
      bvid: BVID,
      title: "合集标题",
      requestedPage: null,
      pages: [
        { page: 1, title: "第一首", duration: 180 },
        { page: 2, title: "第二首", duration: 200 },
      ],
    };
    invokeMock.mockImplementation(async (command) =>
      command === "bilibili_video_info" ? info : undefined
    );
    await expect(usePlayerStore.getState().probeBilibiliVideo(`  ${BVID} `)).resolves.toEqual(info);
    expect(lastCall("bilibili_video_info")).toEqual({ input: BVID });

    invokeMock.mockImplementation(async () => {
      throw { code: "invalid_input", message: "没有找到有效的 B 站 BV 号或视频链接" };
    });
    await expect(usePlayerStore.getState().probeBilibiliVideo("乱写的")).resolves.toBeNull();
    expect(usePlayerStore.getState().notification?.text).toBe(
      "没有找到有效的 B 站 BV 号或视频链接"
    );

    // 空输入不联网
    invokeMock.mockClear();
    await expect(usePlayerStore.getState().probeBilibiliVideo("   ")).resolves.toBeNull();
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("只出一首：不进入批量状态，导入范围原样传给后端", async () => {
    const track = pageTrack(1, { title: "视频标题", album: "Bilibili", trackNumber: undefined });
    const pending = deferred<BilibiliBatchImportResult>();
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video" ? pending.promise : undefined
    );

    const importing = usePlayerStore
      .getState()
      .importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "first" });
    // 只出一首的导入没有进度事件，不能占着批量状态（收藏夹导入要照常可用）
    expect(usePlayerStore.getState().bilibiliBatchProgress).toBeNull();
    pending.resolve(result({ tracks: [track] }));
    await expect(importing).resolves.toBe(true);

    expect(lastCall("import_bilibili_video")).toMatchObject({
      input: BVID,
      options: { ...OPTIONS, multiPage: "first" },
    });
    const state = usePlayerStore.getState();
    expect(state.playlist.map((item) => item.id)).toEqual([track.id]);
    expect(state.activeView).toBe("streaming");
    expect(state.notification?.text).toBe("已添加 B 站音频: 视频标题");

    // 再导一次同一来源：原位更新，不重复
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video" ? result({ tracks: [{ ...track, format: "FLAC" }] }) : undefined
    );
    await usePlayerStore.getState().importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "first" });
    expect(usePlayerStore.getState().playlist).toHaveLength(1);
    expect(usePlayerStore.getState().playlist[0].format).toBe("FLAC");
    expect(usePlayerStore.getState().notification?.text).toBe("已更新 B 站音频: 视频标题");
  });

  it("全量导入：各分 P 各占一条记录，期间是可取消的批量状态，结束后清掉", async () => {
    const tracks = [pageTrack(1), pageTrack(2), pageTrack(3)];
    const pending = deferred<BilibiliBatchImportResult>();
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video" ? pending.promise : undefined
    );

    const importing = usePlayerStore
      .getState()
      .importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "all" });
    const progress = usePlayerStore.getState().bilibiliBatchProgress;
    expect(progress).toMatchObject({ kind: "video", current: 0, total: 0 });
    // 进度事件、取消命令都靠这个任务 ID 对上后端
    expect(lastCall("import_bilibili_video")?.taskId).toBe(progress?.taskId);

    // 批量期间不能再开一个（后端同一时刻只有一个批量槽位）
    await expect(
      usePlayerStore.getState().importBilibiliAudio("BV1yy411c7mE", { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(false);
    await expect(
      usePlayerStore.getState().importBilibiliFavorites("123", OPTIONS)
    ).resolves.toBeNull();
    expect(invokeMock.mock.calls.filter(([name]) => name === "import_bilibili_video")).toHaveLength(1);
    expect(usePlayerStore.getState().bilibiliBatchProgress?.taskId).toBe(progress?.taskId);

    await usePlayerStore.getState().cancelBilibiliFavoritesImport();
    expect(lastCall("cancel_bilibili_favorites_import")).toEqual({ taskId: progress?.taskId });
    expect(usePlayerStore.getState().notification?.text).toBe("正在取消分 P 导入…");

    pending.resolve(result({ tracks }));
    await expect(importing).resolves.toBe(true);

    const state = usePlayerStore.getState();
    expect(state.bilibiliBatchProgress).toBeNull();
    expect(state.playlist.map((item) => item.sourceId)).toEqual([
      BVID,
      `${BVID}-p2`,
      `${BVID}-p3`,
    ]);
    expect(state.notification?.text).toBe("分 P 导入完成：新增 3 首，更新 0 首，失败 0 首");
  });

  it("全量导入并入旧记录：此前只导了首 P 的那一条原位更新，收藏与歌单归属不丢", async () => {
    const legacy = pageTrack(1, {
      title: "视频标题",
      album: "Bilibili",
      trackNumber: undefined,
      sourceUrl: `https://www.bilibili.com/video/${BVID}`,
    });
    usePlayerStore.setState({
      playlist: [legacy],
      liked: { [legacy.id]: true },
      userPlaylists: [{ id: "pl-1", name: "歌单", trackIds: [legacy.id], createdAt: 1 }],
    });
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video"
        ? result({
            tracks: [pageTrack(1), pageTrack(2)],
            failed: [{ input: "合集标题 · P3", reason: "这个分 P 没有可用的音频流" }],
          })
        : undefined
    );

    await expect(
      usePlayerStore.getState().importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(true);

    const state = usePlayerStore.getState();
    expect(state.playlist.map((item) => item.id)).toEqual([legacy.id, pageTrack(2).id]);
    // 第 1 P 与旧记录是同一条：标题换成分 P 标题，id 不变
    expect(state.playlist[0].title).toBe("第 1 首");
    expect(state.liked[legacy.id]).toBe(true);
    expect(state.userPlaylists[0].trackIds).toEqual([legacy.id]);
    expect(state.notification?.text).toBe("分 P 导入完成：新增 1 首，更新 1 首，失败 1 首");
  });

  it("中途取消：已下好的照常入库；一首都没有时只提示已取消", async () => {
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video"
        ? result({ tracks: [pageTrack(1)], cancelled: true })
        : undefined
    );
    await expect(
      usePlayerStore.getState().importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(true);
    expect(usePlayerStore.getState().playlist).toHaveLength(1);
    expect(usePlayerStore.getState().notification?.text).toBe(
      "分 P 导入已取消：新增 1 首，更新 0 首，失败 0 首"
    );

    usePlayerStore.setState(initialState, true);
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video" ? result({ cancelled: true }) : undefined
    );
    await expect(
      usePlayerStore.getState().importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(false);
    expect(usePlayerStore.getState().notification?.text).toBe("导入已取消");
    expect(usePlayerStore.getState().bilibiliBatchProgress).toBeNull();
  });

  it("全部失败与后端报错：返回 false、给出原因，批量状态不残留", async () => {
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_video"
        ? result({ failed: [{ input: "合集标题 · P1", reason: "磁盘已满" }] })
        : undefined
    );
    await expect(
      usePlayerStore.getState().importBilibiliAudio(BVID, { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(false);
    expect(usePlayerStore.getState().notification).toMatchObject({
      text: "导入 B 站音频失败：磁盘已满",
      level: "error",
    });

    invokeMock.mockImplementation(async () => {
      throw { code: "invalid_input", message: "这个视频共 3 个分 P，没有第 9 P" };
    });
    await expect(
      usePlayerStore.getState().importBilibiliAudio(`${BVID}?p=9`, { ...OPTIONS, multiPage: "all" })
    ).resolves.toBe(false);
    expect(usePlayerStore.getState().notification?.text).toBe("这个视频共 3 个分 P，没有第 9 P");
    expect(usePlayerStore.getState().bilibiliBatchProgress).toBeNull();
  });

  it("收藏夹导入把全局的分 P 设置带给后端，状态标为收藏夹", async () => {
    const pending = deferred<BilibiliBatchImportResult>();
    invokeMock.mockImplementation(async (command) =>
      command === "import_bilibili_favorites" ? pending.promise : undefined
    );
    const importing = usePlayerStore
      .getState()
      .importBilibiliFavorites("123456", { ...OPTIONS, multiPage: "all" });
    expect(usePlayerStore.getState().bilibiliBatchProgress).toMatchObject({ kind: "favorites" });
    await usePlayerStore.getState().cancelBilibiliFavoritesImport();
    expect(usePlayerStore.getState().notification?.text).toBe("正在取消收藏夹导入…");
    pending.resolve(result({ tracks: [pageTrack(1), pageTrack(2)] }));
    await importing;
    expect(lastCall("import_bilibili_favorites")).toMatchObject({
      input: "123456",
      options: { multiPage: "all" },
    });
    expect(usePlayerStore.getState().playlist).toHaveLength(2);
  });

  it("进度事件只在导入中、任务一致时并入，并保留发起时记下的种类", () => {
    const current = { taskId: "t1", kind: "video" as const, current: 0, total: 0, title: "", ok: true };
    const event = { taskId: "t1", current: 2, total: 5, title: "合集标题 · P2 第二首", ok: true };
    expect(mergeBatchProgress(current, event)).toEqual({ ...event, kind: "video" });
    // 结果落定后滞后到达的事件、别的任务的事件：忽略
    expect(mergeBatchProgress(null, event)).toBeNull();
    expect(mergeBatchProgress(current, { ...event, taskId: "t2" })).toBeNull();
  });
});

describe("分 P 默认导入范围设置（BILI-05）", () => {
  it("默认只导入第 1 P；可切换；重复写入同一个值不触发更新", () => {
    expect(usePlayerStore.getState().bilibiliMultiPageMode).toBe("first");
    usePlayerStore.getState().setBilibiliMultiPageMode("all");
    expect(usePlayerStore.getState().bilibiliMultiPageMode).toBe("all");

    const listener = vi.fn();
    const unsubscribe = usePlayerStore.subscribe(listener);
    usePlayerStore.getState().setBilibiliMultiPageMode("all");
    unsubscribe();
    expect(listener).not.toHaveBeenCalled();
  });

  it("水合时清洗：旧存储没有这个字段、或值认不出，都回到只导入第 1 P", () => {
    expect(migratePersistedPlayerState({}).bilibiliMultiPageMode).toBe("first");
    expect(migratePersistedPlayerState({ bilibiliMultiPageMode: "all" }).bilibiliMultiPageMode).toBe(
      "all"
    );
    for (const bad of ["ALL", "everything", 1, true, null]) {
      expect(migratePersistedPlayerState({ bilibiliMultiPageMode: bad }).bilibiliMultiPageMode).toBe(
        "first"
      );
    }
  });
});
