import {
  ArrowDown,
  ArrowRight,
  ArrowUp,
  Copy,
  FolderSearch,
  Heart,
  Image,
  Info,
  ListPlus,
  ListX,
  Pause,
  Play,
  Plus,
  RotateCw,
  Search,
  Trash2,
} from "lucide-react";
import { copyText } from "@/lib/clipboard";
import { revealTrackFile } from "@/lib/system";
import {
  useContextMenuStore,
  type ContextMenuAction,
  type ContextMenuEntry,
} from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { isStreamingTrack } from "@/components/pages/main-pages/trackFilters";

/**
 * 各表面右键菜单的条目构建器。
 *
 * 条目在菜单打开瞬间用 getState() 快照构建（打开期间数据变化不回流菜单），
 * onSelect 里再取一次最新 state 执行，两段都不产生组件订阅。
 */

async function copyWithNotify(text: string) {
  const copied = await copyText(text);
  usePlayerStore
    .getState()
    .showNotification(copied ? "已复制到剪贴板" : "复制失败");
}

/** 「加入歌单 ▸」子菜单：列出全部歌单（已加入置灰）+ 新建歌单。多曲时整组加入。 */
export function buildAddToPlaylistEntry(trackIds: string[]): ContextMenuAction {
  const state = usePlayerStore.getState();
  const menu = useContextMenuStore.getState();
  const single = trackIds.length === 1 ? trackIds[0] : null;

  return {
    key: "add-to-playlist",
    label:
      trackIds.length > 1 ? `整组加入歌单（${trackIds.length} 首）` : "加入歌单",
    icon: ListPlus,
    children: [
      ...state.userPlaylists.map<ContextMenuEntry>((playlist) => {
        const included = single
          ? playlist.trackIds.includes(single)
          : trackIds.every((id) => playlist.trackIds.includes(id));
        return {
          key: `playlist-${playlist.id}`,
          label: playlist.name,
          hint: included ? "已加入" : `${playlist.trackIds.length} 首`,
          disabled: included,
          onSelect: () => {
            const live = usePlayerStore.getState();
            trackIds.forEach((id) =>
              live.addTrackToUserPlaylist(playlist.id, id)
            );
            if (trackIds.length > 1) {
              live.showNotification(
                `已把 ${trackIds.length} 首加入歌单：${playlist.name}`
              );
            }
          },
        };
      }),
      ...(state.userPlaylists.length > 0
        ? [{ type: "separator", key: "playlist-sep" } as ContextMenuEntry]
        : []),
      {
        key: "playlist-new",
        label: "新建歌单…",
        icon: Plus,
        onSelect: () => menu.openCreatePlaylistWith(trackIds),
      },
    ],
  };
}

export interface TrackMenuOptions {
  /** 歌单详情页上下文：追加上移/下移/移出本歌单 */
  inPlaylist?: { playlistId: string; position: number; total: number };
}

/** 曲目行通用菜单（本地音乐/最近播放/我喜欢/专辑详情/艺术家详情/歌单详情）。 */
export function buildTrackMenuEntries(
  track: Track,
  options: TrackMenuOptions = {}
): ContextMenuEntry[] {
  const state = usePlayerStore.getState();
  const menu = useContextMenuStore.getState();
  const index = state.playlist.findIndex((item) => item.id === track.id);
  const isCurrent = state.currentTrack()?.id === track.id;
  const playing = isCurrent && state.isPlaying;

  const entries: ContextMenuEntry[] = [
    {
      key: "play",
      label: playing
        ? "暂停"
        : track.cacheMissing
          ? "重新缓存并播放"
          : "播放",
      icon: playing ? Pause : track.cacheMissing ? RotateCw : Play,
      disabled: index < 0 && !isCurrent,
      onSelect: () => {
        const live = usePlayerStore.getState();
        if (isCurrent) live.togglePlayback();
        else if (index >= 0) live.loadTrack(index);
      },
    },
    {
      key: "like",
      label: state.liked[track.id] ? "取消收藏" : "收藏",
      icon: Heart,
      onSelect: () => usePlayerStore.getState().toggleLike(track.id),
    },
    buildAddToPlaylistEntry([track.id]),
  ];

  if (options.inPlaylist) {
    const { playlistId, position, total } = options.inPlaylist;
    entries.push(
      { type: "separator", key: "sep-playlist" },
      {
        key: "move-up",
        label: "上移",
        icon: ArrowUp,
        disabled: position <= 0,
        onSelect: () =>
          usePlayerStore
            .getState()
            .moveTrackInUserPlaylist(playlistId, track.id, "up"),
      },
      {
        key: "move-down",
        label: "下移",
        icon: ArrowDown,
        disabled: position >= total - 1,
        onSelect: () =>
          usePlayerStore
            .getState()
            .moveTrackInUserPlaylist(playlistId, track.id, "down"),
      },
      {
        key: "remove-from-playlist",
        label: "移出本歌单",
        icon: ListX,
        onSelect: () =>
          usePlayerStore
            .getState()
            .removeTrackFromUserPlaylist(playlistId, track.id),
      }
    );
  }

  if (isStreamingTrack(track)) {
    entries.push({
      key: "reload-streaming",
      label: "重新加载…",
      icon: RotateCw,
      onSelect: () => menu.openReloadStreaming(track.id),
    });
  }

  entries.push(
    { type: "separator", key: "sep-info" },
    {
      key: "info",
      label: "曲目信息…",
      icon: Info,
      onSelect: () => menu.openTrackInfo(track.id),
    },    {
      key: "copy",
      label: "复制",
      icon: Copy,
      children: [
        {
          key: "copy-title",
          label: "标题 — 艺术家",
          onSelect: () => void copyWithNotify(`${track.title} — ${track.artist}`),
        },
        {
          key: "copy-path",
          label: "文件路径",
          disabled: !track.path,
          onSelect: () => void copyWithNotify(track.path),
        },
      ],
    },
    {
      key: "reveal",
      label: "打开文件所在位置",
      icon: FolderSearch,
      disabled: !track.path || !!track.cacheMissing,
      onSelect: () => void revealTrackFile(track.path),
    },
    { type: "separator", key: "sep-danger" },
    {
      key: "delete",
      label: "删除曲库记录",
      icon: Trash2,
      danger: true,
      onSelect: () => menu.requestDeleteTrack(track.id),
    }
  );

  return entries;
}

