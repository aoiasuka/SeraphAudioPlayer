// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import type { BilibiliVideoInfo } from "@/store/player";
import { StreamingPage } from "./StreamingPage";

// 虚拟列表用 ResizeObserver 测容器高度；jsdom 没有
beforeAll(() => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    }
  );
  Element.prototype.scrollTo = () => {};
});

const BVID = "BV1xx411c7mD";

function videoInfo(pages: number, requestedPage: number | null = null): BilibiliVideoInfo {
  return {
    bvid: BVID,
    title: "合集标题",
    requestedPage,
    pages: Array.from({ length: pages }, (_, index) => ({
      page: index + 1,
      title: `第 ${index + 1} 首`,
      duration: 60 * (index + 1) + 5,
    })),
  };
}

const actions = {
  probeBilibiliVideo: vi.fn<(input: string) => Promise<BilibiliVideoInfo | null>>(),
  importBilibiliAudio: vi.fn<(input: string, options?: unknown) => Promise<boolean>>(),
  importBilibiliFavorites: vi.fn<(input: string, options?: unknown) => Promise<null>>(),
  cancelBilibiliFavoritesImport: vi.fn(async () => {}),
  refreshBilibiliState: vi.fn(async () => {}),
};

function submitSingle(input: string) {
  fireEvent.change(screen.getByPlaceholderText(/链接 \/ BV号/), { target: { value: input } });
  fireEvent.click(screen.getByRole("button", { name: /归档/ }));
}

