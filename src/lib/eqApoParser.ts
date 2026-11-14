import { combinedResponseDb } from "@/lib/eqResponse";
import { EQ_GAIN_LIMIT, MAX_EQ_BANDS } from "@/lib/eqSanitize";
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
 *    转换为一组 peaking（Q 由相邻频率间隔估算）；超过 31 个点时在整个频率范围内
 *    重采样到 31 段，各段增益按合成响应迭代拟合原曲线。
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
  // "Preamp: -6.5 dB"（兼容 "+5.5" 的显式正号前缀）
  const match = line.match(/Preamp:\s*([+-]?\d+(?:\.\d+)?)\s*dB/i);
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

  const fcMatch = line.match(/Fc\s+([+-]?\d+(?:\.\d+)?)\s*Hz/i);
  if (!fcMatch) return null;
  const freq = Number.parseFloat(fcMatch[1]);
  if (!Number.isFinite(freq) || freq <= 0) return null;

  // L-24：正则接受 "+5.5" 前缀——部分工具导出的正增益带显式加号，
  // 旧正则匹配不到会静默当 0（导入“成功”但无效果）
  const gainMatch = line.match(/Gain\s+([+-]?\d+(?:\.\d+)?)\s*dB/i);
  const gain = gainMatch ? Number.parseFloat(gainMatch[1]) : 0;

  const qMatch = line.match(/Q\s+([+-]?\d+(?:\.\d+)?)/i);
  const q = qMatch ? Number.parseFloat(qMatch[1]) : 0.707;

  return {
    kind,
    freq,
    gain: Number.isFinite(gain) ? gain : 0,
    q: Number.isFinite(q) && q > 0 ? q : 0.707,
    enabled: true,
  };
}

/** GraphicEQ 曲线重采样后的频段数：31 段，约 1/3 倍频程间隔。 */
export const GRAPHIC_EQ_MAX_BANDS = 31;
/** 重采样时频段中心的上限（见 resampleGraphicPoints）。 */
const GRAPHIC_EQ_TOP_HZ = 16_000;

interface GraphicPoint {
  freq: number;
  gain: number;
}

/** 在对数频率轴上对曲线线性插值（范围外取端点值）。 */
function interpolateGraphicGain(points: GraphicPoint[], freq: number): number {
  if (freq <= points[0].freq) return points[0].gain;
  const last = points[points.length - 1];
  if (freq >= last.freq) return last.gain;
  let lo = 0;
  let hi = points.length - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (points[mid].freq <= freq) lo = mid;
    else hi = mid;
  }
  const left = points[lo];
  const right = points[hi];
  const span = Math.log(right.freq / left.freq);
  if (!(span > 0)) return left.gain;
  const t = Math.log(freq / left.freq) / span;
  return left.gain + (right.gain - left.gain) * t;
}

/**
 * EQ4-01：点数超限时在**整个频率范围**内按对数等间隔重采样，而不是截断头部——
 * AutoEq 的 GraphicEQ 有一百多个从低到高排列的点，保留前 N 个等于只校正低频。
 */
function resampleGraphicPoints(points: GraphicPoint[], count: number): GraphicPoint[] {
  if (points.length <= count) return points;
  const logMin = Math.log(points[0].freq);
  // 频段中心不超过 16 kHz：44.1 kHz 下 20 kHz 已贴着奈奎斯特（22.05 kHz），
  // 那里的峰化滤波器被压得极窄、基本不起作用，放一个频段只会让拟合去追一个追不上的点。
  const top = Math.max(points[0].freq, Math.min(points[points.length - 1].freq, GRAPHIC_EQ_TOP_HZ));
  const logMax = Math.log(top);
  const result: GraphicPoint[] = [];
  for (let i = 0; i < count; i += 1) {
    const freq = Math.exp(logMin + ((logMax - logMin) * i) / (count - 1));
    result.push({ freq, gain: interpolateGraphicGain(points, freq) });
  }
  return result;
}

function clampGain(gain: number): number {
  return Math.max(-EQ_GAIN_LIMIT, Math.min(EQ_GAIN_LIMIT, gain));
}