/** 播放条当前曲目菜单：收藏/加歌单/在线匹配封面·歌词/信息/打开位置。 */
export function buildCurrentTrackMenuEntries(track: Track): ContextMenuEntry[] {
  const state = usePlayerStore.getState();
  const menu = useContextMenuStore.getState();

  return [
    {
      key: "like",
      label: state.liked[track.id] ? "取消收藏" : "收藏",
      icon: Heart,
      onSelect: () => usePlayerStore.getState().toggleLike(track.id),
    },
    buildAddToPlaylistEntry([track.id]),
    { type: "separator", key: "sep-online" },
    {
      key: "match-cover",
      label: "在线匹配封面",
      icon: Image,
      hint: track.cover ? "已有封面" : undefined,
      disabled: !!track.cover,
      onSelect: () =>
        void usePlayerStore.getState().fetchOnlineCoverForCurrentTrack(),
    },
    {
      key: "match-lyrics",
      label: "在线匹配歌词",
      icon: Search,
      onSelect: () =>
        window.dispatchEvent(new CustomEvent("seraph:open-lyrics-search")),
    },
    { type: "separator", key: "sep-info" },
    {
      key: "info",
      label: "曲目信息…",
      icon: Info,
      onSelect: () => menu.openTrackInfo(track.id),
    },
    {
      key: "reveal",
      label: "打开文件所在位置",
      icon: FolderSearch,
      disabled: !track.path || !!track.cacheMissing,
      onSelect: () => void revealTrackFile(track.path),
    },
  ];
}

/** UP NEXT 卡片菜单。 */
export function buildUpNextMenuEntries(track: Track): ContextMenuEntry[] {
  const state = usePlayerStore.getState();
  const menu = useContextMenuStore.getState();

  return [
    {
      key: "play-now",
      label: "立即播放这首",
      icon: Play,
      onSelect: () => usePlayerStore.getState().playNextPreview(),
    },
    {
      key: "like",
      label: state.liked[track.id] ? "取消收藏" : "收藏",
      icon: Heart,
      onSelect: () => usePlayerStore.getState().toggleLike(track.id),
    },
    buildAddToPlaylistEntry([track.id]),
    { type: "separator", key: "sep-info" },
    {
      key: "info",
      label: "曲目信息…",
      icon: Info,
      onSelect: () => menu.openTrackInfo(track.id),
    },
  ];
}

/** 专辑/艺术家卡片菜单：打开详情、从第一首播放、整组加入歌单。 */
export function buildTrackGroupMenuEntries(
  tracks: Track[],
  openDetail: () => void
): ContextMenuEntry[] {
  const state = usePlayerStore.getState();
  const first = tracks[0];
  const firstIndex = first
    ? state.playlist.findIndex((item) => item.id === first.id)
    : -1;

  return [
    {
      key: "open",
      label: "打开",
      icon: ArrowRight,
      onSelect: openDetail,
    },
    {
      key: "play-all",
      label: "从第一首开始播放",
      icon: Play,
      disabled: firstIndex < 0,
      onSelect: () => {
        const live = usePlayerStore.getState();
        if (firstIndex >= 0) live.loadTrack(firstIndex);
      },
    },
    buildAddToPlaylistEntry(tracks.map((track) => track.id)),
  ];
}
