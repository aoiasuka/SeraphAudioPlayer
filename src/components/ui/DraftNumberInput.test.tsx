// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { DraftNumberInput } from "./DraftNumberInput";

afterEach(cleanup);

it("输入过程中不逐键提交；失焦时整体提交（Q=0.707 不会在第一个 0 就被钳制）", async () => {
  const onCommit = vi.fn();
  render(<DraftNumberInput value={1} onCommit={onCommit} aria-label="Q 值" />);
  const input = screen.getByLabelText("Q 值");
  await userEvent.clear(input);
  await userEvent.type(input, "0.707");
  expect(onCommit).not.toHaveBeenCalled();
  expect(input).toHaveValue("0.707");
  await userEvent.tab();
  expect(onCommit).toHaveBeenCalledTimes(1);
  expect(onCommit).toHaveBeenCalledWith(0.707);
});

it("可以以负号开头输入，回车提交", async () => {
  const onCommit = vi.fn();
  render(<DraftNumberInput value={0} onCommit={onCommit} aria-label="增益" />);
  const input = screen.getByLabelText("增益");
  await userEvent.clear(input);
  await userEvent.type(input, "-6.5{Enter}");
  expect(onCommit).toHaveBeenCalledWith(-6.5);
});

it("清空或非法输入不提交，失焦后回到当前值；Esc 放弃草稿", async () => {
  const onCommit = vi.fn();
  const { rerender } = render(<DraftNumberInput value={1000} onCommit={onCommit} aria-label="频率" />);
  const input = screen.getByLabelText("频率");
  await userEvent.clear(input);
  await userEvent.tab();
  expect(onCommit).not.toHaveBeenCalled();
  expect(input).toHaveValue("1000");

  await userEvent.clear(input);
  await userEvent.type(input, "250{Escape}");
  expect(onCommit).not.toHaveBeenCalled();
  expect(input).toHaveValue("1000");

  await userEvent.clear(input);
  await userEvent.type(input, "12oops");
  await userEvent.tab();
  expect(onCommit).not.toHaveBeenCalled();
  expect(input).toHaveValue("1000");

  // 外部值变化（拖动滑块等）在非编辑态直接反映
  rerender(<DraftNumberInput value={2000} onCommit={onCommit} aria-label="频率" />);
  expect(input).toHaveValue("2000");
});
