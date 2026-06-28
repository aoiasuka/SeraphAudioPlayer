import type { PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

let notificationCounter = 0;

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

    showNotification: (text) => {
      notificationCounter += 1;
      const id = notificationCounter + Date.now() * 1000;
      set({ notification: { id, text } });
    },

    dismissNotification: () => {
      if (get().notification === null) return;
      set({ notification: null });
    },
  };
}

