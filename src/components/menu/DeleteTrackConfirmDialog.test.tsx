// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DeleteTrackConfirmDialog } from "./DeleteTrackConfirmDialog";
import { useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { DeleteTracksResult, Track } from "@/types/track";

const initialState = usePlayerStore.getInitialState();
const local: Track = {
  id: "local", title: "本地曲目", artist: "测试", album: "测试", cover: "", format: "FLAC",
  bitdepth: "16-bit", bitrate: "", channels: "Stereo", size: "1 MB", path: "C:/test/local.flac",
  duration: 180, glowColor: "#fff", lyrics: [],
};
const stream: Track = { ...local, id: "stream", title: "流媒体曲目", album: "Bilibili" };
const deleteTracks = vi.fn<(ids: string[]) => Promise<DeleteTracksResult>>();

beforeEach(() => {
  deleteTracks.mockReset().mockResolvedValue({ deletedIds: ["local", "stream"], deletedFiles: 1, failures: [] });
  usePlayerStore.setState(initialState, true);
  usePlayerStore.setState({ playlist: [local, stream], deleteTracks });
  useContextMenuStore.getState().closeDeleteTrack();
});

afterEach(cleanup);

function openConfirmation() {
  useContextMenuStore.getState().requestDeleteTracks([local.id, stream.id], { scope: "我喜欢", all: true });
  return render(<DeleteTrackConfirmDialog />);
}

describe("删除确认", () => {
  it("明确范围和文件处理方式，取消不删除", async () => {
    openConfirmation();
    expect(screen.getByText("范围：我喜欢 · 2 首")).toBeInTheDocument();
    expect(screen.getByText(/流媒体 1 首.*永久删除本机缓存音频文件/)).toBeInTheDocument();
    expect(screen.getByText(/本地音乐 1 首.*保留原始音频文件/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "取消" }));
    expect(deleteTracks).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("确认期间导入的新曲目不会被全部删除包含", async () => {
    openConfirmation();
    act(() => usePlayerStore.setState({ playlist: [local, stream, { ...local, id: "new", title: "新曲目" }] }));
    await userEvent.click(screen.getByRole("button", { name: "确认删除 2 首" }));
    expect(deleteTracks).toHaveBeenCalledExactlyOnceWith(["local", "stream"]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("部分失败显示原因，重试仅包含失败曲目", async () => {
    deleteTracks.mockResolvedValueOnce({
      deletedIds: ["local"], deletedFiles: 0,
      failures: [{ id: "stream", title: "流媒体曲目", message: "文件被占用" }],
    });
    openConfirmation();
    await userEvent.click(screen.getByRole("button", { name: "确认删除 2 首" }));
    expect(screen.getByRole("alert")).toHaveTextContent("文件被占用");
    expect(screen.getByText("范围：我喜欢 · 1 首")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "重试 1 首" }));
    expect(deleteTracks).toHaveBeenNthCalledWith(2, ["stream"]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("删除中禁用重复提交和关闭，收到回执后关闭", async () => {
    let resolve!: (result: DeleteTracksResult) => void;
    deleteTracks.mockReturnValue(new Promise((done) => { resolve = done; }));
    openConfirmation();
    await userEvent.dblClick(screen.getByRole("button", { name: "确认删除 2 首" }));
    expect(deleteTracks).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "取消" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "删除中…" })).toBeDisabled();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    await act(async () => resolve({ deletedIds: ["local", "stream"], deletedFiles: 1, failures: [] }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});
