import { create } from "zustand";

export type ImmersiveMode = "lyrics" | "analysis";

interface ImmersiveState {
  isOpen: boolean;
  mode: ImmersiveMode;
  open: () => void;
  close: () => void;
  setMode: (mode: ImmersiveMode) => void;
}

// 仅保存界面状态；两种沉浸视图始终使用原播放器的曲目、队列和播放会话。
export const useImmersiveStore = create<ImmersiveState>((set) => ({
  isOpen: false,
  mode: "lyrics",
  open: () => set({ isOpen: true, mode: "lyrics" }),
  close: () => set({ isOpen: false }),
  setMode: (mode) => set({ mode }),
}));
