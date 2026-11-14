import { describe, expect, it } from "vitest";
import type { Track } from "@/types/track";
import { groupAlbums } from "./albums";

const track = (id: string, patch: Partial<Track>) =>
  ({ id, title: id, artist: "A", album: "专辑", cover: "", format: "FLAC", ...patch }) as Track;

describe("groupAlbums（LIB5-01）", () => {
  it("合辑按专辑艺术家归为一张，而不是每位曲目艺术家一张", () => {
    const albums = groupAlbums([
      track("1", { artist: "甲", albumArtist: "群星" }),
      track("2", { artist: "乙 feat. 丙", albumArtist: "群星" }),
      track("3", { artist: "甲", album: "另一张" }),
    ]);
    expect(albums.map((album) => [album.artist, album.title, album.tracks.length])).toEqual([
      ["群星", "专辑", 2],
      ["甲", "另一张", 1],
    ]);
  });

  it("组内按碟号、音轨号排序；没有音轨号的排在后面并保持原顺序", () => {
    const [album] = groupAlbums([
      track("d2t1", { discNumber: 2, trackNumber: 1 }),
      track("none-a", {}),
      track("d1t2", { discNumber: 1, trackNumber: 2 }),
      track("t1", { trackNumber: 1 }),
      track("none-b", {}),
    ]);
    expect(album.tracks.map((item) => item.id)).toEqual(["t1", "d1t2", "none-a", "none-b", "d2t1"]);
  });

  it("整张专辑都没有音轨号时保持曲库顺序；年份与封面取组内第一个可用值", () => {
    const [album] = groupAlbums([
      track("b", {}),
      track("a", { albumYear: "1999", cover: "c.jpg" }),
    ]);
    expect(album.tracks.map((item) => item.id)).toEqual(["b", "a"]);
    expect(album).toMatchObject({ year: "1999", cover: "c.jpg" });
  });
});
