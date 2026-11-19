import { invoke } from "@/lib/tauri";
import { createSerialPoller } from "@/lib/serialPoller";
import type { AnalysisFrame } from "./types";
import { decodeAnalysisFrame, decodeSpectrumFrame, type SpectrumFrame } from "./wire";

// 侧栏与分析页共用槽位，切换组件时也不会叠加在途 IPC。
export const visualizerPoller = createSerialPoller();

export function createAnalysisSession() {
  return globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random()}`;
}

export interface AnalysisDemand {
  spectrum: boolean;
  loudness: boolean;
  levels: boolean;
  field: boolean;
  scope: boolean;
}

/**
 * 以约 30 fps 轮询后端可视化帧。两条命令都返回二进制帧（VIZ-01），在这里解码；
 * 请求身份对不上或序号没前进的帧（迟到、重复）不会交给 `onFrame`。
 */
export function pollVisualizer(
  command: "get_spectrum_frame",
  sessionId: string,
  onFrame: (frame: SpectrumFrame) => void,
): () => void;
export function pollVisualizer(
  command: "get_analysis_frame",
  sessionId: string,
  onFrame: (frame: AnalysisFrame) => void,
  demand: AnalysisDemand,
): () => void;
export function pollVisualizer(
  command: "get_spectrum_frame" | "get_analysis_frame",
  sessionId: string,
  onFrame: ((frame: SpectrumFrame) => void) | ((frame: AnalysisFrame) => void),
  demand?: AnalysisDemand,
) {
  const decode = command === "get_spectrum_frame" ? decodeSpectrumFrame : decodeAnalysisFrame;
  // 重载已保证命令与回调成对；实现签名里的联合类型在这里收窄一次。
  const deliver = onFrame as (frame: SpectrumFrame | AnalysisFrame) => void;
  let lastSequence = -1;
  let failureLogged = false;
  return visualizerPoller.start({
    intervalMs: 33,
    read: async (generation) => {
      const requestId = `${sessionId}:${generation}`;
      const raw = await invoke<unknown>(command, {
        request: { sessionId, requestId },
        ...(demand ? { demand } : {}),
      });
      return { frame: decode(raw), requestId };
    },
    onData: ({ frame, requestId }) => {
      if (!frame || frame.requestId !== requestId || frame.sequence <= lastSequence) return;
      lastSequence = frame.sequence;
      deliver(frame);
    },
    onError: (error) => {
      // 取帧失败或帧解不开时仪表只是停在原地；每轮轮询记一次，不按 30 fps 刷屏。
      if (failureLogged) return;
      failureLogged = true;
      console.warn(`Tauri command failed: ${command}`, error);
    },
  });
}