/** 高斯消元解 n×n 线性方程组（列主元）；奇异时返回 null。 */
function solveLinear(matrix: number[][], rhs: number[]): number[] | null {
  const n = rhs.length;
  const a = matrix.map((row, index) => [...row, rhs[index]]);
  for (let col = 0; col < n; col += 1) {
    let pivot = col;
    for (let row = col + 1; row < n; row += 1) {
      if (Math.abs(a[row][col]) > Math.abs(a[pivot][col])) pivot = row;
    }
    if (Math.abs(a[pivot][col]) < 1e-12) return null;
    [a[col], a[pivot]] = [a[pivot], a[col]];
    for (let row = col + 1; row < n; row += 1) {
      const factor = a[row][col] / a[col][col];
      if (factor === 0) continue;
      for (let k = col; k <= n; k += 1) a[row][k] -= factor * a[col][k];
    }
  }
  const x = new Array<number>(n).fill(0);
  for (let row = n - 1; row >= 0; row -= 1) {
    let sum = a[row][n];
    for (let k = row + 1; k < n; k += 1) sum -= a[row][k] * x[k];
    x[row] = sum / a[row][row];
  }
  return x.every(Number.isFinite) ? x : null;
}

/** 拟合时每个频段之间加密的探测点数（含端点一侧）。 */
const FIT_PROBES_PER_BAND = 4;
/** 拟合时同时考虑的采样率（见 fitGraphicGains）。 */
const FIT_SAMPLE_RATES = [44_100, 96_000];

/**
 * EQ4-02：相邻峰化滤波器的作用范围互相重叠，直接把各点增益当滤波器增益，
 * 级联后的实际响应会偏离原曲线（一段平坦的 −5 dB 会被叠成更深的衰减）。
 *
 * 峰化滤波器的 dB 响应对自身增益近似线性，所以用带阻尼的最小二乘（高斯-牛顿）求各段增益：
 * 在比频段更密的探测频点上，每轮数值求出「第 j 段增益变化 1 dB 时各探测点响应的变化」
 * 矩阵 A，解 (AᵀA + λI)·Δ = Aᵀ·误差。探测点要比频段密——只在频段中心拟合的话，
 * 中心之间（尤其靠近奈奎斯特、滤波器被压窄的高频）会留下几 dB 的起伏。
 * 逐点修正（雅可比迭代）在滤波器重叠较多时不收敛，不能用。
 */
function fitGraphicGains(bands: EqBand[], curve: GraphicPoint[]): void {
  const n = bands.length;
  if (n === 0) return;
  const probes: number[] = [];
  for (let i = 0; i < n; i += 1) {
    probes.push(bands[i].freq);
    if (i + 1 < n) {
      const ratio = bands[i + 1].freq / bands[i].freq;
      for (let k = 1; k < FIT_PROBES_PER_BAND; k += 1) {
        probes.push(bands[i].freq * ratio ** (k / FIT_PROBES_PER_BAND));
      }
    }
  }
  const probeTargets = probes.map((freq) => clampGain(interpolateGraphicGain(curve, freq)));
  // 数字峰化滤波器靠近奈奎斯特时会被压窄，同一组系数在 44.1 kHz 与 96 kHz 下的高频响应
  // 不一样。把两种采样率的误差叠在一起求解，得到在常见采样率下都不离谱的折中，
  // 而不是只对某一个采样率精确。
  const responseAt = () =>
    FIT_SAMPLE_RATES.flatMap((sampleRate) => combinedResponseDb(0, bands, probes, sampleRate));
  const targets = FIT_SAMPLE_RATES.flatMap(() => probeTargets);
  const m = targets.length;
  const lambda = 1e-3;
  for (let iteration = 0; iteration < 8; iteration += 1) {
    const response = responseAt();
    const error = targets.map((target, index) => target - response[index]);

    // jacobian[i][j] = ∂响应(探测点 i) / ∂增益(频段 j)
    const jacobian: number[][] = Array.from({ length: m }, () => new Array<number>(n).fill(0));
    for (let j = 0; j < n; j += 1) {
      const original = bands[j].gain;
      const step = original >= EQ_GAIN_LIMIT - 0.5 ? -0.5 : 0.5;
      bands[j].gain = original + step;
      const bumped = responseAt();
      bands[j].gain = original;
      for (let i = 0; i < m; i += 1) jacobian[i][j] = (bumped[i] - response[i]) / step;
    }

    const normal: number[][] = Array.from({ length: n }, () => new Array<number>(n).fill(0));
    const rhs = new Array<number>(n).fill(0);
    for (let r = 0; r < n; r += 1) {
      for (let c = r; c < n; c += 1) {
        let sum = 0;
        for (let i = 0; i < m; i += 1) sum += jacobian[i][r] * jacobian[i][c];
        normal[r][c] = sum + (r === c ? lambda : 0);
        normal[c][r] = normal[r][c];
      }
      let sum = 0;
      for (let i = 0; i < m; i += 1) sum += jacobian[i][r] * error[i];
      rhs[r] = sum;
    }
    const delta = solveLinear(normal, rhs);
    if (!delta) break;
    let largest = 0;
    for (let j = 0; j < n; j += 1) {
      largest = Math.max(largest, Math.abs(delta[j]));
      bands[j].gain = clampGain(bands[j].gain + delta[j]);
    }
    if (largest < 0.01) break;
  }
}

