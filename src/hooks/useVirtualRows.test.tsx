// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { forgetAllViews } from "@/lib/viewMemory";
import { useVirtualRows } from "./useVirtualRows";

function List({ count, stateKey }: { count: number; stateKey?: string }) {
  const rows = useVirtualRows(count, 50, { mountKey: count > 0, stateKey });
  return count ? <div ref={rows.scrollRef} onScroll={rows.onScroll} data-testid="scroll"><span>{rows.start}</span></div> : null;
}

beforeEach(() => {
  forgetAllViews();
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
});

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it("容器卸载重挂后读取新的 DOM 滚动位置，不继承上一次的空白占位", () => {
  const { rerender } = render(<List count={500} />);
  fireEvent.scroll(screen.getByTestId("scroll"), { target: { scrollTop: 10000 } });
  expect(screen.getByTestId("scroll").textContent).not.toBe("0");
  rerender(<List count={0} />);
  rerender(<List count={500} />);
  expect(screen.getByTestId("scroll").textContent).toBe("0");
});

it("带 stateKey 时，整个组件卸载重建后恢复到离开时的滚动位置", () => {
  const first = render(<List count={500} stateKey="playlist:p1" />);
  fireEvent.scroll(screen.getByTestId("scroll"), { target: { scrollTop: 10000 } });
  const startBefore = screen.getByTestId("scroll").textContent;
  expect(startBefore).not.toBe("0");
  first.unmount();

  render(<List count={500} stateKey="playlist:p1" />);
  expect(screen.getByTestId("scroll").textContent).toBe(startBefore);
  expect(screen.getByTestId("scroll").scrollTop).toBe(10000);
  cleanup();

  // 别的键不受影响
  render(<List count={500} stateKey="playlist:p2" />);
  expect(screen.getByTestId("scroll").textContent).toBe("0");
});

it("带 stateKey 时，同一次挂载内容器重挂仍按真实 DOM 位置走", () => {
  const { rerender } = render(<List count={500} stateKey="playlist:p1" />);
  fireEvent.scroll(screen.getByTestId("scroll"), { target: { scrollTop: 10000 } });
  rerender(<List count={0} stateKey="playlist:p1" />);
  rerender(<List count={500} stateKey="playlist:p1" />);
  expect(screen.getByTestId("scroll").textContent).toBe("0");
});
