// @vitest-environment jsdom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { VirtualGrid } from "./VirtualGrid";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

it("五千张卡片只挂载可见行，键盘可到达末项，缩放后重新计算列数", () => {
  let columns = 2;
  const resizeCallbacks: (() => void)[] = [];
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resizeCallbacks.push(callback); }
    observe() {}
    disconnect() {}
  });
  vi.stubGlobal("getComputedStyle", () => ({ gridTemplateColumns: Array(columns).fill("160px").join(" "), getPropertyValue: () => "" }));
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({ height: 120 } as DOMRect);
  const items = Array.from({ length: 5000 }, (_, index) => ({ id: String(index) }));
  render(<VirtualGrid items={items} itemKey={(item) => item.id} label="cards" renderItem={(item) => <button>Card {item.id}</button>} />);
  expect(screen.getAllByRole("listitem").length).toBeLessThan(30);
  expect(screen.getByRole("button", { name: "Card 0" })).toBeTruthy();
  fireEvent.keyDown(screen.getByRole("button", { name: "Card 0" }), { key: "End" });
  expect(document.activeElement?.textContent).toBe("Card 4999");
  expect(screen.getAllByRole("listitem").length).toBeLessThan(30);
  columns = 5;
  act(() => { resizeCallbacks.forEach((callback) => callback()); });
  fireEvent.keyDown(screen.getByRole("button", { name: "Card 4999" }), { key: "Home" });
  expect(document.activeElement?.textContent).toBe("Card 0");
  expect(screen.getAllByRole("listitem").length).toBeLessThan(45);
});
