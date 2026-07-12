import { useEffect } from "react";
import { isTauriRuntime } from "@/lib/tauri";
import { checkForUpdate } from "@/lib/update";
import { usePlayerStore } from "@/store/player";

/**
 * 启动后静默检查一次更新（延迟 8s，避开水合与曲库加载高峰）。
 * 有新版仅弹一次通知提示到设置页查看；任何失败静默忽略。
 */
export function useUpdateCheck() {
  useEffect(() => {
    if (!isTauriRuntime()) return;

    const timer = window.setTimeout(() => {
      void checkForUpdate()
        .then((result) => {
          if (result.updateAvailable) {
            usePlayerStore
              .getState()
              .showNotification(
                `发现新版本 v${result.latestVersion}，可在设置中前往下载`
              );
          }
        })
        .catch(() => {
          // 静默：启动期网络不可用不打扰用户
        });
    }, 8000);

    return () => window.clearTimeout(timer);
  }, []);
}
