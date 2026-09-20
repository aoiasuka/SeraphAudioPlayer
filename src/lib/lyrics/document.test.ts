import { describe, expect, it } from "vitest";
import { describeLyricSource, emptyLyricDocument, hasLyrics, lyricDocument, lyricLines } from "./document";

describe("lyrics document helpers", () => {
  it("emptyLyricDocument / lyricDocument / lyricLines / hasLyrics", () => {
    expect(emptyLyricDocument().lines).toEqual([]);
    expect(emptyLyricDocument()).not.toBe(emptyLyricDocument());
    const doc = lyricDocument([{ startMs: 0, text: "a", words: [{ startMs: 0, text: "a" }] }], { kind: "manual", pinned: true });
    expect(doc.sync).toBe("word");
    expect(doc.source).toEqual({ kind: "manual", pinned: true });
    expect(lyricDocument([{ startMs: 0, text: "a" }]).sync).toBe("line");
    expect(lyricDocument([]).sync).toBe("none");
    expect(lyricLines({ lyrics: doc })).toBe(doc.lines);
    expect(lyricLines(null)).toEqual([]);
    expect(hasLyrics({ lyrics: doc })).toBe(true);
    expect(hasLyrics(undefined)).toBe(false);
  });

  it("describeLyricSource 给出中文来源说明，在线来源附平台名", () => {
    expect(describeLyricSource(undefined)).toBe("来源未知");
    expect(describeLyricSource({ kind: "manual" })).toBe("手动导入");
    expect(describeLyricSource({ kind: "online", provider: "netease" })).toBe("在线匹配（网易云音乐）");
    expect(describeLyricSource({ kind: "online", provider: "custom" })).toBe("在线匹配（custom）");
    expect(describeLyricSource({ kind: "ttml", provider: "amll" })).toBe("AMLL TTML");
    expect(describeLyricSource({ kind: "legacy" })).toBe("旧版曲库");
  });
});
