import { lazy, Suspense, useEffect, useState } from "react";
import { Sidebar } from "@/components/layout/Sidebar";
import { TitleBar } from "@/components/layout/TitleBar";
import { MainPages } from "@/components/pages/MainPages";
import { useFileDropImport } from "@/hooks/useFileDropImport";
import { useHydratePlayerStore } from "@/hooks/useHydratePlayerStore";
import { usePlayback } from "@/hooks/usePlayback";
import { useRevealWindow } from "@/hooks/useRevealWindow";
import { useStreamingEvents } from "@/hooks/useStreamingEvents";
import { useUpdateCheck } from "@/hooks/useUpdateCheck";
import { usePlayerStore } from "@/store/player";

const LazyRightPanel = lazy(() =>
  import("@/components/layout/RightPanel").then((module) => ({
    default: module.RightPanel,
  }))
);
const LazySettingsModal = lazy(() =>
  import("@/components/modal/SettingsModal").then((module) => ({
    default: module.SettingsModal,
  }))
);
const LazyDragImportOverlay = lazy(() =>
  import("@/components/modal/DragImportOverlay").then((module) => ({
    default: module.DragImportOverlay,
  }))
);
const LazyNotification = lazy(() =>
  import("@/components/modal/Notification").then((module) => ({
    default: module.Notification,
  }))
);

function App() {
  useRevealWindow();
  useHydratePlayerStore();
  usePlayback();
  // 审2-R5：ffmpeg 下载进度监听挂在 App 级，切页不丢事件
  useStreamingEvents();
  // 启动静默检查更新（一次，失败忽略）
  useUpdateCheck();
  const isDraggingFiles = useFileDropImport();
  const hasTrack = usePlayerStore((s) => s.currentTrack() !== null);
  const settingsOpen = usePlayerStore((s) => s.settingsOpen);
  const hasNotification = usePlayerStore((s) => s.notification !== null);
  const [notificationMounted, setNotificationMounted] = useState(false);

  useEffect(() => {
    // M-16：出现过通知后保持组件挂载，让退场滑出动画能完整播放，
    // 不再随 notification 置 null 而瞬间卸载导致动画从未出现。
    if (hasNotification) setNotificationMounted(true);
  }, [hasNotification]);

  return (
    <div className="h-full w-full overflow-hidden flex flex-col app-shell">
      <div className="relative w-full h-full min-h-0 min-w-0 overflow-hidden flex flex-col app-shell select-none">
        <TitleBar />

        <div className="flex-1 min-h-0 min-w-0 flex overflow-hidden z-10 relative">
          <Sidebar />
          <MainPages />
          {hasTrack && (
            <Suspense fallback={null}>
              <LazyRightPanel />
            </Suspense>
          )}
        </div>
      </div>

      {settingsOpen && (
        <Suspense fallback={null}>
          <LazySettingsModal />
        </Suspense>
      )}
      {isDraggingFiles && (
        <Suspense fallback={null}>
          <LazyDragImportOverlay visible />
        </Suspense>
      )}
      {notificationMounted && (
        <Suspense fallback={null}>
          <LazyNotification />
        </Suspense>
      )}
    </div>
  );
}

export default App;
