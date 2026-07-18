import { create } from "zustand";
import type { LucideIcon } from "lucide-react";

/**
 * 全局右键菜单状态。
 *
 * v0.4.3：应用内所有表面（曲目行/歌单/专辑/艺术家/播放条/UP NEXT/歌词稿）共用
 * 一个单例菜单渲染层（ContextMenuLayer），各表面在 onContextMenu 里构建条目
 * 快照后经 showContextMenu 打开。曲目信息 / 新建歌单并加入 / 删除确认三个
 * 全局弹窗的状态也集中在这里，菜单条目可直接触发而不依赖调用方组件的局部状态。
 */

export interface ContextMenuAction {
  type?: "action";
  key: string;
  label: string;
  icon?: LucideIcon;
  /** 右侧辅助小字（如歌单曲目数、「已加入」） */
  hint?: string;
  danger?: boolean;
  disabled?: boolean;
  /** 悬停展开的子菜单条目 */
  children?: ContextMenuEntry[];
  onSelect?: () => void;
}

export interface ContextMenuSeparator {
  type: "separator";
  key: string;
}

export type ContextMenuEntry = ContextMenuAction | ContextMenuSeparator;

export function isSeparator(
  entry: ContextMenuEntry
): entry is ContextMenuSeparator {
  return entry.type === "separator";
}

interface ContextMenuState {
  open: boolean;
  x: number;
  y: number;
  entries: ContextMenuEntry[];
  /** 曲目信息弹窗展示的曲目 id */
  infoTrackId: string | null;
  /** 「新建歌单并加入」弹窗锁定的曲目 id（单曲或整组） */
  createPlaylistTrackIds: string[] | null;
  /** 全局删除曲库记录确认弹窗锁定的曲目 id */
  confirmDeleteTrackId: string | null;
  /** B 站流媒体「重新加载」弹窗锁定的曲目 id */
  reloadStreamingTrackId: string | null;
  openContextMenu: (
    position: { x: number; y: number },
    entries: ContextMenuEntry[]
  ) => void;
  closeContextMenu: () => void;
  openTrackInfo: (trackId: string) => void;
  closeTrackInfo: () => void;
  openCreatePlaylistWith: (trackIds: string[]) => void;
  closeCreatePlaylistWith: () => void;
  requestDeleteTrack: (trackId: string) => void;
  closeDeleteTrack: () => void;
  openReloadStreaming: (trackId: string) => void;
  closeReloadStreaming: () => void;
}

export const useContextMenuStore = create<ContextMenuState>()((set) => ({
  open: false,
  x: 0,
  y: 0,
  entries: [],
  infoTrackId: null,
  createPlaylistTrackIds: null,
  confirmDeleteTrackId: null,
  reloadStreamingTrackId: null,
  openContextMenu: ({ x, y }, entries) => set({ open: true, x, y, entries }),
  closeContextMenu: () => set({ open: false, entries: [] }),
  openTrackInfo: (trackId) =>
    set({ infoTrackId: trackId, open: false, entries: [] }),
  closeTrackInfo: () => set({ infoTrackId: null }),
  openCreatePlaylistWith: (trackIds) =>
    set({ createPlaylistTrackIds: trackIds, open: false, entries: [] }),
  closeCreatePlaylistWith: () => set({ createPlaylistTrackIds: null }),
  requestDeleteTrack: (trackId) =>
    set({ confirmDeleteTrackId: trackId, open: false, entries: [] }),
  closeDeleteTrack: () => set({ confirmDeleteTrackId: null }),
  openReloadStreaming: (trackId) =>
    set({ reloadStreamingTrackId: trackId, open: false, entries: [] }),
  closeReloadStreaming: () => set({ reloadStreamingTrackId: null }),
}));

/**
 * 组件侧统一入口：阻断默认菜单并在事件坐标处打开自绘菜单。
 * 左键触发（如 UP NEXT 的「更多」按钮）与右键 onContextMenu 都可使用。
 * 条目为空时只做屏蔽不弹菜单。
 */
export function showContextMenu(
  event: { preventDefault(): void; clientX: number; clientY: number },
  entries: ContextMenuEntry[]
) {
  event.preventDefault();
  if (entries.length === 0) return;
  useContextMenuStore
    .getState()
    .openContextMenu({ x: event.clientX, y: event.clientY }, entries);
}
