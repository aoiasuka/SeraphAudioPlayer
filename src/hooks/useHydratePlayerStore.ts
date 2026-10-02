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
      if (!isTauriRuntime()) return;
      // BUG-06：持久化设置补同步到后端。应用只跑在 Windows，失败不再以「非 Windows」为由
      // 静默——界面显示的开关与后端实际状态会分叉；汇总成一条提示并写日志。
      const syncs: Array<[label: string, request: Promise<unknown>]> = [];
      // SMTC 默认在后端启用；用户此前关过则水合后同步停用状态
      if (!state.smtcEnabled) {
        syncs.push(["系统媒体控件", invoke("set_smtc_enabled", { enabled: false })]);
      }
      // 任务栏集成同理：后端默认全开，仅用户关过任一项时同步
      if (!state.taskbarButtonsEnabled || !state.taskbarProgressEnabled) {
        syncs.push(["任务栏播控", invoke("set_taskbar_features", {
          buttons: state.taskbarButtonsEnabled,
          progress: state.taskbarProgressEnabled,
        })]);
      }
      // 歌词条默认关闭（后端不创建窗口），仅用户开过时创建。
      // 仅歌词模式（鼠标穿透）标志默认关，仅开过时同步——后端标志常驻，
      // 建窗时沿用；两个 invoke 并发也收敛（穿透命令会对已存在窗口直接生效）
      if (state.taskbarLyricsClickThrough) {
        syncs.push(["歌词条鼠标穿透", invoke("set_taskbar_lyrics_click_through", { enabled: true })]);
      }
      // 位置比例先于建窗命令同步：后端建窗时就按它落位，条不会先出现在
      // 默认位置再跳过去。命令在歌词条关着时也接受（只记比例），且若真
      // 晚到一步也只是对已建好的窗口再定位一次，自愈。
      syncs.push(["歌词条位置", invoke("set_taskbar_lyrics_position", {
        ratio: state.taskbarLyricsPosition,
      })]);
      if (state.taskbarLyricsEnabled) {
        syncs.push(["任务栏歌词条", invoke("set_taskbar_lyrics_enabled", { enabled: true })]);
      }
      // v0.6.0：歌词排除规则在 Rust 侧匹配，水合后把持久化的规则同步过去
      if (state.lyricsExcludeRules.length > 0) {
        syncs.push(["歌词排除规则", invoke("set_lyrics_exclude_rules", { rules: state.lyricsExcludeRules })]);
      }
      // B2：显示选项（隐藏制作信息 / 忽略文件 offset）同样在 Rust 侧投影，非默认值才需同步
      if (!state.showLyricsCredits || state.ignoreLyricsFileOffset) {
        syncs.push(["歌词显示选项", invoke("set_lyrics_display_options", {
          options: lyricsDisplayOptionsOf(state),
        })]);
      }
      const results = await Promise.allSettled(syncs.map(([, request]) => request));
      if (cancelled) return;
      const failed = results.flatMap((result, index) => {
        if (result.status === "fulfilled") return [];
        console.warn(`启动同步失败：${syncs[index][0]}`, result.reason);
        return [syncs[index][0]];
      });
      if (failed.length > 0) {
        usePlayerStore.getState().showNotification(
          `以下设置未能应用到播放内核：${failed.join("、")}（可在设置里重新切换）`,
          "error"
        );
      }
    }).catch((err) => {
      if (cancelled) return;
      console.warn("Failed to initialize playback settings", err);
      usePlayerStore.getState().showNotification("播放设置初始化失败，请重新启动播放器", "error");
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
