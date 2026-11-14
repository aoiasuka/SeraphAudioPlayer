import type { Track } from "@/types/track";

export interface AlbumGroup {
  key: string;
  title: string;
  /** 专辑艺术家；标签里没有时是组内第一首的曲目艺术家 */
  artist: string;
  year?: string;
  format: string;
  cover: string;
  tracks: Track[];
}

/** 专辑归组用的艺术家：专辑艺术家优先，缺省退回曲目艺术家（LIB5-01）。 */
export function albumArtistOf(track: Track) {
  return track.albumArtist?.trim() || track.artist;
}

/**
 * 把曲目按「专辑艺术家 + 专辑名」分组，组内按碟号、音轨号排序。
 *
 * 此前按「曲目艺术家 + 专辑名」分组：合辑、原声带、带 feat. 的专辑每首歌的艺术家不同，
 * 一张专辑被拆成很多张；组内顺序是导入时的文件枚举顺序，多碟专辑和文件名不带曲号的
 * 专辑顺序都不对。没有音轨号的曲目排在有音轨号的之后，彼此保持曲库里的原始顺序。
 */
export function groupAlbums(tracks: readonly Track[]): AlbumGroup[] {
  const groups = new Map<string, AlbumGroup>();
  for (const track of tracks) {
    const artist = albumArtistOf(track);
    const key = `${artist}::${track.album}`;
    const group = groups.get(key);
    if (group) {
      group.tracks.push(track);
      // 专辑内部分曲目缺封面或年份时，用组内第一个可用值兜底
      if (!group.cover && track.cover) group.cover = track.cover;
      if (!group.year && track.albumYear) group.year = track.albumYear;
      continue;
    }
    groups.set(key, {
      key,
      title: track.album,
      artist,
      year: track.albumYear,
      format: track.format,
      cover: track.cover,
      tracks: [track],
    });
  }

  const albums = Array.from(groups.values());
  for (const album of albums) {
    if (!album.tracks.some((track) => track.trackNumber || track.discNumber)) continue;
    album.tracks = album.tracks
      .map((track, index) => ({ track, index }))
      .sort((a, b) =>
        (a.track.discNumber ?? 1) - (b.track.discNumber ?? 1) ||
        (a.track.trackNumber ?? Number.MAX_SAFE_INTEGER) - (b.track.trackNumber ?? Number.MAX_SAFE_INTEGER) ||
        a.index - b.index)
      .map(({ track }) => track);
  }
  return albums;
}
