// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { StrictMode, useState } from "react";
import { createPortal } from "react-dom";
import { Dialog } from "./dialog";

afterEach(cleanup);

it("自动关联自己的标题，不使用嵌套弹窗标题", () => {
  render(<Dialog open onClose={() => {}}><h2>外层标题</h2><Dialog open onClose={() => {}}><h2>内层标题</h2></Dialog></Dialog>);
  expect(screen.getByRole("dialog", { name: "外层标题" })).toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "内层标题" })).toBeInTheDocument();
});

it("支持显式可访问名称和标题引用，并保留已有标题 ID", () => {
  const { rerender } = render(<Dialog open onClose={() => {}} aria-label="显式名称"><h2 id="existing-title">标题</h2></Dialog>);
  expect(screen.getByRole("dialog", { name: "显式名称" })).toBeInTheDocument();
  rerender(<><h2 id="outside-title">外部标题</h2><Dialog open onClose={() => {}} aria-labelledby="outside-title"><h2 id="existing-title">标题</h2></Dialog></>);
  expect(screen.getByRole("dialog", { name: "外部标题" })).toBeInTheDocument();
  expect(screen.getByText("标题")).toHaveAttribute("id", "existing-title");
});

it("关闭和卸载后返回触发按钮，portal 不改变行为", () => {
  function Example() {
    const [open, setOpen] = useState(false);
    return <><button onClick={() => setOpen(true)}>打开</button>{createPortal(<Dialog open={open} onClose={() => setOpen(false)}><h2>弹窗</h2></Dialog>, document.body)}</>;
  }
  render(<Example />);
  const trigger = screen.getByRole("button", { name: "打开" });
  trigger.focus();
  fireEvent.click(trigger);
  expect(screen.getByRole("dialog")).toHaveFocus();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(trigger).toHaveFocus();
});

it("父组件内联 onClose 更新不抢走子弹窗栈顶且使用最新回调", () => {
  const parentClose = vi.fn();
  const childClose = vi.fn();
  const { rerender } = render(<><Dialog open onClose={() => parentClose("旧")}><h2>父</h2></Dialog><Dialog open onClose={childClose}><h2>子</h2></Dialog></>);
  rerender(<><Dialog open onClose={() => parentClose("新")}><h2>父</h2></Dialog><Dialog open onClose={childClose}><h2>子</h2></Dialog></>);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(childClose).toHaveBeenCalledTimes(1);
  expect(parentClose).not.toHaveBeenCalled();
  rerender(<><Dialog open onClose={() => parentClose("新")}><h2>父</h2></Dialog><Dialog open={false} onClose={childClose}><h2>子</h2></Dialog></>);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(parentClose).toHaveBeenCalledExactlyOnceWith("新");
});

it("真正嵌套弹窗一次 Escape 只关子层并归还焦点", () => {
  function Example() {
    const [parent, setParent] = useState(true);
    const [child, setChild] = useState(false);
    return <Dialog open={parent} onClose={() => setParent(false)}><h2>父</h2><button onClick={() => setChild(true)}>打开子层</button><Dialog open={child} onClose={() => setChild(false)}><h2>子</h2><button>子按钮</button></Dialog></Dialog>;
  }
  render(<Example />);
  const trigger = screen.getByRole("button", { name: "打开子层" });
  trigger.focus();
  fireEvent.click(trigger);
  fireEvent.keyDown(window, { key: "Escape" });
  expect(screen.queryByRole("dialog", { name: "子" })).not.toBeInTheDocument();
  expect(screen.getByRole("dialog", { name: "父" })).toBeInTheDocument();
  expect(trigger).toHaveFocus();
});

it("同时挂载嵌套弹窗只关闭子层，整个树卸载返回原始触发元素", () => {
  const trigger = document.createElement("button");
  document.body.append(trigger);
  trigger.focus();
  const parentClose = vi.fn();
  const childClose = vi.fn();
  const { unmount } = render(<StrictMode><Dialog open onClose={parentClose}><h2>父</h2><Dialog open onClose={childClose}><h2>子</h2></Dialog></Dialog></StrictMode>);
  expect(screen.getByRole("dialog", { name: "子" })).toHaveFocus();
  fireEvent.keyDown(window, { key: "Escape" });
  expect(childClose).toHaveBeenCalledTimes(1);
  expect(parentClose).not.toHaveBeenCalled();
  unmount();
  expect(trigger).toHaveFocus();
  trigger.remove();
});

it("从容器 Shift+Tab 到最后控件，Tab 到第一控件；遮罩拖选不关闭", () => {
  const close = vi.fn();
  render(<Dialog open onClose={close}><h2>标题</h2><button>首</button><button>末</button></Dialog>);
  const dialog = screen.getByRole("dialog");
  fireEvent.keyDown(dialog, { key: "Tab", shiftKey: true });
  expect(screen.getByRole("button", { name: "末" })).toHaveFocus();
  fireEvent.keyDown(document.activeElement!, { key: "Tab" });
  expect(screen.getByRole("button", { name: "首" })).toHaveFocus();
  fireEvent.pointerDown(dialog);
  fireEvent.click(dialog.parentElement!);
  expect(close).not.toHaveBeenCalled();
  fireEvent.pointerDown(dialog.parentElement!);
  fireEvent.click(dialog.parentElement!);
  expect(close).toHaveBeenCalledTimes(1);
});