describe("流媒体页 · 多 P 视频导入（BILI-05）", () => {
  beforeEach(() => {
    Object.values(actions).forEach((action) => action.mockReset());
    actions.importBilibiliAudio.mockResolvedValue(true);
    actions.importBilibiliFavorites.mockResolvedValue(null);
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      ...actions,
      bilibiliPreferFlac: true,
    });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("单 P 视频不询问，直接导入", async () => {
    actions.probeBilibiliVideo.mockResolvedValue(videoInfo(1));
    render(<StreamingPage />);
    submitSingle(BVID);

    await waitFor(() => expect(actions.importBilibiliAudio).toHaveBeenCalledTimes(1));
    expect(actions.probeBilibiliVideo).toHaveBeenCalledWith(BVID);
    expect(actions.importBilibiliAudio).toHaveBeenCalledWith(BVID, {
      preferFlac: true,
      preferDolbyAtmos: false,
      remuxWithFfmpeg: false,
      multiPage: "first",
    });
    expect(screen.queryByRole("dialog")).toBeNull();
    // 导入成功后清空输入框
    await waitFor(() => expect(screen.getByPlaceholderText(/链接 \/ BV号/)).toHaveValue(""));
  });

  it("链接指定了分 P 的多 P 视频不询问（后端只导入那一 P）", async () => {
    actions.probeBilibiliVideo.mockResolvedValue(videoInfo(4, 3));
    render(<StreamingPage />);
    const input = `https://www.bilibili.com/video/${BVID}?p=3`;
    submitSingle(input);

    await waitFor(() => expect(actions.importBilibiliAudio).toHaveBeenCalledTimes(1));
    expect(actions.importBilibiliAudio.mock.calls[0][0]).toBe(input);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("多 P 且没有指定分 P：弹窗询问，默认选中项跟随全局设置，按所选范围导入", async () => {
    usePlayerStore.setState({ bilibiliMultiPageMode: "all" });
    actions.probeBilibiliVideo.mockResolvedValue(videoInfo(7));
    render(<StreamingPage />);
    submitSingle(BVID);

    const dialog = await screen.findByRole("dialog", { name: "这个视频有 7 个分 P" });
    expect(actions.importBilibiliAudio).not.toHaveBeenCalled();
    expect(dialog).toHaveTextContent("合集标题");
    // 只列前 5 个分 P，其余报数量
    const list = within(dialog).getByRole("list", { name: "分 P 列表" });
    expect(within(list).getAllByRole("listitem")).toHaveLength(6);
    expect(list).toHaveTextContent("第 1 首");
    expect(list).toHaveTextContent("1:05");
    expect(list).toHaveTextContent("其余 2 个");
    expect(list).not.toHaveTextContent("第 6 首");

    const first = within(dialog).getByRole("radio", { name: /仅导入首 P/ });
    const all = within(dialog).getByRole("radio", { name: /全量导入/ });
    expect(all).toBeChecked();
    expect(first).not.toBeChecked();

    // 这一次改选「仅导入首 P」
    fireEvent.click(first);
    fireEvent.click(within(dialog).getByRole("button", { name: "导入" }));

    await waitFor(() => expect(actions.importBilibiliAudio).toHaveBeenCalledTimes(1));
    expect(actions.importBilibiliAudio).toHaveBeenCalledWith(
      BVID,
      expect.objectContaining({ multiPage: "first", preferFlac: true })
    );
    expect(screen.queryByRole("dialog")).toBeNull();
    // 弹窗里的临时选择不改全局设置
    expect(usePlayerStore.getState().bilibiliMultiPageMode).toBe("all");
  });

  it("全局设置为只导入首 P 时弹窗默认选中首 P；选择全量导入则传 all", async () => {
    actions.probeBilibiliVideo.mockResolvedValue(videoInfo(3));
    render(<StreamingPage />);
    submitSingle(BVID);

    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("radio", { name: /仅导入首 P/ })).toBeChecked();
    fireEvent.click(within(dialog).getByRole("radio", { name: /全量导入/ }));
    fireEvent.click(within(dialog).getByRole("button", { name: "导入" }));

    await waitFor(() =>
      expect(actions.importBilibiliAudio).toHaveBeenCalledWith(
        BVID,
        expect.objectContaining({ multiPage: "all" })
      )
    );
  });

  it("在弹窗里取消：不导入，输入框内容保留", async () => {
    actions.probeBilibiliVideo.mockResolvedValue(videoInfo(3));
    render(<StreamingPage />);
    submitSingle(BVID);

    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "取消" }));

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(actions.importBilibiliAudio).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText(/链接 \/ BV号/)).toHaveValue(BVID);
    // 可以再次提交
    await waitFor(() => expect(screen.getByRole("button", { name: /归档/ })).toBeEnabled());
  });

  it("探测失败（链接无效等）不导入，输入框内容保留", async () => {
    actions.probeBilibiliVideo.mockResolvedValue(null);
    render(<StreamingPage />);
    submitSingle("乱写的");

    await waitFor(() => expect(actions.probeBilibiliVideo).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByRole("button", { name: /归档/ })).toBeEnabled());
    expect(actions.importBilibiliAudio).not.toHaveBeenCalled();
    expect(screen.getByPlaceholderText(/链接 \/ BV号/)).toHaveValue("乱写的");
  });

  it("收藏夹批量导入不询问，直接带上全局设置", async () => {
    usePlayerStore.setState({ bilibiliMultiPageMode: "all" });
    render(<StreamingPage />);
    fireEvent.change(screen.getByPlaceholderText(/收藏夹链接/), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "批量" }));

    await waitFor(() => expect(actions.importBilibiliFavorites).toHaveBeenCalledTimes(1));
    expect(actions.importBilibiliFavorites).toHaveBeenCalledWith(
      "123456",
      expect.objectContaining({ multiPage: "all", preferFlac: true })
    );
    expect(actions.probeBilibiliVideo).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("分 P 全量导入的进度与取消显示在单链接一栏，收藏夹一栏不受影响", () => {
    usePlayerStore.setState({
      bilibiliBatchProgress: {
        taskId: "t1",
        kind: "video",
        current: 2,
        total: 7,
        title: "合集标题 · P2 第 2 首",
        ok: true,
      },
    });
    render(<StreamingPage />);

    expect(screen.getByRole("button", { name: "2/7" })).toBeDisabled();
    expect(screen.getByPlaceholderText(/链接 \/ BV号/)).toBeDisabled();
    // 收藏夹一栏仍显示「批量」，没有它自己的进度
    expect(screen.getByRole("button", { name: "批量" })).toBeInTheDocument();
    const cancelButtons = screen.getAllByRole("button", { name: "取消" });
    expect(cancelButtons).toHaveLength(1);
    fireEvent.click(cancelButtons[0]);
    expect(actions.cancelBilibiliFavoritesImport).toHaveBeenCalledTimes(1);
  });

  it("收藏夹导入的进度显示在收藏夹一栏，单链接一栏不显示进度", () => {
    usePlayerStore.setState({
      bilibiliBatchProgress: {
        taskId: "t1",
        kind: "favorites",
        current: 3,
        total: 20,
        title: "某个视频",
        ok: true,
      },
    });
    render(<StreamingPage />);

    expect(screen.getByRole("button", { name: "3/20" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /归档/ })).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "取消" })).toHaveLength(1);
  });
});
