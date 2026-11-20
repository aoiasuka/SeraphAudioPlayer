// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { usePlayerStore } from "@/store/player";
import { StreamingSettingsTab } from "./StreamingSettingsTab";

describe("设置页 · 流媒体（BILI-05）", () => {
  beforeEach(() => {
    usePlayerStore.setState(usePlayerStore.getInitialState(), true);
  });
  afterEach(cleanup);

  it("无特定分 P 时的默认导入范围：默认只导入第 1 P，点选即时写入 store", () => {
    render(<StreamingSettingsTab />);
    const group = screen.getByRole("radiogroup", { name: "无特定分 P 时的默认导入范围" });
    const first = screen.getByRole("radio", { name: "仅导入第 1 P" });
    const all = screen.getByRole("radio", { name: "导入全部 P" });
    expect(group).toContainElement(first);
    expect(first).toHaveAttribute("aria-checked", "true");
    expect(all).toHaveAttribute("aria-checked", "false");

    fireEvent.click(all);
    expect(usePlayerStore.getState().bilibiliMultiPageMode).toBe("all");
    expect(all).toHaveAttribute("aria-checked", "true");
    expect(first).toHaveAttribute("aria-checked", "false");

    fireEvent.click(first);
    expect(usePlayerStore.getState().bilibiliMultiPageMode).toBe("first");
  });

  it("说明里写清三种情况：单链接先询问、收藏夹跟随设置、带 ?p= 只导入那一 P", () => {
    render(<StreamingSettingsTab />);
    const row = screen.getByText("多 P 视频 · 无特定分 P 时").parentElement;
    expect(row).toHaveTextContent("粘贴单个链接会先询问");
    expect(row).toHaveTextContent("收藏夹批量导入不询问");
    expect(row).toHaveTextContent("?p=");
  });
});
