import { useEffect } from "react";
import { runWhenIdle } from "@/lib/startup";
import { invoke, isTauriRuntime, listen } from "@/lib/tauri";
import { useEqStore } from "@/store/eq";
import { usePlayerStore } from "@/store/player";
import { lyricsDisplayOptionsOf } from "@/store/player/lyricsActions";
import { hydrationGate } from "@/store/player/persistStorage";

export function useHydratePlayerStore() {
  useEffect(() => {
    let cancelled = false;
    let cancelLibraryLoad: (() => void) | undefined;
    const unlisteners: (() => void)[] = [];
    const libraryEventsReady = isTauriRuntime() ? Promise.all([
      listen("seraph://library-updated", () => { void usePlayerStore.getState().loadBackendLibrary(); }),
      listen<string>("seraph://library-import-warning", (message) => usePlayerStore.getState().showNotification(message)),
    ].map((subscription) => subscription.then((unlisten) => {
      if (cancelled) unlisten();
      else unlisteners.push(unlisten);
    }).catch((error) => { console.warn("曲库事件监听失败", error); }))) : Promise.resolve();
    // 播放配置立即水合；只把曲库扫描留到空闲时，避免首个点击使用默认音量。
    // 审2-R1：必须在 rehydrate 之前打开写门闩，version 迁移触发的回写才不会被丢弃
    hydrationGate.ready = true;
    const hydration = usePlayerStore.persist.rehydrate();
    void Promise.resolve(hydration).then(async () => {
      if (cancelled) return;
      const restored = usePlayerStore.getState();
      if ((restored.driverKind as string) === "usb") {
        usePlayerStore.setState({ driverKind: "wasapi" });
      } else if (restored.driverKind === "asio") {
        usePlayerStore.setState({ driverKind: "direct" });
      }
      if (isTauriRuntime()) {
        await invoke("set_volume", { volume: restored.isMuted ? 0 : restored.volume });
      }
      if (cancelled) return;
      restored.normalizeLibrary();
      await restored.loadDevices();
      if (cancelled) return;
      await libraryEventsReady;
      if (cancelled) return;
      cancelLibraryLoad = runWhenIdle(() => {
        void usePlayerStore.getState().loadBackendLibrary();
      }, 1800);
      // 等待设备配置后重新读取，避免把用户刚修改的任务栏设置覆盖回旧值。
      const state = usePlayerStore.getState();
      // SMTC 默认在后端启用；用户此前关过则水合后同步停用状态
      if (isTauriRuntime() && !state.smtcEnabled) {
        void invoke("set_smtc_enabled", { enabled: false }).catch(() => {
          // 非 Windows 或 SMTC 未初始化时静默
        });
      }
      // 任务栏集成同理：后端默认全开，仅用户关过任一项时同步
      if (
        isTauriRuntime() &&
        (!state.taskbarButtonsEnabled || !state.taskbarProgressEnabled)
      ) {
        void invoke("set_taskbar_features", {
          buttons: state.taskbarButtonsEnabled,
          progress: state.taskbarProgressEnabled,
        }).catch(() => {
          // 非 Windows 或任务栏集成未初始化时静默
        });
      }
      // 歌词条默认关闭（后端不创建窗口），仅用户开过时创建。
      // 仅歌词模式（鼠标穿透）标志默认关，仅开过时同步——后端标志常驻，
      // 建窗时沿用；两个 invoke 并发也收敛（穿透命令会对已存在窗口直接生效）
      if (isTauriRuntime() && state.taskbarLyricsClickThrough) {
        void invoke("set_taskbar_lyrics_click_through", {
          enabled: true,
        }).catch(() => {
          // 非 Windows 时静默
        });
      }
      // 位置比例先于建窗命令同步：后端建窗时就按它落位，条不会先出现在
      // 默认位置再跳过去。命令在歌词条关着时也接受（只记比例），且若真
      // 晚到一步也只是对已建好的窗口再定位一次，自愈。
      if (isTauriRuntime()) {
        void invoke("set_taskbar_lyrics_position", {
          ratio: state.taskbarLyricsPosition,
        }).catch(() => {
          // 非 Windows 时静默
        });
      }
      if (isTauriRuntime() && state.taskbarLyricsEnabled) {
        void invoke("set_taskbar_lyrics_enabled", { enabled: true }).catch(
          () => {
            // 非 Windows 时静默
          }
        );
      }
      // v0.6.0：歌词排除规则在 Rust 侧匹配，水合后把持久化的规则同步过去
      if (isTauriRuntime() && state.lyricsExcludeRules.length > 0) {
        void invoke("set_lyrics_exclude_rules", { rules: state.lyricsExcludeRules }).catch(
          (err) => console.warn("同步歌词排除规则失败", err)
        );
      }
      // B2：显示选项（隐藏制作信息 / 忽略文件 offset）同样在 Rust 侧投影，非默认值才需同步
      if (isTauriRuntime() && (!state.showLyricsCredits || state.ignoreLyricsFileOffset)) {
        void invoke("set_lyrics_display_options", {
          options: lyricsDisplayOptionsOf(state),
        }).catch((err) => console.warn("同步歌词显示选项失败", err));
      }
    }).catch((err) => {
      if (cancelled) return;
      console.warn("Failed to initialize playback settings", err);
      usePlayerStore.getState().showNotification("播放设置初始化失败，请重新启动播放器");
    });
    // v0.4.4：EQ/DSP 配置独立持久化，水合后经 onRehydrateStorage 同步到引擎。
    void Promise.resolve(useEqStore.persist.rehydrate());
    return () => {
      cancelled = true;
      cancelLibraryLoad?.();
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);
}
