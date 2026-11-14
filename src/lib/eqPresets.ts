import { combinedResponseDb, logFreqPoints } from "@/lib/eqResponse";
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
  let peak = 0;
  for (const sampleRate of HEADROOM_SAMPLE_RATES) {
    const freqs = logFreqPoints(600, 10, Math.min(22_000, sampleRate / 2 - 1));
    for (const db of combinedResponseDb(0, bands, freqs, sampleRate)) {
      if (db > peak) peak = db;
    }
  }
  // 取整前留一点量，避免 3.0000001 这类浮点结果刚好落在台阶上
  const steps = Math.ceil((peak - 1e-6) / HEADROOM_STEP_DB);
  return steps <= 0 ? 0 : -steps * HEADROOM_STEP_DB;
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
