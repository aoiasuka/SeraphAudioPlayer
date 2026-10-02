// @vitest-environment jsdom
import { fireEvent, render, screen, cleanup } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useVirtualRows } from "./useVirtualRows";

function List({ count }: { count: number }) {
  const rows = useVirtualRows(count, 50, { mountKey: count > 0 });
  return count ? <div ref={rows.scrollRef} onScroll={rows.onScroll} data-testid="scroll"><span>{rows.start}</span></div> : null;
}

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

it("容器卸载重挂后读取新的 DOM 滚动位置，不继承上一次的空白占位", () => {
  vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
  const { rerender } = render(<List count={500} />);
  fireEvent.scroll(screen.getByTestId("scroll"), { target: { scrollTop: 10000 } });
  expect(screen.getByTestId("scroll").textContent).not.toBe("0");
  rerender(<List count={0} />);
  rerender(<List count={500} />);
  expect(screen.getByTestId("scroll").textContent).toBe("0");
});
