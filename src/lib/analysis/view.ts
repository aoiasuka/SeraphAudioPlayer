/**
 * 分析面板显示状态机：帧摄入（~30fps IPC 轮询）+ 每帧步进（rAF）。
 *
 * 弹道学约定（与设计演示 design/visualizer-demo 一致）：
 * - 频谱快攻慢放（attack 24/s、release 7/s），峰值保持线 3.5 dB/s 缓落
 * - 电平 RMS 攻击 9/s 释放 2.6/s；峰值保持 1.4s 后 14 dB/s 回落
 * - CLIP 任一声道峰值 > -1.2 dBFS 置位，3s 自动解除
 * - 数据断流（暂停/停止）超过 0.25s 后所有量优雅衰减，瀑布继续走纸
 */

import {
  ANALYSIS_BIN_COUNT,
  HISTORY_INTERVAL_SEC,
  HISTORY_ROWS,
  LEVEL_DB_FLOOR,
  SPECTRUM_DB_FLOOR,
  clamp,
  linearToDb,
  type AnalysisFrame,
  type AnalysisView,
  type ChannelLevelView,
} from "./types";

const PEAK_HOLD_SECONDS = 1.4;
const PEAK_FALL_DB_PER_SEC = 14;
const CLIP_HOLD_SECONDS = 3;
const CLIP_THRESHOLD_DB = -1.2;
/** 断流判定：超过该间隔没有新帧就进入衰减模式 */
const STALE_AFTER_SEC = 0.25;

function emptyChannel(): ChannelLevelView {
  return {
    rmsDb: LEVEL_DB_FLOOR,
    peakDb: LEVEL_DB_FLOOR,
    holdDb: LEVEL_DB_FLOOR,
    holdAt: 0,
  };
}

export function createAnalysisView(): AnalysisView {
  return {
    spectrumDb: new Float32Array(ANALYSIS_BIN_COUNT).fill(SPECTRUM_DB_FLOOR),
    peakHoldDb: new Float32Array(ANALYSIS_BIN_COUNT).fill(SPECTRUM_DB_FLOOR),
    levels: { l: emptyChannel(), r: emptyChannel(), clip: false, clipAt: -10 },
    loud: {
      m: null,
      s: null,
      i: null,
      lra: null,
      tp: null,
      tpMax: null,
      target: -14,
    },
    stereo: { pts: new Float32Array(0), count: 0, corr: 0 },
    history: Array.from(
      { length: HISTORY_ROWS },
      () => new Float32Array(ANALYSIS_BIN_COUNT).fill(SPECTRUM_DB_FLOOR)
    ),
    historyHead: HISTORY_ROWS - 1,
    lastFrameAt: -10,
    lastStepAt: -10,
    lastHistoryAt: -10,
    hasData: false,
  };
}

/** 换曲目：与后端 reset_analysis_meters 配套，清掉会话累计量（积分/LRA/真峰最大） */
export function resetAnalysisSession(view: AnalysisView) {
  view.loud.i = null;
  view.loud.lra = null;
  view.loud.tp = null;
  view.loud.tpMax = null;
}

