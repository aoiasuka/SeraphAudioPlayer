/**
 * 纯浏览器（stub）模式的模拟信号源。
 *
 * 桌面运行时分析页由后端真实音频驱动；`npm run dev` 纯浏览器没有音频后端，
 * 用与设计演示（design/visualizer-demo）同源的"音乐感"合成器生成
 * AnalysisFrame，保证页面在纯前端迭代时可预览、测试可运行。
 */

import {
  ANALYSIS_BIN_COUNT,
  SPECTRUM_DB_FLOOR,
  clamp,
  type AnalysisFrame,
} from "./types";

const BPM = 96;
const SCATTER_PAIRS = 150;
const WAVE_POINTS = 1024;

function mulberry32(seed: number) {
  let a = seed >>> 0;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const gauss = (x: number, mu: number, sigma: number) => {
  const d = (x - mu) / sigma;
  return Math.exp(-0.5 * d * d);
};

const BIN_LOGF = Float32Array.from({ length: ANALYSIS_BIN_COUNT }, (_, i) =>
  Math.log10(20 * Math.pow(1000, i / (ANALYSIS_BIN_COUNT - 1)))
);

export interface AnalysisSimulator {
  next(dt: number): AnalysisFrame;
}

export function createAnalysisSimulator(seed = 20260718): AnalysisSimulator {
  const rng = mulberry32(seed);
  const TAU = Math.PI * 2;
  let t = 0;
  const jitter = new Float32Array(ANALYSIS_BIN_COUNT);
  const spectrumDb = new Float32Array(ANALYSIS_BIN_COUNT).fill(SPECTRUM_DB_FLOOR);
  let integrated = -16.2;
  let momentary = -23;
  let shortTerm = -23;
  let tpMax = -8;

  return {
    next(dt: number): AnalysisFrame {
      t += dt;
      const beatF = (t * BPM) / 60;
      const bar = Math.floor(beatF / 4);
      const beat = Math.floor(beatF) % 4;
      const bp = beatF % 1;
      const kick = Math.exp(-7 * bp);
      const snare = beat === 1 || beat === 3 ? Math.exp(-10 * bp) : 0;
      const hat = Math.exp(-15 * ((beatF * 2) % 1)) * (Math.floor(beatF * 2) % 2 ? 0.5 : 0.9);
      const roots = [55, 55, 73.42, 61.74];
      const root = roots[bar % 4];
      const bassAmp = clamp(0.55 + 0.3 * Math.sin(t * 0.9) + 0.15 * kick, 0.1, 1);
      const melF = 740 * Math.pow(2, 0.8 * Math.sin(t * 0.21) + 0.3 * Math.sin(t * 0.047));
      const melAmp = clamp(0.45 + 0.4 * Math.sin(t * 0.31 + 1), 0.05, 1);
      const pad = 0.5 + 0.5 * Math.sin(t * 0.13);

      const lin2db = (x: number) => 20 * Math.log10(Math.max(x, 1e-5));
      const kickDb = -13 + lin2db(kick);
      const bassDb = -16 + lin2db(bassAmp);
      const snareDb = -18 + lin2db(snare + 1e-4);
      const hatDb = -22 + lin2db(hat + 1e-4);
      const padDb = -30 + lin2db(pad + 1e-3);
      const melDb = -23 + lin2db(melAmp);
      const lRoot = Math.log10(root);
      const lMel = Math.log10(melF);

      const spectrum: number[] = new Array(ANALYSIS_BIN_COUNT);
      for (let i = 0; i < ANALYSIS_BIN_COUNT; i += 1) {
        const lf = BIN_LOGF[i];
        const base = -39 - 15.5 * (lf - 1.3) + 9 * gauss(lf, 1.78, 0.34);
        let p = Math.pow(10, base / 10);
        p += (gauss(lf, 1.716, 0.14) + 0.25 * gauss(lf, 3.55, 0.4)) * Math.pow(10, kickDb / 10);
        p +=
          (gauss(lf, lRoot, 0.11) +
            0.5 * gauss(lf, lRoot + 0.301, 0.13) +
            0.22 * gauss(lf, lRoot + 0.477, 0.13)) *
          Math.pow(10, bassDb / 10);
        p += gauss(lf, 3.28, 0.42) * Math.pow(10, snareDb / 10);
        p += gauss(lf, 3.93, 0.26) * Math.pow(10, hatDb / 10);
        p += gauss(lf, 2.72, 0.45) * Math.pow(10, padDb / 10);
        p += (gauss(lf, lMel, 0.09) + 0.4 * gauss(lf, lMel + 0.301, 0.1)) * Math.pow(10, melDb / 10);
        jitter[i] = jitter[i] * 0.9 + (rng() - 0.5) * 1.7;
        const target = clamp(10 * Math.log10(p) + jitter[i], SPECTRUM_DB_FLOOR, 0);
        const k = target > spectrumDb[i] ? Math.min(1, dt * 24) : Math.min(1, dt * 7);
        spectrumDb[i] += (target - spectrumDb[i]) * k;
        spectrum[i] = clamp((spectrumDb[i] - SPECTRUM_DB_FLOOR) / -SPECTRUM_DB_FLOOR, 0, 1);
      }

      // 电平（线性幅度，前端弹道学再加工）
      const instDb =
        -16.5 + 7 * kick + 4.5 * snare + 1.6 * hat + 2.2 * (bassAmp - 0.55) + 1.2 * (pad - 0.5);
      const pan = 1.1 * Math.sin(t * 0.53);
      const peakDbL = Math.min(-0.2, instDb + pan + 5 + 2.5 * snare + (rng() - 0.5));
      const peakDbR = Math.min(-0.2, instDb - pan + 5 + 2.5 * snare + (rng() - 0.5));
      const db2lin = (db: number) => Math.pow(10, db / 20);

      // 响度模拟
      const lmInst = instDb - 5.2;
      momentary += (lmInst - momentary) * Math.min(1, dt * 2.6);
      shortTerm += (momentary - shortTerm) * Math.min(1, dt * 0.45);
      const anchor = -15.8 + 0.8 * Math.sin(t * 0.021);
      integrated += (anchor + (shortTerm - anchor) * 0.25 - integrated) * Math.min(1, dt * 0.05);
      const lra = 5.6 + 1.9 * Math.sin(t * 0.037 + 2) + 0.7 * Math.sin(t * 0.011);
      const tp = Math.max(peakDbL, peakDbR) + 0.35 + (rng() < dt / 9 ? 2.4 : 0);
      tpMax = Math.max(tp, tpMax - dt * 0.02);

      // 声场散点
      const width = clamp(0.42 + 0.27 * Math.sin(t * 0.23) + 0.12 * Math.sin(t * 0.071), 0.05, 0.85);
      const scatter: number[] = new Array(SCATTER_PAIRS * 2);
      let sll = 0;
      let srr = 0;
      let slr = 0;
      for (let k2 = 0; k2 < SCATTER_PAIRS; k2 += 1) {
        const tau = k2 / SCATTER_PAIRS;
        const mid =
          0.52 * Math.sin(TAU * (3.1 * tau + t * 1.61)) +
          0.3 * Math.sin(TAU * (7.7 * tau - t * 1.13) + 0.7) +
          0.26 * (rng() * 2 - 1) +
          0.35 * kick * Math.sin(TAU * (1.35 * tau + t * 2.2));
        const side = width * (0.5 * Math.sin(TAU * (5.3 * tau + t * 0.87) + 1.1) + 0.5 * (rng() * 2 - 1));
        const left = clamp((mid + side) * 0.62, -1, 1);
        const right = clamp((mid - side) * 0.62, -1, 1);
        scatter[k2 * 2] = left;
        scatter[k2 * 2 + 1] = right;
        sll += left * left;
        srr += right * right;
        slr += left * right;
      }

      // 示波器波形：低音基波 + 泛音 + kick 冲击 + 噪底，L/R 相位/幅度略异
      const waveform: number[] = new Array(WAVE_POINTS * 2);
      const waveAmp = clamp(0.28 + 0.5 * kick + 0.2 * snare, 0.05, 0.95);
      const waveWindowSec = (WAVE_POINTS * 2) / 48000;
      for (let k3 = 0; k3 < WAVE_POINTS; k3 += 1) {
        const tk = t - waveWindowSec + (k3 / WAVE_POINTS) * waveWindowSec;
        const phase = TAU * root * tk;
        const body =
          0.62 * Math.sin(phase) +
          0.22 * Math.sin(2 * phase + 0.6) +
          0.12 * Math.sin(3.02 * phase + 1.9) +
          0.1 * Math.sin(TAU * melF * tk) * melAmp;
        const noise = (rng() * 2 - 1) * 0.05;
        const left = clamp(waveAmp * (body + noise) * (1 + 0.12 * pan), -1, 1);
        const right = clamp(
          waveAmp * (0.94 * body + noise) * (1 - 0.12 * pan),
          -1,
          1
        );
        waveform[k3 * 2] = Math.round(left * 32767);
        waveform[k3 * 2 + 1] = Math.round(right * 32767);
      }

      return {
        spectrum,
        peakLeft: db2lin(peakDbL),
        peakRight: db2lin(peakDbR),
        rmsLeft: db2lin(instDb + pan),
        rmsRight: db2lin(instDb - pan),
        momentaryLufs: momentary,
        shortTermLufs: shortTerm,
        integratedLufs: integrated,
        loudnessRangeLu: lra,
        truePeakDb: tp,
        truePeakMaxDb: tpMax,
        correlation: slr / Math.sqrt(sll * srr + 1e-9),
        scatter,
        waveform,
        sampleRate: 48000,
      };
    },
  };
}
