// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { forgetAllViews } from "@/lib/viewMemory";
import { useVirtualRows } from "./useVirtualRows";

function List({ count, stateKey }: { count: number; stateKey?: string }) {
  const rows = useVirtualRows(count, 50, { mountKey: count > 0, stateKey });
  return count ? (
    <>
      <button type="button" onClick={rows.scrollToTop}>top</button>
      <div ref={rows.scrollRef} onScroll={rows.onScroll} data-testid="scroll"><span>{rows.start}</span></div>
    </>
  ) : null;
}

it("scrollToTop 回到顶部并清掉记住的位置（UI3-02）", () => {
  const first = render(<List count={500} stateKey="tracks:top" />);
  const scroll = screen.getByTestId("scroll");
  // jsdom 没有实现 scrollTo
  scroll.scrollTo = ((options: ScrollToOptions) => {
    scroll.scrollTop = options.top ?? 0;
  }) as typeof scroll.scrollTo;
  fireEvent.scroll(scroll, { target: { scrollTop: 10000 } });
  expect(scroll.textContent).not.toBe("0");

  fireEvent.click(screen.getByRole("button", { name: "top" }));
  expect(scroll.textContent).toBe("0");
  expect(scroll.scrollTop).toBe(0);
  first.unmount();

  // 重建后不再恢复到之前的位置
  render(<List count={500} stateKey="tracks:top" />);
  expect(screen.getByTestId("scroll").textContent).toBe("0");
  expect(screen.getByTestId("scroll").scrollTop).toBe(0);
});

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
