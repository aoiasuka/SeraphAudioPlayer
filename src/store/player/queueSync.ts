import { invoke } from "@/lib/tauri";
import type { Track } from "@/types/track";
import type { PlaybackQueuePreview, PlayScope, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

const clientId = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
let sequence = 0;
let contentSequence = 0;
let acknowledgedRevision: string | null = null;
let acknowledged: { state: PlayerStore; preview: PlaybackQueuePreview } | null = null;
const contents = new WeakMap<Track[], { revision: string; tracks: ReturnType<typeof playbackQueueArgs>["tracks"] }>();
let lastContent: { playlist: Track[]; content: NonNullable<ReturnType<typeof contents.get>> } | null = null;
const pending = new Set<{ state: PlayerStore; sequence: number; promise: Promise<void> }>();

let scopeMemo: { playlist: Track[]; scope: PlayScope; tracks: Track[]; ids: Set<string> } | null = null;

/**
 * 实际的播放队列（PLAY6-01）：有播放范围时是范围内仍在曲库里的曲目（按范围顺序），
 * 否则是整个曲库。当前曲目不在范围内（如被删除后切到别的歌）时退回整个曲库，
 * 保证「当前曲目一定在队列里」。同一组 (playlist, scope) 返回同一个数组引用。
 * 本文件被基准脚本单独加载，不能新增运行时导入。
 */
export function playbackQueueTracks(
  state: Pick<PlayerStore, "playlist" | "playScope" | "currentTrackIndex">
): Track[] {
  const { playlist, playScope } = state;
  if (!playScope) return playlist;
  if (!scopeMemo || scopeMemo.playlist !== playlist || scopeMemo.scope !== playScope) {
    const byId = new Map<string, Track>();
    for (const track of playlist) if (!byId.has(track.id)) byId.set(track.id, track);
    const ids = new Set<string>();
    const tracks: Track[] = [];
    for (const id of playScope.trackIds) {
      const track = byId.get(id);
      if (!track || ids.has(id)) continue;
      ids.add(id);
      tracks.push(track);
    }
    scopeMemo = { playlist, scope: playScope, tracks, ids };
  }
  const currentId = playlist[state.currentTrackIndex]?.id;
  if (scopeMemo.tracks.length === 0 || (currentId !== undefined && !scopeMemo.ids.has(currentId))) {
    return playlist;
  }
  return scopeMemo.tracks;
}

/** 当前曲目在队列里的下标；队列就是曲库时等于 currentTrackIndex。 */
function queueIndex(state: PlayerStore, queue: Track[]) {
  if (queue === state.playlist) return state.currentTrackIndex;
  const currentId = state.playlist[state.currentTrackIndex]?.id;
  return Math.max(0, queue.findIndex((track) => track.id === currentId));
}

function sameQueueState(a: PlayerStore, b: PlayerStore) {
  return a.playlist === b.playlist && a.playScope === b.playScope &&
    a.currentTrackIndex === b.currentTrackIndex &&
    a.recentTrackIds === b.recentTrackIds && a.shuffleMode === b.shuffleMode &&
    a.loopMode === b.loopMode;
}

/** 重新建立桌面连接或重置 store 后，使下一次同步发送完整快照。 */
export function resetPlaybackQueueSync() {
  sequence += 1;
  acknowledgedRevision = null;
  acknowledged = null;
  lastContent = null;
  pending.clear();
}

export function playbackQueueArgs(get: PlayerStoreGet) {
  const state = get();
  const { recentTrackIds, shuffleMode, loopMode } = state;
  const queue = playbackQueueTracks(state);
  const currentTrackIndex = queueIndex(state, queue);

  return {
    tracks: queue.map((track) => ({
      id: track.id,
      path: track.path,
      // SMTC 系统媒体浮窗展示用元数据
      title: track.title,
      artist: track.artist,
      album: track.album,
      cover: track.cover,
      duration: track.duration,
    })),
    currentTrackIndex,
    recentTrackIds,
    shuffleMode,
    loopMode,
  };
}

export function syncPlaybackQueue(get: PlayerStoreGet, set: PlayerStoreSet): Promise<void> {
  const state = get();
  for (const request of pending) {
    if (request.sequence === sequence && sameQueueState(request.state, state)) return request.promise;
  }
  if (acknowledged && sameQueueState(acknowledged.state, state) &&
      state.playbackQueuePreview === acknowledged.preview) return Promise.resolve();

  const queue = playbackQueueTracks(state);
  let content = contents.get(queue);
  if (!content) {
    // 按需加载歌词会替换 playlist 引用，但不改变播放队列元数据；沿用原版本。
    const sameContent = lastContent && queue.length === lastContent.playlist.length &&
      queue.every((track, index) => {
        const previous = lastContent!.playlist[index];
        return track === previous || (track.id === previous.id && track.path === previous.path &&
          track.title === previous.title && track.artist === previous.artist && track.album === previous.album &&
          track.cover === previous.cover && track.duration === previous.duration);
      });
    content = sameContent ? lastContent!.content :
      { revision: `${clientId}:${++contentSequence}`, tracks: playbackQueueArgs(() => state).tracks };
    contents.set(queue, content);
  }
  lastContent = { playlist: queue, content };
  const { revision, tracks } = content;
  const requestSequence = ++sequence;
  const args = {
    // 后端的下标是队列内下标，不是曲库下标
    currentTrackIndex: queueIndex(state, queue),
    recentTrackIds: state.recentTrackIds,
    shuffleMode: state.shuffleMode,
    loopMode: state.loopMode,
    sync: { clientId, revision, sequence: requestSequence },
  };
  const send = (full: boolean) => invoke<PlaybackQueuePreview>("sync_playback_queue", {
    ...args, ...(full ? { tracks } : {}),
  });
  const full = acknowledgedRevision !== revision;
  const request = { state, sequence: requestSequence, promise: Promise.resolve() };
  request.promise = send(full)
    .catch((error: unknown) => {
      // 后端重启/旧客户端改写队列后恢复全量；过期请求不再重试。
      if (!full && error === "queue_revision_mismatch" && requestSequence === sequence &&
          sameQueueState(get(), state)) return send(true);
      throw error;
    })
    .then((preview) => {
      if (!preview || requestSequence !== sequence) return;
      acknowledgedRevision = revision;
      if (!sameQueueState(get(), state)) return;
      acknowledged = { state, preview };
      set({ playbackQueuePreview: preview });
    })
    .finally(() => { pending.delete(request); });
  pending.add(request);
  return request.promise;
}

// 模式更新与 effect 共用同一在途请求/序号，避免交叉覆盖。
export const syncPlaybackModes = syncPlaybackQueue;
