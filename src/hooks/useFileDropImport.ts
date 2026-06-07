import { useEffect, useState } from "react";
import { runWhenIdle } from "@/lib/startup";
import { isTauriRuntime } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

interface DragPayload {
  paths?: string[];
}

function getDroppedPaths(payload: unknown) {
  if (Array.isArray(payload)) {
    return payload.filter((path): path is string => typeof path === "string");
  }

  if (payload && typeof payload === "object") {
    const paths = (payload as DragPayload).paths;
    if (Array.isArray(paths)) {
      return paths.filter((path): path is string => typeof path === "string");
    }
  }

  return [];
}

export function useFileDropImport() {
  const importLocalTracks = usePlayerStore((s) => s.importLocalTracks);
  const [isDraggingFiles, setIsDraggingFiles] = useState(false);

  useEffect(() => {
    let disposed = false;
    let unlistenAll: Array<() => void> = [];

    const preventDefault = (event: DragEvent) => {
      // M-9: 只拦截 window 顶层；让输入框 / contenteditable 等元素能继续接收文本拖拽。
      const target = event.target as HTMLElement | null;
      if (target) {
        const tag = target.tagName;
        if (
          tag === "INPUT" ||
          tag === "TEXTAREA" ||
          target.isContentEditable === true
        ) {
          return;
        }
      }
      event.preventDefault();
    };

    window.addEventListener("dragover", preventDefault);
    window.addEventListener("drop", preventDefault);

    async function bindTauriDropEvents() {
      try {
        if (!isTauriRuntime()) return;

        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          if (disposed) return;

          if (event.payload.type === "enter" || event.payload.type === "over") {
            setIsDraggingFiles(true);
            return;
          }

          setIsDraggingFiles(false);

          if (event.payload.type === "drop") {
            void importLocalTracks(getDroppedPaths(event.payload));
          }
        });

        if (disposed) {
          unlisten();
          return;
        }

        unlistenAll = [unlisten];
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Failed to bind file drop events", err);
      }
    }

    const cancelDeferredBind = runWhenIdle(() => {
      void bindTauriDropEvents();
    }, 1800);

    return () => {
      disposed = true;
      cancelDeferredBind();
      window.removeEventListener("dragover", preventDefault);
      window.removeEventListener("drop", preventDefault);
      unlistenAll.forEach((unlisten) => unlisten());
    };
  }, [importLocalTracks]);

  return isDraggingFiles;
}
