// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { usePlayerStore } from "@/store/player";
import { DEFAULT_AMLL_TTML_DB_URL } from "@/lib/lyrics/settings";
import { LyricsSettingsTab } from "./LyricsSettingsTab";

vi.mock("@/lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/lib/tauri")>()),
  isTauriRuntime: () => false,
  invoke: vi.fn(async () => undefined),
}));

describe("歌词设置标签页", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      ...usePlayerStore.getInitialState(),
      lyricsExcludeRules: [],
      amllTtmlDbUrl: DEFAULT_AMLL_TTML_DB_URL,
      amllTtmlDbCustom: false,
    });
  });
  afterEach(cleanup);

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
});
