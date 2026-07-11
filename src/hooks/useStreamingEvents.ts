import { useEffect } from "react";
import { listen } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { ffmpegDownloadStateFromProgress } from "@/store/player/streamingActions";
import type { FfmpegDownloadProgress } from "@/store/player/types";

/**
 * 审2-R5：ffmpeg 下载进度监听从 StreamingPage 提升为 App 级一次性挂载。
 * MainPages 用 key={activeView} 强制卸载页面，监听放在组件里会随切页丢失，
 * 后台仍在下载时进度事件无人接收；提升后事件直接写入 store，切回页面进度仍连续。
 */
export function useStreamingEvents() {
  useEffect(() => {
    // 遵循 useFileDropImport 的竞态处理模式：disposed 标志 + resolve 后即时 unlisten
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<FfmpegDownloadProgress>("seraph://ffmpeg-download", (progress) => {
      if (disposed) return;
      usePlayerStore.setState({
        ffmpegDownload: ffmpegDownloadStateFromProgress(progress),
      });
    }).then((fn) => {
      // cleanup 已先于 listen resolve 执行时立即注销，避免监听器泄漏
      if (disposed) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);
}
