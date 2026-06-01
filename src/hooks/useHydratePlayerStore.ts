import { useEffect } from "react";
import { runWhenIdle } from "@/lib/startup";
import { usePlayerStore } from "@/store/player";

export function useHydratePlayerStore() {
  useEffect(() => {
    return runWhenIdle(() => {
      const hydration = usePlayerStore.persist.rehydrate();
      void Promise.resolve(hydration).then(() => {
        const state = usePlayerStore.getState();
        state.normalizeLibrary();
        void state.loadBackendLibrary();
        state.loadDevices();
      });
    }, 1800);
  }, []);
}
