import { invoke, isTauriRuntime, normalizeIpcError } from "@/lib/tauri";
import { emptyLyricDocument } from "@/lib/lyrics/document";
import type { DeleteTracksResult, Track } from "@/types/track";
import { resetNextIndexCache } from "./playbackActions";
import { bumpPlayEpoch } from "./playEpoch";
import { normalizePath, streamingSourceInput, trackMergeKey } from "./trackIdentity";
import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

let libraryLoadSequence = 0;

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

  // 审2-R11：在原 liked 基础上做 id 迁移/合并，而不是只保留在场曲目——
  // 曲库分批加载/合并时不在当前 playlist 的收藏（如后端尚未返回的流媒体曲目）不再被丢弃。
  const nextLiked: Record<string, boolean> = { ...liked };
  for (const track of playlist) {
    // 重复曲目合并后，存活条目按 mergeKey 继承 liked（保持既有语义）
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
  const incomingLyrics = incoming.lyrics ?? emptyLyricDocument();
  const existingLyrics = existing.lyrics ?? emptyLyricDocument();
  const merged = {
    ...incoming,
    sourceUrl: incoming.sourceUrl ?? existing.sourceUrl,
    sourceId: incoming.sourceId ?? existing.sourceId,
    cacheMissing: incoming.cacheMissing ?? false,
  };
  if (incomingLyrics.lines.length === 0 && existingLyrics.lines.length > 0) {
    return { ...merged, lyrics: existingLyrics, lyricsLoaded: true };
  }
  return { ...merged, lyrics: incomingLyrics };
}