function parseGraphicEq(line: string): { bands: EqBand[]; sourcePoints: number } {
  // "GraphicEQ: 20 -1.5; 25 -1.6; ..."
  const payload = line.slice(line.indexOf(":") + 1).trim();
  const parsed = payload
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
    .filter(
      (point) =>
        Number.isFinite(point.freq) &&
        // L-24：GraphicEQ 点必须为正频率（0/负值进 band 会靠上下游钳制兜底成直通）
        point.freq > 0 &&
        Number.isFinite(point.gain)
    )
    .sort((left, right) => left.freq - right.freq);
  // 同频率的重复点只留第一个（插值与 Q 估算都要求频率严格递增）
  const source: GraphicPoint[] = [];
  for (const point of parsed) {
    if (source.length === 0 || point.freq > source[source.length - 1].freq) source.push(point);
  }
  if (source.length === 0) return { bands: [], sourcePoints: 0 };

  const points = resampleGraphicPoints(source, GRAPHIC_EQ_MAX_BANDS);
  // 每个点一个 peaking，Q 用相邻点的频率比估算。
  const bands = points.map((point, index) => {
    const prev = points[index - 1]?.freq ?? point.freq / 1.12;
    const next = points[index + 1]?.freq ?? point.freq * 1.12;
    // 带宽取到相邻两点的整段跨度（1/3 倍频程间隔 → Q≈2.1）：滤波器重叠多一些，
    // 中心之间不留缝；重叠带来的叠加由下面的最小二乘拟合抵消。
    const bandwidthOctaves = Math.max(0.05, Math.log2(Math.max(next, 1) / Math.max(prev, 1)));
    // Q = sqrt(2^bw) / (2^bw - 1)
    const factor = 2 ** bandwidthOctaves;
    const q = Math.sqrt(factor) / (factor - 1);
    // GraphicEQ 曲线在首尾两点之外保持端点值。峰化滤波器离开中心后会回到 0 dB，
    // 撑不住这段平台，所以两端各用一个搁架滤波器（点数太少时不值得，仍全用峰化）。
    const shelf = points.length >= 3 && (index === 0 || index === points.length - 1);
    const kind: EqBandKind = !shelf ? "peaking" : index === 0 ? "lowshelf" : "highshelf";
    return {
      kind,
      freq: Math.round(point.freq * 10) / 10,
      gain: clampGain(point.gain),
      q: shelf ? 0.707 : Number.isFinite(q) && q > 0 ? Math.min(q, 10) : 1,
      enabled: true,
    } satisfies EqBand;
  });
  fitGraphicGains(bands, source);
  for (const band of bands) band.gain = Math.round(band.gain * 100) / 100;
  return { bands, sourcePoints: source.length };
}

/** 上限，防止异常文件生成上千个频段拖垮 UI/引擎（与消毒、手动添加、后端同一个值）。 */
const MAX_PARSED_BANDS = MAX_EQ_BANDS;

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
      const graphic = parseGraphicEq(line);
      if (graphic.bands.length > 0) {
        for (const band of graphic.bands) bands.push(band);
        if (graphic.sourcePoints > graphic.bands.length) {
          warnings.push(
            `GraphicEQ 含 ${graphic.sourcePoints} 个点，已在全频段重采样为 ${graphic.bands.length} 段并拟合原曲线`
          );
        }
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
  // L-24：LP/HP 导出写 LPQ/HPQ——APO 的 LP/HP 是固定 Q、会忽略行尾 Q 参数，
  // 自定义 Q 的低/高通回导入其它播放器时形状会变；LPQ/HPQ 才携带 Q。
  const apoTypeReverse: Record<EqBandKind, string> = {
    peaking: "PK",
    lowshelf: "LSC",
    highshelf: "HSC",
    lowpass: "LPQ",
    highpass: "HPQ",
  };
  const lines = [`Preamp: ${preamp.toFixed(1)} dB`];
  bands.forEach((band, index) => {
    const state = band.enabled ? "ON" : "OFF";
    lines.push(
      // 频率保留一位小数（31.5 Hz 不再写成 32 Hz），整数不带小数点
      `Filter ${index + 1}: ${state} ${apoTypeReverse[band.kind]} Fc ${Number(
        band.freq.toFixed(1)
      )} Hz Gain ${band.gain.toFixed(1)} dB Q ${band.q.toFixed(2)}`
    );
  });
  return lines.join("\n");
}
