/**
 * 声学分析页共享类型与常量。
 *
 * 后端口径（src-tauri ipc/visualizer.rs）：
 * - 频谱 96 频点，0..1 归一化（-72dB 地板线性映射）
 * - 电平为线性幅度，弹道学（攻击/释放/峰值保持）在前端做
 * - 响度 LUFS 字段数据不足时为 null（前端显示 --）
 */

export const ANALYSIS_BIN_COUNT = 96;
/** 频谱归一化地板（与后端 map_bins_to_db 一致） */
export const SPECTRUM_DB_FLOOR = -72;
/** 电平表显示下限 */
export const LEVEL_DB_FLOOR = -90;
/** 瀑布缓存行数与写入间隔 */
export const HISTORY_ROWS = 64;
export const HISTORY_INTERVAL_SEC = 0.09;
/** 频轴范围（log） */
export const FREQ_MIN = 20;
export const FREQ_MAX = 20000;
/** 示波器波形 i16 量化满幅 */
export const WAVE_I16_SCALE = 32767;

/** 后端 get_analysis_frame 返回的一帧（serde camelCase） */
export interface AnalysisFrame {
  spectrum: number[];
  peakLeft: number;
  peakRight: number;
  rmsLeft: number;
  rmsRight: number;
  momentaryLufs: number | null;
  shortTermLufs: number | null;
  integratedLufs: number | null;
  loudnessRangeLu: number | null;
  truePeakDb: number | null;
  truePeakMaxDb: number | null;
  correlation: number;
  scatter: number[];
  /** 交错 L,R 的 i16 量化波形（时间正序，约 43ms 窗、每 2 帧 1 点） */
  waveform: number[];
  sampleRate: number;
}

export interface ChannelLevelView {
  /** 显示值（dBFS，已做弹道学平滑） */
  rmsDb: number;
  peakDb: number;
  holdDb: number;
  holdAt: number;
  /** VU 弹道值（dBFS；0 VU = -18 dBFS 参考，300ms 表针积分） */
  vuDb: number;
}

/** 面板显示状态：由帧摄入 + 每帧步进共同维护 */
export interface AnalysisView {
  spectrumDb: Float32Array;
  peakHoldDb: Float32Array;
  levels: {
    l: ChannelLevelView;
    r: ChannelLevelView;
    clip: boolean;
    clipAt: number;
  };
  loud: {
    m: number | null;
    s: number | null;
    i: number | null;
    lra: number | null;
    tp: number | null;
    tpMax: number | null;
    /** 响度目标（前端本地选择，默认 -14 流媒体） */
    target: number;
  };
  stereo: {
    pts: Float32Array;
    count: number;
    corr: number;
  };
  /** 示波器：拆分后的 L/R 波形（-1..1）与元数据 */
  wave: {
    l: Float32Array;
    r: Float32Array;
    points: number;
    /** 波形窗口时长（秒），由采样率与抽取步长换算 */
    windowSec: number;
  };
  history: Float32Array[];
  historyHead: number;
  /** 瀑布推进计数（每写一行 +1），离屏缓存据此判断是否需要重绘 */
  historyVersion: number;
  /** 内部时间戳（秒） */
  lastFrameAt: number;
  lastStepAt: number;
  lastHistoryAt: number;
  hasData: boolean;
}

export const linearToDb = (value: number) =>
  20 * Math.log10(Math.max(value, 1e-6));

export const clamp = (value: number, lo: number, hi: number) =>
  Math.min(hi, Math.max(lo, value));
