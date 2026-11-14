import { create } from "zustand";

export type ImmersiveMode = "lyrics" | "analysis";

interface ImmersiveState {
  isOpen: boolean;
  mode: ImmersiveMode;
  /** 放大歌词字号；关闭沉浸页再进来仍保留（不持久化到磁盘） */
  largeLyrics: boolean;
  toggleLargeLyrics: () => void;
  open: () => void;
  close: () => void;
  setMode: (mode: ImmersiveMode) => void;
}

// 仅保存界面状态；两种沉浸视图始终使用原播放器的曲目、队列和播放会话。
export const useImmersiveStore = create<ImmersiveState>((set) => ({
  isOpen: false,
  mode: "lyrics",
  largeLyrics: false,
  toggleLargeLyrics: () => set((state) => ({ largeLyrics: !state.largeLyrics })),
  open: () => set({ isOpen: true, mode: "lyrics" }),
  close: () => set({ isOpen: false }),
  setMode: (mode) => set({ mode }),
}));
