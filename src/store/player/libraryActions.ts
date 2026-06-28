import { invoke } from "@/lib/tauri";
import type { Track } from "@/types/track";
import { sendCommand } from "./commands";
import { resetNextIndexCache } from "./playbackActions";
import { normalizePath, streamingSourceInput, trackMergeKey } from "./trackIdentity";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

function dedupeTracks(tracks: Track[]) {
  const byKey = new Map<string, Track>();
  const orderedKeys: string[] = [];

  for (const track of tracks) {
    const key = trackMergeKey(track);
    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, track);
      orderedKeys.push(key);
      continue;
    }

    const preferred =
      existing.cacheMissing && !track.cacheMissing
        ? mergeIncomingTrack(existing, track)
        : mergeIncomingTrack(track, existing);
    byKey.set(key, preferred);
  }

  return orderedKeys.map((key) => byKey.get(key)).filter((track): track is Track => !!track);
}

function dedupeTracksWithLiked(tracks: Track[], liked: Record<string, boolean>) {
  // L-14: 一次迭代里同时算 dedupe + liked carry-over，避免对大曲库重复遍历。
  const byKey = new Map<string, Track>();
  const orderedKeys: string[] = [];
  const likedByKey = new Set<string>();

  for (const track of tracks) {
    const key = trackMergeKey(track);
    if (liked[track.id]) likedByKey.add(key);

    const existing = byKey.get(key);
    if (!existing) {
      byKey.set(key, track);
      orderedKeys.push(key);
      continue;
    }

    const preferred =
      existing.cacheMissing && !track.cacheMissing
        ? mergeIncomingTrack(existing, track)
        : mergeIncomingTrack(track, existing);
    byKey.set(key, preferred);
  }

  const playlist = orderedKeys
    .map((key) => byKey.get(key))
    .filter((track): track is Track => !!track);

  const nextLiked: Record<string, boolean> = {};
  for (const track of playlist) {
    if (liked[track.id] || likedByKey.has(trackMergeKey(track))) {
      nextLiked[track.id] = true;
    }
  }

  return { playlist, liked: nextLiked };
}

function mergeTracksByPath(existing: Track[], incoming: Track[]) {
  const remaining = new Map<string, Track>();
  for (const track of incoming) {
    const key = trackMergeKey(track);
    if (key) remaining.set(key, track);
  }

  const playlist = existing.map((track) => {
    const key = trackMergeKey(track);
    const updated = remaining.get(key);
    if (!updated) return track;
    remaining.delete(key);
    return mergeIncomingTrack(track, updated);
  });

  return dedupeTracks([...playlist, ...Array.from(remaining.values())]);
}

export function mergeTracksByPathWithStats(existing: Track[], incoming: Track[]) {
  const remaining = new Map<string, Track>();
  for (const track of incoming) {
    const key = trackMergeKey(track);
    if (key) remaining.set(key, track);
  }

  let updatedCount = 0;
  const playlist = existing.map((track) => {
    const key = trackMergeKey(track);
    const updated = remaining.get(key);
    if (!updated) return track;
    remaining.delete(key);
    updatedCount += 1;
    return mergeIncomingTrack(track, updated);
  });
  const addedTracks = Array.from(remaining.values());

  return {
    playlist: dedupeTracks([...playlist, ...addedTracks]),
    addedCount: addedTracks.length,
    updatedCount,
  };
}

export function mergeIncomingTrack(existing: Track, incoming: Track) {
  const incomingLyrics = incoming.lyrics ?? [];
  const existingLyrics = existing.lyrics ?? [];
  const merged = {
    ...incoming,
    sourceUrl: incoming.sourceUrl ?? existing.sourceUrl,
    sourceId: incoming.sourceId ?? existing.sourceId,
    cacheMissing: incoming.cacheMissing ?? false,
  };
  if (incomingLyrics.length === 0 && existingLyrics.length > 0) {
    return { ...merged, lyrics: existingLyrics };
  }
  return { ...merged, lyrics: incomingLyrics };
}

