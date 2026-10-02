// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { ContextMenuLayer } from "./ContextMenuLayer";
import { useContextMenuStore } from "@/store/contextMenu";

vi.mock("./CreatePlaylistWithTracksDialog", () => ({ CreatePlaylistWithTracksDialog: () => null }));
vi.mock("./DeleteTrackConfirmDialog", () => ({ DeleteTrackConfirmDialog: () => null }));
vi.mock("./ReloadStreamingDialog", () => ({ ReloadStreamingDialog: () => null }));
vi.mock("./TrackInfoDialog", () => ({ TrackInfoDialog: () => null }));
afterEach(() => { cleanup(); useContextMenuStore.getState().closeContextMenu(); });

function openMenu() {
  const select = vi.fn();
  render(<ContextMenuLayer />);
  act(() => useContextMenuStore.getState().openContextMenu({ x: 20, y: 20 }, [
    { key: "first", label: "首项", onSelect: select },
    { key: "disabled", label: "禁用", disabled: true },
    { key: "sep", type: "separator" },
    { key: "parent", label: "导出", children: [
      { key: "disabled-child", label: "不可用", disabled: true },
      { key: "lrc", label: "逐行", onSelect: select },
      { key: "word", label: "逐字", children: [{ key: "enhanced", label: "增强", onSelect: select }] },
    ] },
    { key: "last", label: "末项", onSelect: select },
  ]));
  return select;
}

it("触控点击父项打开子菜单并提供 menu/menuitem/expanded 语义", () => {
  const select = openMenu();
  const parent = screen.getByRole("menuitem", { name: "导出" });
  expect(parent).toHaveAttribute("aria-haspopup", "menu");
  expect(parent).toHaveAttribute("aria-expanded", "false");
  fireEvent.pointerDown(parent, { pointerType: "touch" });
  fireEvent.click(parent);
  expect(parent).toHaveAttribute("aria-expanded", "true");
  const submenu = screen.getByRole("menu", { name: "导出" });
  expect(submenu.id).toBe(parent.getAttribute("aria-controls"));
  fireEvent.click(within(submenu).getByRole("menuitem", { name: "逐行" }));
  expect(select).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

it("方向键跳过禁用和分隔项，Home/End 只在当前层循环", () => {
  openMenu();
  expect(screen.getByRole("menuitem", { name: "首项" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
  expect(screen.getByRole("menuitem", { name: "导出" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "End" });
  expect(screen.getByRole("menuitem", { name: "末项" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
  expect(screen.getByRole("menuitem", { name: "首项" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowUp" });
  expect(screen.getByRole("menuitem", { name: "末项" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "Home" });
  expect(screen.getByRole("menuitem", { name: "首项" })).toHaveFocus();
});

it("Right 进入多级子菜单，Left/Escape 逐层退回而不关闭根菜单", () => {
  openMenu();
  screen.getByRole("menuitem", { name: "导出" }).focus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
  expect(screen.getByRole("menuitem", { name: "逐行" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "End" });
  expect(screen.getByRole("menuitem", { name: "逐字" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
  expect(screen.getByRole("menuitem", { name: "增强" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  expect(screen.getByRole("menuitem", { name: "逐字" })).toHaveFocus();
  expect(screen.queryByRole("menuitem", { name: "增强" })).not.toBeInTheDocument();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowLeft" });
  expect(screen.getByRole("menuitem", { name: "导出" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

it.each(["{Enter}", " "])("键盘 %s 激活父项和子项", async (key) => {
  const user = userEvent.setup();
  const select = openMenu();
  screen.getByRole("menuitem", { name: "导出" }).focus();
  await user.keyboard(key);
  expect(screen.getByRole("menuitem", { name: "逐行" })).toHaveFocus();
  await user.keyboard(key);
  expect(select).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("menu")).not.toBeInTheDocument();
});

it("全禁用子菜单仍能聚焦容器并通过 Escape 返回", () => {
  render(<ContextMenuLayer />);
  act(() => useContextMenuStore.getState().openContextMenu({ x: 0, y: 0 }, [
    { key: "parent", label: "空操作", children: [{ key: "disabled", label: "不可用", disabled: true }] },
  ]));
  fireEvent.keyDown(document.activeElement!, { key: "ArrowRight" });
  expect(screen.getByRole("menu", { name: "空操作" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "ArrowDown" });
  fireEvent.keyDown(document.activeElement!, { key: "Escape" });
  expect(screen.getByRole("menuitem", { name: "空操作" })).toHaveFocus();
});

it("鼠标移出不移除仍持有键盘焦点的子菜单", () => {
  openMenu();
  const parent = screen.getByRole("menuitem", { name: "导出" });
  fireEvent.click(parent);
  screen.getByRole("menuitem", { name: "逐行" }).focus();
  fireEvent.pointerLeave(parent.parentElement!);
  expect(screen.getByRole("menuitem", { name: "逐行" })).toHaveFocus();
});
