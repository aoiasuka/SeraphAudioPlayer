import type { PersistStorage } from "zustand/middleware";
import type { PersistedPlayerState } from "./types";

function isSamePersistedState(
  previous: PersistedPlayerState,
  next: PersistedPlayerState
) {
  return (
    previous.currentTrackIndex === next.currentTrackIndex &&
    previous.recentTrackIds === next.recentTrackIds &&
    previous.volume === next.volume &&
    previous.isMuted === next.isMuted &&
    previous.previousVolume === next.previousVolume &&
    previous.shuffleMode === next.shuffleMode &&
    previous.loopMode === next.loopMode &&
    previous.liked === next.liked &&
    previous.userPlaylists === next.userPlaylists &&
    previous.currentDeviceId === next.currentDeviceId &&
    previous.driverKind === next.driverKind &&
    previous.activeView === next.activeView
  );
}

export function createPlayerPersistStorage(): PersistStorage<PersistedPlayerState> {
  let lastValue: {
    state: PersistedPlayerState;
    version?: number;
  } | null = null;
  const memoryStorage = new Map<string, string>();

  const storage = () =>
    typeof window === "undefined" ? null : window.localStorage;

  return {
    getItem: (name) => {
      const value = storage()?.getItem(name) ?? memoryStorage.get(name) ?? null;
      if (value === null) {
        lastValue = null;
        return null;
      }

      try {
        const parsed = JSON.parse(value) as {
          state: PersistedPlayerState;
          version?: number;
        };
        lastValue = parsed;
        return parsed;
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Failed to parse persisted player state, resetting it", err);
        lastValue = null;
        storage()?.removeItem(name);
        memoryStorage.delete(name);
        return null;
      }
    },
    setItem: (name, value) => {
      if (
        lastValue &&
        lastValue.version === value.version &&
        isSamePersistedState(lastValue.state, value.state)
      ) {
        return;
      }

      lastValue = value;
      const serialized = JSON.stringify(value);
      const localStorage = storage();
      if (localStorage) {
        try {
          localStorage.setItem(name, serialized);
        } catch (err) {
          // QuotaExceededError / SecurityError 等：回退到内存存储，避免崩溃
          // eslint-disable-next-line no-console
          console.warn(
            "localStorage.setItem failed, falling back to memory storage",
            err
          );
          memoryStorage.set(name, serialized);
        }
      } else {
        memoryStorage.set(name, serialized);
      }
    },
    removeItem: (name) => {
      lastValue = null;
      storage()?.removeItem(name);
      memoryStorage.delete(name);
    },
  };
}

