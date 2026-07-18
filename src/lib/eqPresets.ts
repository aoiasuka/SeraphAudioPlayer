import type { EqBand } from "@/types/dsp";

/**
 * 内置曲风 EQ 预设。
 *
 * 采用 10 段标准中心频率（31/62/125/250/500/1k/2k/4k/8k/16k Hz）的参数 EQ，
 * 首尾用搁架、中间用 peaking，与图形均衡器习惯一致又保留参数化灵活度。
 * 增益为温和取值（多在 ±6dB 内），避免预设即削波。
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
  preamp: number;
  gains: number[];
}

// 每个预设的 10 段增益（dB）。preamp 取负值抵消整体抬升，避免削波。
const GENRE_PRESETS: GenrePreset[] = [
  { id: "flat", name: "平直（Flat）", preamp: 0, gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  {
    id: "rock",
    name: "摇滚（Rock）",
    preamp: -4,
    gains: [4.5, 3.5, 2, 0, -1, 0.5, 2, 3, 3.5, 4],
  },
  {
    id: "pop",
    name: "流行（Pop）",
    preamp: -3,
    gains: [-1, 0, 1, 2.5, 3, 3, 1.5, 0, -0.5, -1],
  },
  {
    id: "jazz",
    name: "爵士（Jazz）",
    preamp: -2,
    gains: [3, 2, 1, 1.5, -0.5, -0.5, 0, 1, 2, 3],
  },
  {
    id: "classical",
    name: "古典（Classical）",
    preamp: -2,
    gains: [3.5, 3, 2, 1.5, -1, -1, 0, 1.5, 2.5, 3.5],
  },
  {
    id: "electronic",
    name: "电子（Electronic）",
    preamp: -4,
    gains: [5, 4, 1.5, 0, -1.5, 1, 0.5, 1.5, 3.5, 4.5],
  },
  {
    id: "hiphop",
    name: "嘻哈（Hip-Hop）",
    preamp: -5,
    gains: [6, 5, 2.5, 1.5, -0.5, -0.5, 0.5, 1, 2, 3],
  },
  {
    id: "vocal",
    name: "人声（Vocal）",
    preamp: -3,
    gains: [-2, -1.5, -0.5, 1.5, 3.5, 3.5, 2.5, 1.5, 0, -1],
  },
  {
    id: "bass-boost",
    name: "低音增强（Bass Boost）",
    preamp: -6,
    gains: [7, 6, 4, 2, 0, 0, 0, 0, 0, 0],
  },
  {
    id: "treble-boost",
    name: "高音增强（Treble Boost）",
    preamp: -4,
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
  (preset) => ({
    id: preset.id,
    name: preset.name,
    preamp: preset.preamp,
    bands: bandsFromGains(preset.gains),
  })
);

export const DEFAULT_EQ_BANDS: EqBand[] = bandsFromGains([
  0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
]);

export { CENTER_FREQS as EQ_CENTER_FREQS };
