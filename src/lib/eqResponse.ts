import type { EqBand } from "@/types/dsp";

/**
 * 前端 EQ 频响曲线：与后端 seraph-dsp 的 RBJ biquad 幅频公式同构，
 * 用于设置页实时绘制合成响应（后端计算音频、前端仅用于可视化）。
 */

interface Biquad {
  b0: number;
  b1: number;
  b2: number;
  a1: number;
  a2: number;
}

function identity(): Biquad {
  return { b0: 1, b1: 0, b2: 0, a1: 0, a2: 0 };
}

function designBiquad(band: EqBand, sampleRate: number): Biquad {
  const nyquist = sampleRate * 0.5;
  const freq = Math.max(1, Math.min(band.freq, nyquist - 1));
  if (freq >= nyquist) return identity();
  const q = Number.isFinite(band.q) && band.q > 1e-3 ? band.q : 0.707;
  const gainDb = Number.isFinite(band.gain) ? band.gain : 0;

  const a = 10 ** (gainDb / 40);
  const w0 = (2 * Math.PI * freq) / sampleRate;
  const cos = Math.cos(w0);
  const sin = Math.sin(w0);
  const alpha = sin / (2 * q);

  let b0: number, b1: number, b2: number, a0: number, a1: number, a2: number;
  switch (band.kind) {
    case "peaking":
      b0 = 1 + alpha * a;
      b1 = -2 * cos;
      b2 = 1 - alpha * a;
      a0 = 1 + alpha / a;
      a1 = -2 * cos;
      a2 = 1 - alpha / a;
      break;
    case "lowshelf": {
      const s = 2 * Math.sqrt(a) * alpha;
      b0 = a * (a + 1 - (a - 1) * cos + s);
      b1 = 2 * a * (a - 1 - (a + 1) * cos);
      b2 = a * (a + 1 - (a - 1) * cos - s);
      a0 = a + 1 + (a - 1) * cos + s;
      a1 = -2 * (a - 1 + (a + 1) * cos);
      a2 = a + 1 + (a - 1) * cos - s;
      break;
    }
    case "highshelf": {
      const s = 2 * Math.sqrt(a) * alpha;
      b0 = a * (a + 1 + (a - 1) * cos + s);
      b1 = -2 * a * (a - 1 + (a + 1) * cos);
      b2 = a * (a + 1 + (a - 1) * cos - s);
      a0 = a + 1 - (a - 1) * cos + s;
      a1 = 2 * (a - 1 - (a + 1) * cos);
      a2 = a + 1 - (a - 1) * cos - s;
      break;
    }
    case "lowpass":
      b0 = (1 - cos) / 2;
      b1 = 1 - cos;
      b2 = (1 - cos) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cos;
      a2 = 1 - alpha;
      break;
    case "highpass":
      b0 = (1 + cos) / 2;
      b1 = -(1 + cos);
      b2 = (1 + cos) / 2;
      a0 = 1 + alpha;
      a1 = -2 * cos;
      a2 = 1 - alpha;
      break;
    default:
      return identity();
  }
  if (Math.abs(a0) < 1e-12) return identity();
  return { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0 };
}

function magnitudeDb(bq: Biquad, freq: number, sampleRate: number): number {
  const w = (2 * Math.PI * freq) / sampleRate;
  const cos1 = Math.cos(w);
  const sin1 = Math.sin(w);
  const cos2 = Math.cos(2 * w);
  const sin2 = Math.sin(2 * w);
  const numRe = bq.b0 + bq.b1 * cos1 + bq.b2 * cos2;
  const numIm = -(bq.b1 * sin1 + bq.b2 * sin2);
  const denRe = 1 + bq.a1 * cos1 + bq.a2 * cos2;
  const denIm = -(bq.a1 * sin1 + bq.a2 * sin2);
  const num = Math.hypot(numRe, numIm);
  const den = Math.max(Math.hypot(denRe, denIm), 1e-12);
  return 20 * Math.log10(num / den);
}

/** 合成响应（含 preamp），返回给定频点数组的 dB 值。 */
export function combinedResponseDb(
  preamp: number,
  bands: EqBand[],
  freqs: number[],
  sampleRate = 48_000
): number[] {
  const active = bands.filter((band) => band.enabled);
  const coeffs = active.map((band) => designBiquad(band, sampleRate));
  return freqs.map((freq) => {
    let total = Number.isFinite(preamp) ? preamp : 0;
    for (const bq of coeffs) total += magnitudeDb(bq, freq, sampleRate);
    return total;
  });
}

/** 对数分布的频率采样点（用于曲线横轴）。 */
export function logFreqPoints(count: number, min = 20, max = 20_000): number[] {
  const points: number[] = [];
  const logMin = Math.log10(min);
  const logMax = Math.log10(max);
  for (let i = 0; i < count; i += 1) {
    points.push(10 ** (logMin + ((logMax - logMin) * i) / (count - 1)));
  }
  return points;
}