/** 摄入一帧后端数据（IPC 轮询节奏调用） */
export function applyAnalysisFrame(
  view: AnalysisView,
  frame: AnalysisFrame,
  now: number
) {
  const dt = clamp(now - (view.lastFrameAt > 0 ? view.lastFrameAt : now - 0.033), 0.001, 0.2);
  view.lastFrameAt = now;
  view.hasData = true;

  // 频谱：norm(0..1, -72 地板) → dB，快攻慢放
  const bins = frame.spectrum;
  const kUp = Math.min(1, dt * 24);
  const kDown = Math.min(1, dt * 7);
  for (let i = 0; i < view.spectrumDb.length; i += 1) {
    const norm = clamp(bins.length > i ? bins[i] : 0, 0, 1);
    const target = SPECTRUM_DB_FLOOR + norm * -SPECTRUM_DB_FLOOR;
    const k = target > view.spectrumDb[i] ? kUp : kDown;
    view.spectrumDb[i] += (target - view.spectrumDb[i]) * k;
  }

  // 电平：RMS 平滑；峰值直接置位（保持逻辑在 step 里）
  const ingestChannel = (
    channel: ChannelLevelView,
    peakLinear: number,
    rmsLinear: number
  ) => {
    const rmsDb = clamp(linearToDb(rmsLinear), LEVEL_DB_FLOOR, 0);
    const peakDb = clamp(linearToDb(peakLinear), LEVEL_DB_FLOOR, 0);
    const kRms = Math.min(1, dt * (rmsDb > channel.rmsDb ? 9 : 2.6));
    channel.rmsDb += (rmsDb - channel.rmsDb) * kRms;
    channel.peakDb = Math.max(peakDb, channel.peakDb - dt * 60);
    if (channel.peakDb > channel.holdDb) {
      channel.holdDb = channel.peakDb;
      channel.holdAt = now;
    }
  };
  ingestChannel(view.levels.l, frame.peakLeft, frame.rmsLeft);
  ingestChannel(view.levels.r, frame.peakRight, frame.rmsRight);
  const framePeakDb = Math.max(
    linearToDb(frame.peakLeft),
    linearToDb(frame.peakRight)
  );
  if (framePeakDb > CLIP_THRESHOLD_DB) {
    view.levels.clip = true;
    view.levels.clipAt = now;
  }

  // 响度：M/S 轻平滑消除 10Hz 台阶，其余直读
  const glide = (current: number | null, target: number | null) => {
    if (target === null) return current;
    if (current === null) return target;
    return current + (target - current) * Math.min(1, dt * 8);
  };
  view.loud.m = glide(view.loud.m, frame.momentaryLufs);
  view.loud.s = glide(view.loud.s, frame.shortTermLufs);
  view.loud.i = frame.integratedLufs ?? view.loud.i;
  view.loud.lra = frame.loudnessRangeLu ?? view.loud.lra;
  view.loud.tp = frame.truePeakDb ?? view.loud.tp;
  view.loud.tpMax = frame.truePeakMaxDb ?? view.loud.tpMax;

  // 声场
  if (frame.scatter.length >= 2) {
    view.stereo.pts = Float32Array.from(frame.scatter);
    view.stereo.count = frame.scatter.length / 2;
  }
  view.stereo.corr += (frame.correlation - view.stereo.corr) * Math.min(1, dt * 6);
}

/** 每帧步进：峰值保持、CLIP 超时、瀑布走纸、断流衰减（rAF 节奏调用） */
export function stepAnalysisView(view: AnalysisView, now: number) {
  const dt = clamp(now - (view.lastStepAt > 0 ? view.lastStepAt : now - 0.016), 0.001, 0.2);
  view.lastStepAt = now;
  if (!view.hasData) return;

  // 频谱峰值保持
  for (let i = 0; i < view.peakHoldDb.length; i += 1) {
    view.peakHoldDb[i] = Math.max(
      view.spectrumDb[i],
      view.peakHoldDb[i] - dt * 3.5
    );
  }

  // 电平峰值保持回落
  for (const channel of [view.levels.l, view.levels.r]) {
    if (now - channel.holdAt > PEAK_HOLD_SECONDS) {
      channel.holdDb = Math.max(
        channel.peakDb,
        channel.holdDb - dt * PEAK_FALL_DB_PER_SEC
      );
    }
  }
  if (view.levels.clip && now - view.levels.clipAt > CLIP_HOLD_SECONDS) {
    view.levels.clip = false;
  }

  // 断流（暂停/停止）：优雅衰减，瀑布继续走纸展示静音尾迹
  if (now - view.lastFrameAt > STALE_AFTER_SEC) {
    const kDecay = Math.min(1, dt * 3);
    for (let i = 0; i < view.spectrumDb.length; i += 1) {
      view.spectrumDb[i] += (SPECTRUM_DB_FLOOR - view.spectrumDb[i]) * kDecay;
    }
    for (const channel of [view.levels.l, view.levels.r]) {
      channel.rmsDb += (LEVEL_DB_FLOOR - channel.rmsDb) * kDecay;
      channel.peakDb += (LEVEL_DB_FLOOR - channel.peakDb) * kDecay;
    }
    view.stereo.corr *= 1 - Math.min(1, dt * 1.5);
  }

  // 瀑布历史（有数据后固定 90ms 一行）
  if (now - view.lastHistoryAt >= HISTORY_INTERVAL_SEC) {
    view.lastHistoryAt = now;
    view.historyHead = (view.historyHead + 1) % HISTORY_ROWS;
    view.history[view.historyHead].set(view.spectrumDb);
  }
}
