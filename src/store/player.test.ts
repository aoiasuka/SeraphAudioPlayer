import { beforeEach, describe, expect, it } from "vitest";
import { usePlayerStore } from "@/store/player";

describe("player store output driver", () => {
  beforeEach(() => {
    usePlayerStore.setState({
      driverKind: "direct",
      notification: null,
    });
  });

  it("blocks the unfinished ASIO backend", () => {
    usePlayerStore.getState().setDriver("asio");

    const state = usePlayerStore.getState();
    expect(state.driverKind).toBe("direct");
    expect(state.notification?.text).toContain("ASIO 输出尚未开放");
  });

  it("allows implemented output drivers", () => {
    usePlayerStore.getState().setDriver("wasapi");

    expect(usePlayerStore.getState().driverKind).toBe("wasapi");
  });
});
