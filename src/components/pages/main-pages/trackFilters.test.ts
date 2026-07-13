import { describe, expect, it } from "vitest";
import { filterAndSortTracks } from "./trackFilters";
import type { Track } from "@/types/track";

function makeTrack(overrides: Partial<Track> & { id: string }): Track {
  return {
    title: overrides.id,
    artist: "Artist",
    album: "Album",
    cover: "",
    format: "FLAC",
    bitdepth: "FLAC 24-bit / 96 kHz",
    bitrate: "Unknown",
    channels: "Stereo",
    size: "1 MB",
    path: `C:/Music/${overrides.id}.flac`,
    duration: 100,
    glowColor: "#fff",
    lyrics: [],
    ...overrides,
  } as Track;
}

const TRACKS: Track[] = [
  makeTrack({ id: "a", title: "夜曲", artist: "周杰伦", album: "十一月的萧邦", duration: 226 }),
  makeTrack({ id: "b", title: "First Love", artist: "宇多田ヒカル", album: "First Love", duration: 258 }),
  makeTrack({ id: "c", title: "Answer", artist: "幾田りら", album: "Sketch", duration: 190 }),
];

describe("filterAndSortTracks", () => {
  it("空查询默认排序时原样返回（不复制数组）", () => {
    expect(filterAndSortTracks(TRACKS, "", "default")).toBe(TRACKS);
  });

  it("按标题过滤，大小写与首尾空格不敏感", () => {
    expect(filterAndSortTracks(TRACKS, "  first LOVE ", "default")).toEqual([
      TRACKS[1],
    ]);
  });

  it("匹配艺术家与专辑字段", () => {
    expect(filterAndSortTracks(TRACKS, "周杰伦", "default")).toHaveLength(1);
    expect(filterAndSortTracks(TRACKS, "sketch", "default")).toHaveLength(1);
  });

  it("无匹配返回空数组", () => {
    expect(filterAndSortTracks(TRACKS, "不存在的关键词", "default")).toEqual([]);
  });

  it("按时长排序且不修改入参顺序", () => {
    const before = [...TRACKS];
    const sorted = filterAndSortTracks(TRACKS, "", "duration");
    expect(sorted.map((track) => track.id)).toEqual(["c", "a", "b"]);
    expect(TRACKS).toEqual(before);
  });

  it("按标题排序使用本地化比较", () => {
    const sorted = filterAndSortTracks(TRACKS, "", "title");
    expect(sorted.map((track) => track.title)).toEqual(
      [...TRACKS.map((track) => track.title)].sort((a, b) =>
        new Intl.Collator(undefined, { numeric: true, sensitivity: "base" }).compare(a, b)
      )
    );
  });

  it("过滤与排序可组合", () => {
    const tracks = [
      makeTrack({ id: "x", title: "Song B", artist: "Same", duration: 30 }),
      makeTrack({ id: "y", title: "Song A", artist: "Same", duration: 20 }),
      makeTrack({ id: "z", title: "Other", artist: "Same", duration: 10 }),
    ];
    const result = filterAndSortTracks(tracks, "song", "title");
    expect(result.map((track) => track.id)).toEqual(["y", "x"]);
  });
});
