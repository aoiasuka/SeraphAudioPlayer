import { invoke } from "@/lib/tauri";
import { createSerialPoller } from "@/lib/serialPoller";

// 侧栏与分析页共用槽位，切换组件时也不会叠加在途 IPC。
export const visualizerPoller = createSerialPoller();

export function createAnalysisSession() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

interface FrameIdentity {
  requestId: string;
  sequence: number;
}

export function pollVisualizer<T>(
  command: "get_spectrum_frame" | "get_analysis_frame",
  sessionId: string,
  onFrame: (frame: T) => void,
  demand?: { spectrum: boolean; loudness: boolean; levels: boolean; field: boolean; scope: boolean },
) {
  let lastSequence = -1;
  return visualizerPoller.start({
    intervalMs: 33,
    read: async (generation) => {
      const requestId = `${sessionId}:${generation}`;
      const frame = await invoke<(T & FrameIdentity) | null>(command, {
        request: { sessionId, requestId },
        ...(demand ? { demand } : {}),
      });
      return { frame, requestId };
    },
    onData: ({ frame, requestId }) => {
      if (!frame || frame.requestId !== requestId || frame.sequence <= lastSequence) return;
      lastSequence = frame.sequence;
      onFrame(frame);
    },
  });
}
