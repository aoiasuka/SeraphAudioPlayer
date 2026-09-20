// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { DEFAULT_AMLL_TTML_DB_URL } from "@/lib/lyrics/settings";
import type { Track } from "@/types/track";
import { CREDITS_EXCLUDE_PRESET_PATTERN, LyricsSettingsTab } from "./LyricsSettingsTab";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(async () => undefined),
  tauri: false,
}));
vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  isTauriRuntime: () => bridge.tauri,
  invoke: (command: string, args?: Record<string, unknown>) => bridge.invoke(command, args),
}));

describe("歌词设置标签页", () => {
  beforeEach(() => {
    bridge.tauri = false;
    bridge.invoke.mockReset();
    bridge.invoke.mockResolvedValue(undefined);
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      lyricsExcludeRules: [],
      amllTtmlDbUrl: DEFAULT_AMLL_TTML_DB_URL,
      amllTtmlDbCustom: false,
    });
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("五个设置项即时写入 store", () => {
    render(<LyricsSettingsTab />);
    fireEvent.change(screen.getByLabelText("歌词源优先级"), { target: { value: "kugou" } });
    expect(usePlayerStore.getState().lyricsSourcePriority).toBe("kugou");

    fireEvent.click(screen.getByRole("button", { name: "繁体中文" }));
    expect(usePlayerStore.getState().preferTraditionalLyrics).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "在线 TTML 歌词" }));
    expect(usePlayerStore.getState().ttmlLyricsEnabled).toBe(false);
  });

  it("AMLL 地址弹窗：预设模式固定 bikonoo 地址，自定义模式接受公网模板并拒绝内网", () => {
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[0]);
    const dialog = screen.getByRole("dialog");
    // 预设模式没有输入框，只展示固定地址
    expect(within(dialog).queryByLabelText("AMLL TTML DB 地址")).toBeNull();
    expect(within(dialog).getByText("https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml")).toBeInTheDocument();
    expect(within(dialog).getByText(/示例请求：https:\/\/amlldb\.bikonoo\.com\/ncm-lyrics\/\d+\.ttml/)).toBeInTheDocument();

    fireEvent.click(within(dialog).getByRole("radio", { name: "自定义地址" }));
    const input = within(dialog).getByLabelText("AMLL TTML DB 地址");
    fireEvent.change(input, { target: { value: "https://example.org/{dir}/{id}.ttml" } });
    expect(within(dialog).getByRole("button", { name: "保存" })).toBeEnabled();
    expect(within(dialog).getByText(/示例请求：https:\/\/example\.org\/ncm-lyrics\/\d+\.ttml/)).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "https://127.0.0.1/%s.ttml" } });
    expect(within(dialog).getByRole("button", { name: "保存" })).toBeDisabled();

    fireEvent.click(within(dialog).getByRole("button", { name: "amlldb.bikonoo.com（全平台）" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));
    expect(usePlayerStore.getState().amllTtmlDbUrl).toBe("https://amlldb.bikonoo.com/{dir}/{id}.ttml");
    expect(usePlayerStore.getState().amllTtmlDbCustom).toBe(true);
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("AMLL 地址弹窗：从自定义切回预设并保存即恢复固定地址", () => {
    usePlayerStore.setState({ amllTtmlDbUrl: "https://example.org/%s", amllTtmlDbCustom: true });
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[0]);
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByRole("radio", { name: "预设" }));
    fireEvent.click(within(dialog).getByRole("button", { name: "保存" }));
    expect(usePlayerStore.getState().amllTtmlDbUrl).toBe(DEFAULT_AMLL_TTML_DB_URL);
    expect(usePlayerStore.getState().amllTtmlDbCustom).toBe(false);
  });

  it("排除规则弹窗：添加关键词与正则、拒绝坏正则、删除规则", () => {
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[1]);
    const dialog = screen.getByRole("dialog");
    const input = within(dialog).getByLabelText("规则内容");

    fireEvent.change(input, { target: { value: "作词" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "添加规则" }));
    expect(usePlayerStore.getState().lyricsExcludeRules).toHaveLength(1);

    fireEvent.change(within(dialog).getByLabelText("规则类型"), { target: { value: "regex" } });
    fireEvent.change(input, { target: { value: "(" } });
    expect(within(dialog).getByRole("button", { name: "添加规则" })).toBeDisabled();
    expect(within(dialog).getByText(/正则无效/)).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "^作曲" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "添加规则" }));
    const rules = usePlayerStore.getState().lyricsExcludeRules;
    expect(rules.map((r) => [r.kind, r.pattern])).toEqual([
      ["keyword", "作词"],
      ["regex", "^作曲"],
    ]);

    fireEvent.click(within(dialog).getByRole("button", { name: "删除规则 作词" }));
    expect(usePlayerStore.getState().lyricsExcludeRules.map((r) => r.pattern)).toEqual(["^作曲"]);
  });

  it("排除规则弹窗：制作信息预设一键加入且不重复；命中预览按当前曲目 hidden 标记列出", () => {
    const hiddenLines = Array.from({ length: 32 }, (_, i) => ({ startMs: i * 1000, text: `作词：第${i}行`, hidden: true }));
    usePlayerStore.setState({
      playlist: [{ id: "t", duration: 180, lyrics: lyricDocument([{ startMs: 100000, text: "正文" }, ...hiddenLines]) }] as Track[],
      currentTrackIndex: 0,
    });
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[1]);
    const dialog = screen.getByRole("dialog");

    fireEvent.click(within(dialog).getByRole("button", { name: "制作信息预设" }));
    expect(usePlayerStore.getState().lyricsExcludeRules).toEqual([
      expect.objectContaining({ kind: "regex", pattern: CREDITS_EXCLUDE_PRESET_PATTERN }),
    ]);
    fireEvent.click(within(dialog).getByRole("button", { name: "制作信息预设" }));
    expect(usePlayerStore.getState().lyricsExcludeRules).toHaveLength(1);
    expect(usePlayerStore.getState().notification?.text).toContain("已存在");

    const preview = within(dialog).getByRole("list", { name: "被隐藏的歌词行" });
    expect(within(preview).getByText("作词：第0行")).toBeInTheDocument();
    expect(within(preview).getByText("作词：第29行")).toBeInTheDocument();
    expect(within(preview).queryByText("作词：第30行")).toBeNull();
    expect(within(preview).getByText("…还有 2 行")).toBeInTheDocument();
    expect(within(dialog).queryByText("正文")).toBeNull();
  });

  it("排除规则弹窗：无当前曲目 / 曲目无命中时的预览文案", () => {
    usePlayerStore.setState({ playlist: [], currentTrackIndex: 0 });
    const first = render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[1]);
    expect(within(screen.getByRole("dialog")).getByText("当前没有播放曲目")).toBeInTheDocument();
    first.unmount();

    usePlayerStore.setState({ playlist: [{ id: "t", duration: 180, lyrics: lyricDocument([{ startMs: 0, text: "正文" }]) }] as Track[] });
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[1]);
    expect(within(screen.getByRole("dialog")).getByText("当前曲目没有被隐藏的行")).toBeInTheDocument();
  });

  it("恢复歌词设置默认值：confirm 取消不动，确认后 9 个字段回默认并同步后端清空规则", () => {
    usePlayerStore.setState({
      lyricsSourcePriority: "kugou",
      preferTraditionalLyrics: true,
      ttmlLyricsEnabled: false,
      amllTtmlDbUrl: "https://example.org/%s.ttml",
      amllTtmlDbCustom: true,
      lyricsExcludeRules: [{ id: "k", kind: "keyword", pattern: "作词" }],
      showLyricsTranslation: false,
      showLyricsRoman: true,
      lyricsFolder: "D:/lyrics",
    });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<LyricsSettingsTab />);
    const button = screen.getByRole("button", { name: "恢复默认" });
    fireEvent.click(button);
    expect(usePlayerStore.getState().lyricsSourcePriority).toBe("kugou");

    confirmSpy.mockReturnValue(true);
    fireEvent.click(button);
    expect(usePlayerStore.getState()).toMatchObject({
      lyricsSourcePriority: "auto",
      preferTraditionalLyrics: false,
      ttmlLyricsEnabled: true,
      amllTtmlDbUrl: DEFAULT_AMLL_TTML_DB_URL,
      amllTtmlDbCustom: false,
      lyricsExcludeRules: [],
      showLyricsTranslation: true,
      showLyricsRoman: false,
      lyricsFolder: "",
    });
    expect(bridge.invoke).toHaveBeenCalledWith("set_lyrics_exclude_rules", { rules: [] });
    expect(usePlayerStore.getState().notification?.text).toBe("歌词设置已恢复默认");
  });

  it("AMLL 地址弹窗：非 Tauri 环境「测试连接」禁用", () => {
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[0]);
    expect(within(screen.getByRole("dialog")).getByRole("button", { name: "测试连接" })).toBeDisabled();
  });

  it("AMLL 地址弹窗：测试连接按 kind 显示中文结论与请求 URL，地址无效时禁用", async () => {
    bridge.tauri = true;
    bridge.invoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "test_amll_ttml_db") {
        return { ok: true, kind: "ok", message: "", url: `${String(args?.template).replace("%s", "123")}`, lines: 42 };
      }
      if (command === "validate_lyrics_exclude_rules") return [{ id: "probe", error: null }];
      return undefined;
    });
    render(<LyricsSettingsTab />);
    fireEvent.click(screen.getAllByRole("button", { name: "配置" })[0]);
    const dialog = screen.getByRole("dialog");
    const test = within(dialog).getByRole("button", { name: "测试连接" });
    expect(test).toBeEnabled();
    fireEvent.click(test);
    expect(bridge.invoke).toHaveBeenCalledWith("test_amll_ttml_db", { template: DEFAULT_AMLL_TTML_DB_URL, custom: false });
    expect(await within(dialog).findByText("连接正常，解析到 42 行")).toBeInTheDocument();
    expect(within(dialog).getByText(/请求：https:\/\/amlldb\.bikonoo\.com\/ncm-lyrics\/123\.ttml/)).toBeInTheDocument();

    // 切到自定义并输入非法地址 → 旧结论清除、按钮禁用
    fireEvent.click(within(dialog).getByRole("radio", { name: "自定义地址" }));
    fireEvent.change(within(dialog).getByLabelText("AMLL TTML DB 地址"), { target: { value: "https://127.0.0.1/%s.ttml" } });
    expect(within(dialog).queryByText("连接正常，解析到 42 行")).toBeNull();
    expect(within(dialog).getByRole("button", { name: "测试连接" })).toBeDisabled();

    // 合法自定义地址 → not_found 分类文案
    bridge.invoke.mockImplementation(async (command: string) => {
      if (command === "test_amll_ttml_db") return { ok: false, kind: "not_found", message: "404", url: "https://example.org/ncm-lyrics/123.ttml", lines: 0 };
      return undefined;
    });
    fireEvent.change(within(dialog).getByLabelText("AMLL TTML DB 地址"), { target: { value: "https://example.org/{dir}/{id}.ttml" } });
    fireEvent.click(within(dialog).getByRole("button", { name: "测试连接" }));
    expect(bridge.invoke).toHaveBeenLastCalledWith("test_amll_ttml_db", { template: "https://example.org/{dir}/{id}.ttml", custom: true });
    expect(await within(dialog).findByText("目标未收录（示例歌曲不存在，地址本身可达）")).toBeInTheDocument();
  });
});