export function createLibraryActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "createUserPlaylist" | "renameUserPlaylist" | "deleteUserPlaylist" | "addTrackToUserPlaylist" | "removeTrackFromUserPlaylist" | "moveTrackInUserPlaylist" | "importPlaylistFromM3u8" | "exportUserPlaylistToM3u8" | "deleteTrack" | "deleteTracks" | "loadBackendLibrary" | "importLocalTracks" | "fetchOnlineCoverForCurrentTrack" | "markTracksCacheMissingByPaths" | "normalizeLibrary"> {
  return {
  createUserPlaylist: (name) => {
    const trimmedName = name.trim();
    if (!trimmedName) {
      get().showNotification("请输入歌单名称");
      return null;
    }

    const createdAt = Date.now();
    const id = `playlist-${createdAt}-${Math.random()
      .toString(36)
      .slice(2, 8)}`;
    set((state) => ({
      userPlaylists: [
        ...state.userPlaylists,
        {
          id,
          name: trimmedName,
          trackIds: [],
          createdAt,
        },
      ],
    }));
    get().showNotification(`已创建歌单：${trimmedName}`);
    return id;
  },

  renameUserPlaylist: (playlistId, name) => {
    const trimmedName = name.trim();
    if (!trimmedName) {
      get().showNotification("请输入歌单名称");
      return;
    }
    const playlist = get().userPlaylists.find((item) => item.id === playlistId);
    if (!playlist || playlist.name === trimmedName) return;

    set((state) => ({
      userPlaylists: state.userPlaylists.map((item) =>
        item.id === playlistId ? { ...item, name: trimmedName } : item
      ),
    }));
    get().showNotification(`已重命名歌单：${trimmedName}`);
  },

  deleteUserPlaylist: (playlistId) => {
    const playlist = get().userPlaylists.find((item) => item.id === playlistId);
    if (!playlist) return;

    set((state) => ({
      userPlaylists: state.userPlaylists.filter((item) => item.id !== playlistId),
    }));
    get().showNotification(`已删除歌单：${playlist.name}`);
  },

  addTrackToUserPlaylist: (playlistId, trackId) => {
    const playlist = get().userPlaylists.find((item) => item.id === playlistId);
    const track = get().playlist.find((item) => item.id === trackId);
    if (!playlist || !track) return;
    if (playlist.trackIds.includes(trackId)) {
      get().showNotification(`已在歌单「${playlist.name}」中`);
      return;
    }

    set((state) => ({
      userPlaylists: state.userPlaylists.map((item) =>
        item.id === playlistId
          ? { ...item, trackIds: [...item.trackIds, trackId] }
          : item
      ),
    }));
    get().showNotification(`已加入歌单：${playlist.name}`);
  },

  removeTrackFromUserPlaylist: (playlistId, trackId) => {
    set((state) => ({
      userPlaylists: state.userPlaylists.map((item) =>
        item.id === playlistId
          ? { ...item, trackIds: item.trackIds.filter((id) => id !== trackId) }
          : item
      ),
    }));
  },

  moveTrackInUserPlaylist: (playlistId, trackId, direction) => {
    set((state) => ({
      userPlaylists: state.userPlaylists.map((item) => {
        if (item.id !== playlistId) return item;
        const index = item.trackIds.indexOf(trackId);
        const target = direction === "up" ? index - 1 : index + 1;
        if (index < 0 || target < 0 || target >= item.trackIds.length) {
          return item;
        }
        const trackIds = [...item.trackIds];
        [trackIds[index], trackIds[target]] = [trackIds[target], trackIds[index]];
        return { ...item, trackIds };
      }),
    }));
  },

  importPlaylistFromM3u8: async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "播放列表", extensions: ["m3u8", "m3u"] }],
      });
      if (typeof selected !== "string" || !selected) return;

      const imported = await invoke<{
        name: string;
        paths: string[];
        skipped: number;
        truncated?: boolean;
      }>("import_playlist_m3u8", { path: selected });

      if (imported.paths.length === 0) {
        get().showNotification("清单中没有可用的本地音频文件");
        return;
      }

      // 先入库（内部有去重合并），再按物理路径映射回曲目 id 建歌单；
      // keepView 留在歌单页，不被 importLocalTracks 甩到本地音乐页（M-7）
      await get().importLocalTracks(imported.paths, { keepView: true });
      const idByPath = new Map(
        get().playlist.map((track) => [normalizePath(track.path), track.id])
      );
      const trackIds = imported.paths
        .map((path) => idByPath.get(normalizePath(path)))
        .filter((id): id is string => !!id);

      if (trackIds.length === 0) {
        get().showNotification("清单曲目导入失败");
        return;
      }

      const names = new Set(get().userPlaylists.map((item) => item.name));
      const baseName = imported.name.trim() || "导入歌单";
      let name = baseName;
      let suffix = 2;
      while (names.has(name)) {
        name = `${baseName} ${suffix}`;
        suffix += 1;
      }

      const createdAt = Date.now();
      set((state) => ({
        userPlaylists: [
          ...state.userPlaylists,
          {
            id: `playlist-${createdAt}-${Math.random().toString(36).slice(2, 8)}`,
            name,
            trackIds,
            createdAt,
          },
        ],
      }));
      const parts = [`${trackIds.length} 首`];
      if (imported.skipped > 0) parts.push(`跳过 ${imported.skipped} 条`);
      if (imported.truncated) parts.push("清单超长已截断");
      get().showNotification(`已导入歌单「${name}」（${parts.join("，")}）`);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("import_playlist_m3u8 failed", err);
      get().showNotification(`导入歌单失败: ${normalizeIpcError(err).message}`);
    }
  },

  exportUserPlaylistToM3u8: async (playlistId) => {
    const userPlaylist = get().userPlaylists.find((item) => item.id === playlistId);
    if (!userPlaylist) return;
    const trackById = new Map(get().playlist.map((track) => [track.id, track]));
    const entries = userPlaylist.trackIds
      .map((id) => trackById.get(id))
      .filter((track): track is Track => !!track)
      .map((track) => ({
        title: track.title,
        artist: track.artist,
        duration: track.duration,
        path: track.path,
      }));
    if (entries.length === 0) {
      get().showNotification("歌单没有可导出的曲目");
      return;
    }

    try {
      const { save } = await import("@tauri-apps/plugin-dialog");
      const target = await save({
        defaultPath: `${userPlaylist.name}.m3u8`,
        filters: [{ name: "播放列表", extensions: ["m3u8"] }],
      });
      if (!target) return;

      await invoke("export_playlist_m3u8", { path: target, entries });
      get().showNotification(`已导出歌单：${userPlaylist.name}`);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("export_playlist_m3u8 failed", err);
      get().showNotification(`导出歌单失败: ${normalizeIpcError(err).message}`);
    }
  },

  deleteTrack: async (trackId) => {
    await get().deleteTracks([trackId]);
  },

  deleteTracks: async (trackIds) => {
    const requested = new Set(trackIds);
    const tracks = get().playlist.filter((track) => requested.has(track.id));
    const empty: DeleteTracksResult = { deletedIds: [], deletedFiles: 0, failures: [] };
    if (tracks.length === 0) return empty;
    const ids = tracks.map((track) => track.id);
    // 删除意图立即取消挂起的重缓存播放和此前的曲库读取。
    libraryLoadSequence += 1;
    const currentId = get().currentTrack()?.id;
    if (currentId && requested.has(currentId)) bumpPlayEpoch();

    try {
      // Windows 音频解码器会占用文件，收到停止回执后才能执行物理删除。
      if (currentId && requested.has(currentId)) {
        await invoke("stop");
        if (get().currentTrack()?.id === currentId) {
          set({ isPlaying: false, currentTime: 0 });
        }
      }
      let result = await invoke<DeleteTracksResult>("delete_tracks", { trackIds: ids });
      // 仅浏览器演示允许无 IPC 回执；桌面端绝不将无效响应视为删除成功。
      if (!result && !isTauriRuntime()) result = { ...empty, deletedIds: ids };
      if (!result || !Array.isArray(result.deletedIds) || !Array.isArray(result.failures)) {
        throw new Error("未收到有效的删除结果，请重试");
      }
      const deleted = new Set(result.deletedIds.filter((id) => requested.has(id)));
      libraryLoadSequence += 1;

      const latestId = get().currentTrack()?.id;
      if (latestId && deleted.has(latestId)) {
        bumpPlayEpoch();
        // 等待期间用户/系统可能切到另一首待删曲目，只停止仍在删除集合中的播放。
        if (latestId !== currentId || get().isPlaying) {
          await invoke("stop").catch((err) => {
            console.warn("删除后停止播放失败", err);
          });
        }
      }
      if (deleted.size > 0) resetNextIndexCache();

      set((state) => {
        if (deleted.size === 0) return {};
        const previousId = state.playlist[state.currentTrackIndex]?.id;
        const deletingCurrent = !!previousId && deleted.has(previousId);
        const playlist = state.playlist.filter((item) => !deleted.has(item.id));
        const liked = { ...state.liked };
        for (const id of deleted) delete liked[id];
        const survivingBefore = state.playlist.slice(0, state.currentTrackIndex)
          .filter((item) => !deleted.has(item.id)).length;
        const currentTrackIndex = deletingCurrent
          ? Math.max(0, Math.min(survivingBefore, playlist.length - 1))
          : Math.max(0, playlist.findIndex((item) => item.id === previousId));

        return {
          playlist,
          currentTrackIndex,
          currentTime: deletingCurrent ? 0 : state.currentTime,
          isPlaying: deletingCurrent ? false : state.isPlaying,
          persistedCurrentTrackId: playlist[currentTrackIndex]?.id ?? null,
          persistedCurrentTime: deletingCurrent ? 0 : state.currentTime,
          recentTrackIds: state.recentTrackIds.filter((id) => !deleted.has(id)),
          liked,
          userPlaylists: state.userPlaylists.map((playlist) => ({
            ...playlist,
            trackIds: playlist.trackIds.filter((id) => !deleted.has(id)),
          })),
        };
      });

      const summary = tracks.length === 1 && deleted.size === 1
        ? `已从曲库移除：${tracks[0].title}`
        : `已删除 ${deleted.size} 首曲目`;
      get().showNotification(
        `${summary}${result.deletedFiles > 0 ? `，清理 ${result.deletedFiles} 个缓存音频文件` : ""}` +
        (result.failures.length > 0 ? `；${result.failures.length} 首未删除，请查看失败原因` : "")
      );
      return result;
    } catch (err) {
      libraryLoadSequence += 1;
      const message = normalizeIpcError(err).message;
      console.warn("Tauri command failed: delete_tracks", err);
      get().showNotification(`删除失败：${message}`);
      return { ...empty, failures: tracks.map((track) => ({ id: track.id, title: track.title, message })) };
    }
  },

  fetchOnlineCoverForCurrentTrack: async () => {
    const track = get().currentTrack();
    if (!track) return false;
    if (track.cover) return true;

    try {
      const cover = await invoke<string>("fetch_online_cover", {
        trackId: track.id,
        title: track.title,
        artist: track.artist,
      });
      set((state) => ({
        playlist: state.playlist.map((item) =>
          item.id === track.id ? { ...item, cover } : item
        ),
      }));
      get().showNotification("封面匹配成功");
      return true;
    } catch (err) {
      const { code, message } = normalizeIpcError(err);
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: fetch_online_cover", err);
      get().showNotification(
        code === "not_found" ? "未找到匹配的在线封面" : `封面匹配失败: ${message}`
      );
      return false;
    }
  },

  loadBackendLibrary: async () => {
    const request = ++libraryLoadSequence;
    try {
      const response = await invoke<Track[]>("get_playlist", { includeLyrics: false });
      if (request !== libraryLoadSequence) return;
      const cached = Array.isArray(response)
        ? response.map((track) => ({
            ...track,
            lyrics: track.lyrics ?? emptyLyricDocument(),
            lyricsLoaded: (track.lyrics?.lines.length ?? 0) > 0,
          }))
        : response;
      if (!Array.isArray(cached) || cached.length === 0) return;

      set((state) => {
        // 发现1：playlist 尚未加载（启动水合）时用持久化的曲目 id 恢复上次播放位置
        const prevId =
          state.playlist[state.currentTrackIndex]?.id ?? state.persistedCurrentTrackId;
        const merged = dedupeTracksWithLiked(
          mergeTracksByPath(state.playlist, cached),
          state.liked
        );
        // M-8：合并/去重会改变顺序与长度，按曲目 id 重定位当前曲目，
        // 否则 currentTrackIndex 可能指向别的歌，甚至越界返回 null。
        const remapped = prevId
          ? merged.playlist.findIndex((track) => track.id === prevId)
          : -1;
        // 启动恢复播放进度：仅在未开播、进度仍为 0 的水合场景下应用持久化
        // 位置（钳制到曲目时长内）；曲目已不存在则清零，避免下次误恢复。
        // v0.4.2：记忆播放关闭时不恢复位置。
        const restoredTrack = remapped >= 0 ? merged.playlist[remapped] : null;
        const shouldRestoreTime =
          state.rememberPlayback &&
          !state.isPlaying &&
          state.currentTime === 0 &&
          restoredTrack !== null;
        const restoredTime = shouldRestoreTime
          ? Math.min(
              Math.max(0, state.persistedCurrentTime),
              restoredTrack.duration > 0
                ? Math.max(0, restoredTrack.duration - 1)
                : Number.MAX_SAFE_INTEGER
            )
          : null;
        return {
          ...merged,
          currentTrackIndex: remapped >= 0 ? remapped : 0,
          persistedCurrentTrackId:
            (remapped >= 0 ? prevId : merged.playlist[0]?.id) ?? null,
          ...(restoredTime !== null
            ? { currentTime: restoredTime }
            : remapped < 0
              ? { currentTime: 0, persistedCurrentTime: 0 }
              : {}),
        };
      });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: get_playlist", err);
      get().showNotification(`曲库读取失败：${normalizeIpcError(err).message}`);
    }
  },

  importLocalTracks: async (paths, options) => {
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
          // 发现8：与其他导入路径一致，保留已有歌词 / sourceUrl 等前端合并语义
          return mergeIncomingTrack(track, updatedTrack);
        });
        const newTracks = Array.from(remaining.values());
        addedCount = newTracks.length;

        return {
          playlist: [...playlist, ...newTracks],
          currentTrackIndex: previousLength === 0 && newTracks.length > 0
            ? 0
            : state.currentTrackIndex,
          // M-7：M3U8 建歌单等调用方传 keepView 留在当前页（此前无条件跳
          // 本地音乐页，导入歌单的瞬间用户被甩离歌单页）
          ...(options?.keepView ? {} : { activeView: "local" as const }),
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
      // 发现1：启动水合时 playlist 为空，保持持久化的索引 / 曲目 id 不动，
      // 等 loadBackendLibrary 按 id 重映射。
      if (state.playlist.length === 0) return {};

      // 发现9：与 loadBackendLibrary (M-8) 一致，去重后按曲目 id 重映射索引，
      // 避免去重移除靠前的重复项后 currentTrackIndex 指向别的歌。
      const prevId = state.playlist[state.currentTrackIndex]?.id;
      const deduped = dedupeTracksWithLiked(state.playlist, state.liked);
      const remapped = prevId
        ? deduped.playlist.findIndex((track) => track.id === prevId)
        : -1;
      return {
        ...deduped,
        currentTrackIndex:
          remapped >= 0
            ? remapped
            : Math.min(
                state.currentTrackIndex,
                Math.max(deduped.playlist.length - 1, 0)
              ),
      };
    });
  },
  };
}

