import { describe, expect, it } from "vitest";
import { lyricDocument } from "@/lib/lyrics/document";
import type { Track } from "@/types/track";
import { filterArtistGroups, groupTracksByArtist, splitArtistNames } from "./artists";

function track(id: string, artist: string): Track {
  return {
    id, title: id, artist, album: "Album", cover: "", format: "FLAC", bitdepth: "",
    bitrate: "", channels: "", size: "", path: `C:/m/${id}.flac`, duration: 1,
    glowColor: "", lyrics: lyricDocument([]),
  } as Track;
}

describe("splitArtistNames", () => {
  it("按斜杠、顿号、分号与 feat. 标记拆分", () => {
    expect(splitArtistNames("周杰伦/方文山")).toEqual(["周杰伦", "方文山"]);
    expect(splitArtistNames("A 、 B；C")).toEqual(["A", "B", "C"]);
    expect(splitArtistNames("Artist A feat. Artist B")).toEqual(["Artist A", "Artist B"]);
    expect(splitArtistNames("Artist A (ft. Artist B)")).toEqual(["Artist A", "Artist B"]);
    expect(splitArtistNames("Artist A（Featuring Artist B / Artist C）"))
      .toEqual(["Artist A", "Artist B", "Artist C"]);
  });

  it("不拆 & 和逗号，也不拆 AC/DC 这类名字", () => {
    expect(splitArtistNames("Simon & Garfunkel")).toEqual(["Simon & Garfunkel"]);
    expect(splitArtistNames("Earth, Wind & Fire")).toEqual(["Earth, Wind & Fire"]);
    expect(splitArtistNames("AC/DC")).toEqual(["AC/DC"]);
    expect(splitArtistNames("AC/DC / Guest")).toEqual(["AC/DC", "Guest"]);
  });

  it("去掉重复与空段；名字里含 feat 字样的单词不受影响", () => {
    expect(splitArtistNames("A / a /  / B")).toEqual(["A", "B"]);
    expect(splitArtistNames("Feather")).toEqual(["Feather"]);
    expect(splitArtistNames("  ")).toEqual([""]);
  });
});

describe("groupTracksByArtist", () => {
  it("合作曲目归到每位艺术家名下，大小写与空白差异合并，结果按名称排序", () => {
    const groups = groupTracksByArtist([
      track("1", "Zed"),
      track("2", "alpha / Zed"),
      track("3", "Alpha"),
      track("4", "  ALPHA  feat. Beta"),
    ]);
    expect(groups.map((group) => group.name)).toEqual(["alpha", "Beta", "Zed"]);
    expect(groups[0].tracks.map((item) => item.id)).toEqual(["2", "3", "4"]);
    expect(groups[2].tracks.map((item) => item.id)).toEqual(["1", "2"]);
  });

  it("同一首歌在同一位艺术家名下只出现一次", () => {
    const groups = groupTracksByArtist([track("1", "A / a")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].tracks).toHaveLength(1);
  });
});

it("filterArtistGroups 不区分大小写，空词返回原引用", () => {
  const groups = groupTracksByArtist([track("1", "Alpha"), track("2", "Beta")]);
  expect(filterArtistGroups(groups, "  ")).toBe(groups);
  expect(filterArtistGroups(groups, "ALP").map((group) => group.name)).toEqual(["Alpha"]);
});
