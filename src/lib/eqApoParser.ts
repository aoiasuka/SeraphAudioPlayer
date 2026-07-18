import type { EqBand, EqBandKind } from "@/types/dsp";

/**
 * AutoEq / EqualizerAPO 预设解析。
 *
 * 支持两种最常见格式：
 * 1) EqualizerAPO ParametricEQ（AutoEq 的 `ParametricEQ.txt`）：
 *      Preamp: -6.0 dB
 *      Filter 1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.70
 *      Filter 2: ON LSC Fc 105 Hz Gain 5.5 dB Q 0.70
 *    类型：PK(peaking) / LSC(lowshelf) / HSC(highshelf) / LP(lowpass) / HP(highpass) /
 *          LPQ/HPQ(带 Q 的低/高通)。忽略 OFF 行与无法识别的类型。
 * 2) GraphicEQ（AutoEq 的 `GraphicEQ.txt`）：
 *      GraphicEQ: 20 -1.5; 25 -1.6; 31 -1.8; ...
 *    转换为一组固定频率的 peaking（Q 由相邻频率间隔估算）。
 */

export interface ParsedEqPreset {
  preamp: number;
  bands: EqBand[];
  /** 供 UI 提示的告警（如遇到无法识别的行） */
  warnings: string[];
}

const APO_TYPE_MAP: Record<string, EqBandKind | undefined> = {
  PK: "peaking",
  PEQ: "peaking",
  LSC: "lowshelf",
  LS: "lowshelf",
  HSC: "highshelf",
  HS: "highshelf",
  LP: "lowpass",
  LPQ: "lowpass",
  HP: "highpass",
  HPQ: "highpass",
};

function parsePreampLine(line: string): number | null {
  // "Preamp: -6.5 dB"
  const match = line.match(/Preamp:\s*(-?\d+(?:\.\d+)?)\s*dB/i);
  return match ? Number.parseFloat(match[1]) : null;
}

function parseFilterLine(line: string): EqBand | null {
  // "Filter 1: ON PK Fc 105 Hz Gain 5.5 dB Q 0.70"
  const on = /Filter\s+\d+:\s*ON\s+/i.test(line);
  if (!on) return null;

  const typeMatch = line.match(/ON\s+([A-Z]+)\s/i);
  if (!typeMatch) return null;
  const kind = APO_TYPE_MAP[typeMatch[1].toUpperCase()];
  if (!kind) return null;

  const fcMatch = line.match(/Fc\s+(-?\d+(?:\.\d+)?)\s*Hz/i);
  if (!fcMatch) return null;
  const freq = Number.parseFloat(fcMatch[1]);
  if (!Number.isFinite(freq) || freq <= 0) return null;

  const gainMatch = line.match(/Gain\s+(-?\d+(?:\.\d+)?)\s*dB/i);
  const gain = gainMatch ? Number.parseFloat(gainMatch[1]) : 0;

  const qMatch = line.match(/Q\s+(-?\d+(?:\.\d+)?)/i);
  const q = qMatch ? Number.parseFloat(qMatch[1]) : 0.707;

  return {
    kind,
    freq,
    gain: Number.isFinite(gain) ? gain : 0,
    q: Number.isFinite(q) && q > 0 ? q : 0.707,
    enabled: true,
  };
}

function parseGraphicEq(line: string): EqBand[] {
  // "GraphicEQ: 20 -1.5; 25 -1.6; ..."
  const payload = line.slice(line.indexOf(":") + 1).trim();
  const points = payload
    .split(";")
    .map((entry) => entry.trim())
    .filter(Boolean)
    .map((entry) => {
      const [freqStr, gainStr] = entry.split(/\s+/);
      return {
        freq: Number.parseFloat(freqStr),
        gain: Number.parseFloat(gainStr),
      };
    })
    .filter((point) => Number.isFinite(point.freq) && Number.isFinite(point.gain));

  // GraphicEQ 点数可能很多（AutoEq 常 100+ 点）；每个点转成一个 peaking，
  // Q 用相邻点的频率比估算（半个八度间隔 → Q≈2.9）。上层会做数量提示。
  return points.map((point, index) => {
    const prev = points[index - 1]?.freq ?? point.freq / 1.12;
    const next = points[index + 1]?.freq ?? point.freq * 1.12;
    const bandwidthOctaves = Math.max(
      0.05,
      Math.log2(Math.max(next, 1) / Math.max(prev, 1)) / 2
    );
    // Q = sqrt(2^bw) / (2^bw - 1)
    const factor = 2 ** bandwidthOctaves;
    const q = Math.sqrt(factor) / (factor - 1);
    return {
      kind: "peaking" as const,
      freq: point.freq,
      gain: point.gain,
      q: Number.isFinite(q) && q > 0 ? Math.min(q, 10) : 1,
      enabled: true,
    } satisfies EqBand;
  });
}

/** 上限，防止异常文件生成上千个频段拖垮 UI/引擎。 */
const MAX_PARSED_BANDS = 60;

/**
 * 解析 AutoEq/EqualizerAPO 文本。识别不了任何频段时抛错（供上层提示）。
 */
export function parseApoPreset(text: string): ParsedEqPreset {
  const warnings: string[] = [];
  let preamp = 0;
  const bands: EqBand[] = [];

  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;

    if (/^Preamp:/i.test(line)) {
      const value = parsePreampLine(line);
      if (value !== null) preamp = value;
      continue;
    }
    if (/^GraphicEQ:/i.test(line)) {
      const graphicBands = parseGraphicEq(line);
      if (graphicBands.length > 0) {
        bands.push(...graphicBands);
      } else {
        warnings.push("GraphicEQ 行解析为空");
      }
      continue;
    }
    if (/^Filter\s+\d+:/i.test(line)) {
      const band = parseFilterLine(line);
      if (band) bands.push(band);
      // OFF 行与无法识别类型静默跳过（AutoEq 常含占位的 OFF 行）
      continue;
    }
    // 其它未知行忽略
  }

  if (bands.length === 0) {
    throw new Error("未能从文件中解析出任何 EQ 频段（不是有效的 AutoEq / EqualizerAPO 预设）");
  }

  let finalBands = bands;
  if (bands.length > MAX_PARSED_BANDS) {
    warnings.push(
      `预设含 ${bands.length} 个频段，已截断至前 ${MAX_PARSED_BANDS} 个`
    );
    finalBands = bands.slice(0, MAX_PARSED_BANDS);
  }

  return { preamp, bands: finalBands, warnings };
}

/**
 * 导出为 EqualizerAPO ParametricEQ 文本（与 AutoEq 输出兼容，可回导入 APO/其它播放器）。
 */
export function toApoText(preamp: number, bands: EqBand[]): string {
  const apoTypeReverse: Record<EqBandKind, string> = {
    peaking: "PK",
    lowshelf: "LSC",
    highshelf: "HSC",
    lowpass: "LP",
    highpass: "HP",
  };
  const lines = [`Preamp: ${preamp.toFixed(1)} dB`];
  bands.forEach((band, index) => {
    const state = band.enabled ? "ON" : "OFF";
    lines.push(
      `Filter ${index + 1}: ${state} ${apoTypeReverse[band.kind]} Fc ${band.freq.toFixed(
        0
      )} Hz Gain ${band.gain.toFixed(1)} dB Q ${band.q.toFixed(2)}`
    );
  });
  return lines.join("\n");
}
