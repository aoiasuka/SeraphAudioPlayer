import { combinedResponseDb, logFreqPoints } from "@/lib/eqResponse";
import { EQ_PREAMP_LIMIT } from "@/lib/eqSanitize";
import type { EqBand } from "@/types/dsp";

/**
 * 内置曲风 EQ 预设。
 *
 * 采用 10 段标准中心频率（31/62/125/250/500/1k/2k/4k/8k/16k Hz）的参数 EQ，
 * 首尾用搁架、中间用 peaking，与图形均衡器习惯一致又保留参数化灵活度。
 * 增益为温和取值（多在 ±6dB 内）；预放大由合成响应算出（见 `headroomPreamp`），
 * 保证预设本身不会把满幅素材推过 0 dB。
 */

const CENTER_FREQS = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

/** 用增益数组构造 10 段 EQ：首段低搁架、末段高搁架、中间 peaking(Q=1.0)。 */
function bandsFromGains(gains: number[]): EqBand[] {
  return CENTER_FREQS.map((freq, index) => {
    const kind =
      index === 0
        ? "lowshelf"
        : index === CENTER_FREQS.length - 1
          ? "highshelf"
          : "peaking";
    return {
      kind,
      freq,
      gain: gains[index] ?? 0,
      q: 1.0,
      enabled: true,
    } satisfies EqBand;
  });
}

export interface GenrePreset {
  id: string;
  name: string;
  gains: number[];
}

/** 预放大按这几个采样率各算一遍取最保守的：高架滤波器的峰值随采样率变化 */
const HEADROOM_SAMPLE_RATES = [44_100, 48_000, 96_000, 192_000];
const HEADROOM_STEP_DB = 0.5;

/**
 * 抵消一组频段整体抬升所需的预放大（≤ 0，按 0.5 dB 向下取整）。
 *
 * 各段 Q=1、按倍频程排列，相邻频段互相重叠，合成响应的峰值高于任何单段的增益——
 * 此前手填的预放大（-2 ~ -6 dB）比实际峰值少 1.2 ~ 3.2 dB，引擎在均衡器之后
 * 没有限幅，满幅素材会被硬削波。
 */
export function headroomPreamp(bands: EqBand[]): number {
  return preampForPeak(responsePeakDb(bands));
}

/** 一组频段（不含预放大）在各常见采样率下合成响应的最大提升量（dB，≥ 0）。 */
function responsePeakDb(bands: EqBand[]): number {
  let peak = 0;
  for (const sampleRate of HEADROOM_SAMPLE_RATES) {
    const freqs = logFreqPoints(600, 10, Math.min(22_000, sampleRate / 2 - 1));
    for (const db of combinedResponseDb(0, bands, freqs, sampleRate)) {
      if (db > peak) peak = db;
    }
  }
  return peak;
}

function preampForPeak(peak: number): number {
  // 取整前留一点量，避免 3.0000001 这类浮点结果刚好落在台阶上
  const steps = Math.ceil((peak - 1e-6) / HEADROOM_STEP_DB);
  return steps <= 0 ? 0 : -steps * HEADROOM_STEP_DB;
}

export interface ClipRisk {
  /** 满幅素材经过当前 EQ 后是否会超过 0 dBFS */
  clipping: boolean;
  /** 合成响应（含预放大）峰值超出 0 dB 的量；不超出为 0 */
  overDb: number;
  /** 刚好不削波的预放大（≤ 0，0.5 dB 一档，不低于预放大下限） */
  suggestedPreamp: number;
}

/**
 * DSP-04：当前 EQ 设置的削波风险。
 *
 * 引擎在均衡器之后没有限幅器，输出端只有量化时的硬钳位——几个频段提升而预放大
 * 留在 0 dB 时，满幅素材会被硬削波。这里只做提示与建议，不改动出声通路：
 * 加限幅器会改变所有人的声音，而提示把选择权留给用户。
 *
 * 判定用「预放大 > 建议值」而不是直接比较峰值：建议值按 0.5 dB 取整，
 * 这样用户按建议设置之后提示一定消失。
 */
export function clipRisk(preamp: number, bands: EqBand[]): ClipRisk {
  const current = Number.isFinite(preamp) ? preamp : 0;
  const peak = responsePeakDb(bands);
  const suggestedPreamp = Math.max(-EQ_PREAMP_LIMIT, preampForPeak(peak));
  const clipping = current > suggestedPreamp + 1e-6;
  return {
    clipping,
    overDb: clipping ? Math.max(0, current + peak) : 0,
    suggestedPreamp,
  };
}

// 每个预设的 10 段增益（dB）。
const GENRE_PRESETS: GenrePreset[] = [
  { id: "flat", name: "平直（Flat）", gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  {
    id: "rock",
    name: "摇滚（Rock）",
    gains: [4.5, 3.5, 2, 0, -1, 0.5, 2, 3, 3.5, 4],
  },
  {
    id: "pop",
    name: "流行（Pop）",
    gains: [-1, 0, 1, 2.5, 3, 3, 1.5, 0, -0.5, -1],
  },
  {
    id: "jazz",
    name: "爵士（Jazz）",
    gains: [3, 2, 1, 1.5, -0.5, -0.5, 0, 1, 2, 3],
  },
  {
    id: "classical",
    name: "古典（Classical）",
    gains: [3.5, 3, 2, 1.5, -1, -1, 0, 1.5, 2.5, 3.5],
  },
  {
    id: "electronic",
    name: "电子（Electronic）",
    gains: [5, 4, 1.5, 0, -1.5, 1, 0.5, 1.5, 3.5, 4.5],
  },
  {
    id: "hiphop",
    name: "嘻哈（Hip-Hop）",
    gains: [6, 5, 2.5, 1.5, -0.5, -0.5, 0.5, 1, 2, 3],
  },
  {
    id: "vocal",
    name: "人声（Vocal）",
    gains: [-2, -1.5, -0.5, 1.5, 3.5, 3.5, 2.5, 1.5, 0, -1],
  },
  {
    id: "bass-boost",
    name: "低音增强（Bass Boost）",
    gains: [7, 6, 4, 2, 0, 0, 0, 0, 0, 0],
  },
  {
    id: "treble-boost",
    name: "高音增强（Treble Boost）",
    gains: [0, 0, 0, 0, 0, 0.5, 2, 4, 5.5, 6.5],
  },
];

export interface ResolvedGenrePreset {
  id: string;
  name: string;
  preamp: number;
  bands: EqBand[];
}

export const GENRE_EQ_PRESETS: ResolvedGenrePreset[] = GENRE_PRESETS.map(
  (preset) => {
    const bands = bandsFromGains(preset.gains);
    return { id: preset.id, name: preset.name, preamp: headroomPreamp(bands), bands };
  }
);

export const DEFAULT_EQ_BANDS: EqBand[] = bandsFromGains([
  0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]);

export { CENTER_FREQS as EQ_CENTER_FREQS };
