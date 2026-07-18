/**
 * DSP 链类型——与后端 seraph-dsp 的 serde 结构一一对应（camelCase）。
 * 经 set_dsp_settings IPC 下发到引擎。
 */

export type EqBandKind =
  | "peaking"
  | "lowshelf"
  | "highshelf"
  | "lowpass"
  | "highpass";

export interface EqBand {
  kind: EqBandKind;
  /** 中心/转角频率 Hz */
  freq: number;
  /** 增益 dB（低通/高通忽略） */
  gain: number;
  /** 品质因数 Q */
  q: number;
  enabled: boolean;
}

export interface CrossfeedSettings {
  enabled: boolean;
  /** 混入对侧强度 0..1 */
  amount: number;
  /** 对侧低通截止 Hz */
  cutoffHz: number;
}

export interface DspSettings {
  enabled: boolean;
  /** 预放大 dB */
  preamp: number;
  bands: EqBand[];
  crossfeed: CrossfeedSettings;
  /** EQ 是否对 DSD（已解码为 PCM）曲目生效 */
  applyToDsd: boolean;
}

/** 用户保存的 EQ 预设（前端本地持久化）。 */
export interface EqPreset {
  id: string;
  name: string;
  preamp: number;
  bands: EqBand[];
  createdAt: number;
}
