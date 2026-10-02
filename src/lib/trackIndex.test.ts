import { describe, expect, it } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import type { Track } from "@/types/track";
import { trackById, trackIndexById } from "./trackIndex";

const track = (id: string) => ({ id, lyrics: lyricDocument([]) }) as Track;

describe("trackIndexById", () => {
  it("按数组引用缓存，重复 id 取第一个（与 find 一致）", () => {
    const playlist = [track("a"), track("b"), track("a")];
    const index = trackIndexById(playlist);
    expect(index.get("a")).toBe(0);
    expect(index.get("b")).toBe(1);
    expect(trackIndexById(playlist)).toBe(index);
    expect(trackIndexById([...playlist])).not.toBe(index);
  });

  it("trackById 找不到或 id 为空时返回 undefined", () => {
    const playlist = [track("a")];
    expect(trackById(playlist, "a")?.id).toBe("a");
    expect(trackById(playlist, "missing")).toBeUndefined();
    expect(trackById(playlist, null)).toBeUndefined();
  });
});
