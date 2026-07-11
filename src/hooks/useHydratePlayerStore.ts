import { useEffect } from "react";
import { runWhenIdle } from "@/lib/startup";
import { usePlayerStore } from "@/store/player";
import { hydrationGate } from "@/store/player/persistStorage";

export function useHydratePlayerStore() {
  useEffect(() => {
    return runWhenIdle(() => {
      // 审2-R1：必须在 rehydrate 之前打开写门闩，version 迁移触发的回写才不会被丢弃
      hydrationGate.ready = true;
      const hydration = usePlayerStore.persist.rehydrate();
      void Promise.resolve(hydration).then(() => {
        const state = usePlayerStore.getState();
        if ((state.driverKind as string) === "usb") {
          usePlayerStore.setState({ driverKind: "wasapi" });
        } else if (state.driverKind === "asio") {
          usePlayerStore.setState({ driverKind: "direct" });
        }
        state.normalizeLibrary();
        void state.loadBackendLibrary();
        state.loadDevices();
      });
    }, 1800);
  }, []);
}
