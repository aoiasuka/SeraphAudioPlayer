import { useEffect } from "react";
import { listen } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import {
  ffmpegDownloadStateFromProgress,
  shouldIgnoreLaggingFfmpegProgress,
} from "@/store/player/streamingActions";
import type {
  BilibiliBatchProgress,
  FfmpegDownloadProgress,
} from "@/store/player/types";

/**
 * 审2-R5：流媒体相关事件监听从 StreamingPage 提升为 App 级一次性挂载。
 * MainPages 用 key={activeView} 强制卸载页面，监听放在组件里会随切页丢失，
 * 后台仍在下载时进度事件无人接收；提升后事件直接写入 store，切回页面进度仍连续。
 */
export function useStreamingEvents() {
  useEffect(() => {
    // 遵循 useFileDropImport 的竞态处理模式：disposed 标志 + resolve 后即时 unlisten
    let disposed = false;
    const unlisteners: Array<() => void> = [];

    const register = <T,>(event: string, handler: (payload: T) => void) => {
      // L-18：listen Promise 补 catch——注册失败（非 Tauri 环境等）不再是
      // unhandled rejection
      void listen<T>(event, (payload) => {
        if (disposed) return;
        handler(payload);
      })
        .then((fn) => {
          // cleanup 已先于 listen resolve 执行时立即注销，避免监听器泄漏
          if (disposed) {
            fn();
            return;
          }
          unlisteners.push(fn);
        })
        .catch(() => {
          // 注册失败静默；对应功能退化为无进度显示
        });
    };

    register<FfmpegDownloadProgress>("seraph://ffmpeg-download", (progress) => {
      const next = ffmpegDownloadStateFromProgress(progress);
      // M-6：invoke 已落定终态后，忽略滞后到达的非终态（downloading）事件，
      // 避免把 done/error 打回 downloading 导致按钮永久卡 spinner。终态事件仍放行。
      if (next.stage === "downloading" && shouldIgnoreLaggingFfmpegProgress()) {
        return;
      }
      usePlayerStore.setState({ ffmpegDownload: next });
    });

    // M-15：收藏夹批量导入进度。仅在导入中（progress 非 null）时更新，
    // 避免结果落定清空后又被滞后事件复活。
    register<BilibiliBatchProgress>("seraph://bilibili-batch", (progress) => {
      if (usePlayerStore.getState().bilibiliBatchProgress === null) return;
      usePlayerStore.setState({ bilibiliBatchProgress: progress });
    });

    // 任务栏歌词条 ✕ 按钮：设置的单一事实来源在主窗口 store（歌词条窗口
    // 不允许 import persist store），经事件回传由这里落开关并销毁窗口。
    register<null>("seraph://taskbar-lyrics-close", () => {
      usePlayerStore.getState().setTaskbarLyricsEnabled(false);
    });

    // 歌词条被拖拽后回传的位置比例：设置页滑块与它是同一个字段，拖完
    // 滑块就跟着走。setter 自身按值去重，回传与滑块不会互相打架。
    register<number>("seraph://taskbar-lyrics-position", (ratio) => {
      if (typeof ratio !== "number" || !Number.isFinite(ratio)) return;
      usePlayerStore.getState().setTaskbarLyricsPosition(ratio);
    });

    return () => {
      disposed = true;
      for (const unlisten of unlisteners) unlisten();
    };
  }, []);
}