export function createLibraryActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "createUserPlaylist" | "deleteUserPlaylist" | "deleteTrack" | "loadBackendLibrary" | "importLocalTracks" | "markTracksCacheMissingByPaths" | "normalizeLibrary"> {
  return {
  createUserPlaylist: (name) => {
    const trimmedName = name.trim();
    if (!trimmedName) {
      get().showNotification("请输入歌单名称");
      return;
    }

    const createdAt = Date.now();
    set((state) => ({
      userPlaylists: [
        ...state.userPlaylists,
        {
          id: `playlist-${createdAt}-${Math.random()
            .toString(36)
            .slice(2, 8)}`,
          name: trimmedName,
          trackIds: [],
          createdAt,
        },
      ],
    }));
    get().showNotification(`已创建歌单：${trimmedName}`);
  },

  deleteUserPlaylist: (playlistId) => {
    const playlist = get().userPlaylists.find((item) => item.id === playlistId);
    if (!playlist) return;

    set((state) => ({
      userPlaylists: state.userPlaylists.filter((item) => item.id !== playlistId),
    }));
    get().showNotification(`已删除歌单：${playlist.name}`);
  },

  deleteTrack: async (trackId) => {
    const track = get().playlist.find((item) => item.id === trackId);
    if (!track) return;

    try {
      await invoke<boolean>("delete_track", {
        track: {
          id: track.id,
          path: track.path,
          sourceUrl: track.sourceUrl ?? null,
          sourceId: track.sourceId ?? null,
        },
      });

      const deletingCurrentTrack = get().currentTrack()?.id === trackId;
      if (deletingCurrentTrack) {
        sendCommand("stop");
      }
      resetNextIndexCache();

      set((state) => {
        const removedIndex = state.playlist.findIndex(
          (item) => item.id === trackId
        );
        if (removedIndex < 0) return {};

        const playlist = state.playlist.filter((item) => item.id !== trackId);
        const liked = { ...state.liked };
        delete liked[trackId];
        const currentTrackIndex =
          playlist.length === 0
            ? 0
            : removedIndex < state.currentTrackIndex
              ? state.currentTrackIndex - 1
              : removedIndex === state.currentTrackIndex
                ? Math.min(removedIndex, playlist.length - 1)
                : Math.min(state.currentTrackIndex, playlist.length - 1);

        return {
          playlist,
          currentTrackIndex,
          currentTime: deletingCurrentTrack ? 0 : state.currentTime,
          isPlaying: deletingCurrentTrack ? false : state.isPlaying,
          recentTrackIds: state.recentTrackIds.filter((id) => id !== trackId),
          liked,
          userPlaylists: state.userPlaylists.map((playlist) => ({
            ...playlist,
            trackIds: playlist.trackIds.filter((id) => id !== trackId),
          })),
        };
      });

      get().showNotification(`已从曲库移除：${track.title}`);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: delete_track", err);
      get().showNotification("删除曲库记录失败");
    }
  },

  loadBackendLibrary: async () => {
    try {
      const cached = await invoke<Track[]>("get_playlist");
      if (!Array.isArray(cached) || cached.length === 0) return;

      set((state) => {
        const prevId = state.playlist[state.currentTrackIndex]?.id;
        const merged = dedupeTracksWithLiked(
          mergeTracksByPath(state.playlist, cached),
          state.liked
        );
        // M-8：合并/去重会改变顺序与长度，按曲目 id 重定位当前曲目，
        // 否则 currentTrackIndex 可能指向别的歌，甚至越界返回 null。
        const remapped = prevId
          ? merged.playlist.findIndex((track) => track.id === prevId)
          : -1;
        return {
          ...merged,
          currentTrackIndex: remapped >= 0 ? remapped : 0,
        };
      });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: get_playlist", err);
    }
  },

  importLocalTracks: async (paths) => {
    const cleanPaths = paths.filter(Boolean);
    if (cleanPaths.length === 0) return;

    try {
      const imported = await invoke<Track[]>("import_tracks", { paths: cleanPaths });
      const importedByPath = new Map<string, Track>();

      for (const track of imported) {
        const key = normalizePath(track.path);
        if (key) importedByPath.set(key, track);
      }

      if (importedByPath.size === 0) {
        get().showNotification("没有可添加的新音频文件");
        return;
      }

      let updatedCount = 0;
      let addedCount = 0;
      const previousLength = get().playlist.length;

      set((state) => {
        const remaining = new Map(importedByPath);
        const playlist = state.playlist.map((track) => {
          const key = normalizePath(track.path);
          const updatedTrack = remaining.get(key);
          if (!updatedTrack) return track;

          remaining.delete(key);
          updatedCount += 1;
          return updatedTrack;
        });
        const newTracks = Array.from(remaining.values());
        addedCount = newTracks.length;

        return {
          playlist: [...playlist, ...newTracks],
          currentTrackIndex: previousLength === 0 && newTracks.length > 0
            ? 0
            : state.currentTrackIndex,
          activeView: "local",
        };
      });

      if (updatedCount === 0 && addedCount === 0) {
        get().showNotification("没有可添加的新音频文件");
        return;
      }

      if (addedCount > 0 && updatedCount > 0) {
        get().showNotification(`已添加 ${addedCount} 首，更新 ${updatedCount} 首本地音乐`);
      } else if (addedCount > 0) {
        get().showNotification(`已添加 ${addedCount} 首本地音乐`);
      } else {
        get().showNotification(`已更新 ${updatedCount} 首本地音乐`);
      }
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: import_tracks", err);
      get().showNotification("导入本地音乐失败");
    }
  },

  markTracksCacheMissingByPaths: (paths) => {
    const removed = new Set(paths.map(normalizePath).filter(Boolean));
    if (removed.size === 0) return;
    set((state) => {
      const playlist = state.playlist.map((track) =>
        removed.has(normalizePath(track.path)) && streamingSourceInput(track)
          ? { ...track, cacheMissing: true, size: "0 MB" }
          : track
      );
      return {
        playlist,
        currentTrackIndex: Math.min(state.currentTrackIndex, Math.max(playlist.length - 1, 0)),
      };
    });
  },

  normalizeLibrary: () => {
    set((state) => {
      const deduped = dedupeTracksWithLiked(state.playlist, state.liked);
      return {
        ...deduped,
        currentTrackIndex: Math.min(
          state.currentTrackIndex,
          Math.max(deduped.playlist.length - 1, 0)
        ),
      };
    });
  },
  };
}

