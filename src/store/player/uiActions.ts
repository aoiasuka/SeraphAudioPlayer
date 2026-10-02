import type { NotificationPayload, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

let notificationCounter = 0;
/** 错误提示显示期间到达的普通提示：只保留最新一条，错误消失后补显示。 */
let deferredInfo: NotificationPayload | null = null;

function nextId() {
  notificationCounter += 1;
  return notificationCounter + Date.now() * 1000;
}

export function createUiActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "setActiveView" | "toggleSettings" | "showNotification" | "dismissNotification"> {
  return {
    setActiveView: (view) => {
      if (get().activeView === view) return;
      set({ activeView: view });
    },

    toggleSettings: () => set({ settingsOpen: !get().settingsOpen }),

    showNotification: (text, level = "info") => {
      const payload: NotificationPayload = { id: nextId(), text, level };
      // REL-08：单槽通知里，失败提示常被紧随其后的成功提示冲掉（例如批量操作
      // 部分失败后又弹「已完成」）。错误在显示中时，普通提示延后到它消失再出现。
      if (level === "info" && get().notification?.level === "error") {
        deferredInfo = payload;
        return;
      }
      if (level === "error") deferredInfo = null;
      set({ notification: payload });
    },

    dismissNotification: () => {
      if (get().notification === null && deferredInfo === null) return;
      const next = deferredInfo;
      deferredInfo = null;
      set({ notification: next ? { ...next, id: nextId() } : null });
    },
  };
}
