/**
 * 声学分析五面板的 Canvas 渲染器（档案 / 打字机风）。
 *
 * 画法约定与 EqPage 频响曲线、design/visualizer-demo 一致：
 * 网格 = line 色 0.5px；参考线 = 墨灰虚线；主迹线 = 印章红 2px；
 * 高能量警示 = 印章红；数字一律等宽 Courier。
 */

import {
  FREQ_MAX,
  FREQ_MIN,
  HISTORY_ROWS,
  SPECTRUM_DB_FLOOR,
  clamp,
  type AnalysisView,
} from "./types";

export interface ArchiveColors {
  ink: string;
  ink2: string;
  ink3: string;
  line: string;
  stamp: string;
  stampSoft: string;
  brown: string;
  paper2: string;
  card: string;
  goldDark: string;
}

const FALLBACK_COLORS: ArchiveColors = {
  ink: "#2b2722",
  ink2: "#6e675c",
  ink3: "#aaa193",
  line: "#d9d2c2",
  stamp: "#b5482a",
  stampSoft: "#f3e2dc",
  brown: "#7a5c3e",
  paper2: "#ece6d8",
  card: "#fbf9f3",
  goldDark: "#b48a12",
};

export function resolveArchiveColors(): ArchiveColors {
  if (typeof window === "undefined") return FALLBACK_COLORS;
  const style = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) =>
    style.getPropertyValue(name).trim() || fallback;
  return {
    ink: read("--ink", FALLBACK_COLORS.ink),
    ink2: read("--ink2", FALLBACK_COLORS.ink2),
    ink3: read("--ink3", FALLBACK_COLORS.ink3),
    line: read("--line", FALLBACK_COLORS.line),
    stamp: read("--stamp", FALLBACK_COLORS.stamp),
    stampSoft: read("--stamp-soft", FALLBACK_COLORS.stampSoft),
    brown: read("--brown", FALLBACK_COLORS.brown),
    paper2: read("--paper2", FALLBACK_COLORS.paper2),
    card: read("--card", FALLBACK_COLORS.card),
    goldDark: FALLBACK_COLORS.goldDark,
  };
}

function hexAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  const expanded =
    value.length === 3 ? value.replace(/./g, "$&$&") : value.padEnd(6, "0");
  const parsed = Number.parseInt(expanded.slice(0, 6), 16);
  const r = (parsed >> 16) & 255;
  const g = (parsed >> 8) & 255;
  const b = parsed & 255;
  return `rgba(${r},${g},${b},${alpha})`;
}

const lerp = (a: number, b: number, t: number) => a + (b - a) * t;
const LOG_MIN = Math.log10(FREQ_MIN);
const LOG_MAX = Math.log10(FREQ_MAX);
const TW9 = '9px "Courier Prime","Courier New",monospace';
const TW10 = 'bold 10px "Courier Prime","Courier New",monospace';
const TW11 = '11px "Courier Prime","Courier New",monospace';

const freqToX = (logF: number, x0: number, x1: number) =>
  x0 + ((logF - LOG_MIN) / (LOG_MAX - LOG_MIN)) * (x1 - x0);

const binLogF = (index: number, count: number) =>
  LOG_MIN + (index / (count - 1)) * (LOG_MAX - LOG_MIN);

export const binFreq = (index: number, count: number) =>
  Math.pow(10, binLogF(index, count));

export const formatFreq = (freq: number) =>
  freq >= 1000
    ? `${(freq / 1000).toFixed(freq % 1000 === 0 ? 0 : 1)}k`
    : `${Math.round(freq)}`;

/** 画布 DPR 适配；jsdom（测试）无 2D 上下文时返回 null 跳过绘制 */
export function prepCanvas(
  canvas: HTMLCanvasElement
): { ctx: CanvasRenderingContext2D; w: number; h: number } | null {
  const parent = canvas.parentElement;
  if (!parent) return null;
  const w = parent.clientWidth;
  const h = parent.clientHeight;
  if (w < 40 || h < 30) return null;
  const ctx = canvas.getContext("2d");
  if (!ctx) return null;
  const dpr = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(w * dpr));
  const height = Math.max(1, Math.round(h * dpr));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { ctx, w, h };
}

// ============================================================
// No.01 响度：目标偏差标尺
// ============================================================
export function drawLoudnessDeviationBar(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  view: AnalysisView,
  colors: ArchiveColors
) {
  ctx.clearRect(0, 0, w, h);
  const x0 = 6;
  const x1 = w - 6;
  const mid = h / 2 - 4;
  const devX = (lu: number) => lerp(x0, x1, (clamp(lu, -9, 9) + 9) / 18);

  ctx.font = TW9;
  ctx.textAlign = "center";
  ctx.fillStyle = colors.ink3;
  ctx.strokeStyle = colors.line;
  ctx.lineWidth = 1;
  for (const lu of [-9, -6, -3, 0, 3, 6, 9]) {
    const x = devX(lu);
    ctx.beginPath();
    ctx.moveTo(x, mid - 8);
    ctx.lineTo(x, mid + 8);
    ctx.stroke();
    ctx.fillText(lu > 0 ? `+${lu}` : `${lu}`, x, h - 2);
  }
  ctx.strokeStyle = hexAlpha(colors.ink3, 0.7);
  ctx.beginPath();
  ctx.moveTo(x0, mid);
  ctx.lineTo(x1, mid);
  ctx.stroke();
  // 目标线（鎏金）
  ctx.strokeStyle = colors.goldDark;
  ctx.lineWidth = 2;
  ctx.beginPath();
  ctx.moveTo(devX(0), mid - 11);
  ctx.lineTo(devX(0), mid + 11);
  ctx.stroke();

  const { m, i, target } = view.loud;
  if (m !== null) {
    const devM = m - target;
    ctx.fillStyle = hexAlpha(colors.ink, 0.28);
    ctx.fillRect(
      Math.min(devX(0), devX(devM)),
      mid + 4,
      Math.abs(devX(devM) - devX(0)),
      3
    );
  }
  if (i !== null) {
    const devI = i - target;
    ctx.fillStyle =
      devI > 1 ? colors.stamp : devI < -1 ? hexAlpha(colors.ink, 0.55) : colors.brown;
    ctx.fillRect(
      Math.min(devX(0), devX(devI)),
      mid - 6,
      Math.abs(devX(devI) - devX(0)),
      6
    );
  }
}

// ============================================================
// No.02 电平表
// ============================================================
const LEVEL_SCALE = [0, -3, -6, -9, -12, -18, -24, -30, -40, -50, -60];
const LEVEL_LABELED = new Set([0, -6, -12, -24, -40, -60]);

export function drawLevelsMeter(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  view: AnalysisView,
  colors: ArchiveColors
) {
  ctx.clearRect(0, 0, w, h);
  const top = 8;
  const bottom = h - 18;
  const axisW = 34;
  const dbY = (db: number) => lerp(top, bottom, clamp(-db, 0, 60) / 60);
  const meterX0 = axisW + 6;
  const barW = clamp((w - meterX0 - 10) / 4.6, 22, 52);
  const gap = barW * 0.7;
  const cx = (w + meterX0) / 2;
  const bars = [
    { x: cx - gap / 2 - barW, ch: view.levels.l, tag: "L" },
    { x: cx + gap / 2, ch: view.levels.r, tag: "R" },
  ];

  ctx.font = TW9;
  ctx.textAlign = "right";
  ctx.textBaseline = "middle";
  for (const db of LEVEL_SCALE) {
    const y = dbY(db);
    ctx.strokeStyle = colors.line;
    ctx.lineWidth = LEVEL_LABELED.has(db) ? 1 : 0.5;
    ctx.beginPath();
    ctx.moveTo(meterX0 - 3, y);
    ctx.lineTo(w - 6, y);
    ctx.stroke();
    if (LEVEL_LABELED.has(db)) {
      ctx.fillStyle = colors.ink2;
      ctx.fillText(String(db), axisW, y);
    }
  }

  for (const bar of bars) {
    const { x, ch, tag } = bar;
    ctx.fillStyle = colors.paper2;
    ctx.fillRect(x, top, barW, bottom - top);
    // 红区（0 ~ -6 dBFS）阴影线
    const redY = dbY(-6);
    ctx.fillStyle = colors.stampSoft;
    ctx.fillRect(x, top, barW, redY - top);
    ctx.save();
    ctx.beginPath();
    ctx.rect(x, top, barW, redY - top);
    ctx.clip();
    ctx.strokeStyle = hexAlpha(colors.stamp, 0.35);
    ctx.lineWidth = 1;
    for (let sx = x - (bottom - top); sx < x + barW; sx += 5) {
      ctx.beginPath();
      ctx.moveTo(sx, redY + 2);
      ctx.lineTo(sx + (redY - top) + 4, top - 2);
      ctx.stroke();
    }
    ctx.restore();
    // 峰值（浅棕）与 RMS（实墨）
    ctx.fillStyle = hexAlpha(colors.brown, 0.34);
    ctx.fillRect(x, dbY(ch.peakDb), barW, bottom - dbY(ch.peakDb));
    ctx.fillStyle = hexAlpha(colors.ink, 0.85);
    ctx.fillRect(x, dbY(ch.rmsDb), barW, bottom - dbY(ch.rmsDb));
    // 峰值保持线
    ctx.strokeStyle = ch.holdDb > -6 ? colors.stamp : colors.brown;
    ctx.lineWidth = 2.5;
    ctx.beginPath();
    ctx.moveTo(x - 2, dbY(ch.holdDb));
    ctx.lineTo(x + barW + 2, dbY(ch.holdDb));
    ctx.stroke();
    // 外框 + 声道标签
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 1.5;
    ctx.strokeRect(x, top, barW, bottom - top);
    ctx.font = TW10;
    ctx.textAlign = "center";
    ctx.fillStyle = colors.ink2;
    ctx.fillText(tag, x + barW / 2, h - 6);
    ctx.font = TW9;
    ctx.textAlign = "right";
  }
  ctx.textBaseline = "alphabetic";
}

// ============================================================
// No.03 声场（极坐标 / 李萨如 + 相关度）
// ============================================================
export type SoundFieldMode = "polar" | "lissajous";
const TRAIL_ALPHA = [0.8, 0.5, 0.34, 0.22, 0.14, 0.08];

export function drawSoundField(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  view: AnalysisView,
  colors: ArchiveColors,
  mode: SoundFieldMode,
  trail: Float32Array[]
) {
  ctx.clearRect(0, 0, w, h);
  const corrH = 34;
  const mainH = h - corrH;
  if (mainH < 40) return;

  if (mode === "polar") {
    const cx = w / 2;
    const cy = mainH - 10;
    const radius = Math.max(10, Math.min(w * 0.42, mainH - 26));
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.arc(cx, cy, radius, Math.PI, 0);
    ctx.stroke();
    ctx.strokeStyle = colors.line;
    ctx.setLineDash([4, 3]);
    ctx.beginPath();
    ctx.arc(cx, cy, radius / 2, Math.PI, 0);
    ctx.stroke();
    ctx.setLineDash([]);
    for (let deg = -90; deg <= 90; deg += 30) {
      const angle = (deg * Math.PI) / 180;
      const strong = deg === -45 || deg === 45;
      ctx.strokeStyle = strong
        ? hexAlpha(colors.ink, 0.75)
        : hexAlpha(colors.ink3, 0.5);
      ctx.lineWidth = strong ? 1 : 0.5;
      ctx.beginPath();
      ctx.moveTo(
        cx + Math.sin(angle) * radius * 0.12,
        cy - Math.cos(angle) * radius * 0.12
      );
      ctx.lineTo(cx + Math.sin(angle) * radius, cy - Math.cos(angle) * radius);
      ctx.stroke();
    }
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(cx - radius, cy);
    ctx.lineTo(cx + radius, cy);
    ctx.stroke();
    ctx.font = TW10;
    ctx.fillStyle = colors.ink2;
    ctx.textAlign = "center";
    const angleL = (-45 * Math.PI) / 180;
    const angleR = (45 * Math.PI) / 180;
    ctx.fillText(
      "L",
      cx + Math.sin(angleL) * (radius + 11),
      cy - Math.cos(angleL) * (radius + 11) + 3
    );
    ctx.fillText(
      "R",
      cx + Math.sin(angleR) * (radius + 11),
      cy - Math.cos(angleR) * (radius + 11) + 3
    );
    ctx.font = TW9;
    ctx.fillStyle = colors.ink3;
    ctx.fillText("MONO", cx, cy - radius - 8);

    for (let g = 0; g < trail.length; g += 1) {
      const pts = trail[g];
      const age = trail.length - 1 - g;
      const alpha = TRAIL_ALPHA[age] ?? 0.06;
      ctx.fillStyle = hexAlpha(colors.ink, alpha);
      for (let k = 0; k + 1 < pts.length; k += 2) {
        const left = pts[k];
        const right = pts[k + 1];
        const gx = (right - left) / Math.SQRT2;
        const gy = (left + right) / Math.SQRT2;
        const r = Math.min(1, Math.hypot(gx, gy) / 1.35);
        const phi = Math.atan2(gx, Math.abs(gy));
        const px = cx + Math.sin(phi) * r * radius;
        const py = cy - Math.cos(phi) * r * radius;
        if (r > 0.82 && age === 0) ctx.fillStyle = hexAlpha(colors.stamp, 0.8);
        ctx.fillRect(px - 1.1, py - 1.1, 2.2, 2.2);
        if (r > 0.82 && age === 0) ctx.fillStyle = hexAlpha(colors.ink, alpha);
      }
    }
  } else {
    const cx = w / 2;
    const cyc = mainH / 2 + 2;
    const size = Math.max(10, Math.min(w, mainH) / 2 - 18);
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    ctx.moveTo(cx, cyc - size);
    ctx.lineTo(cx + size, cyc);
    ctx.lineTo(cx, cyc + size);
    ctx.lineTo(cx - size, cyc);
    ctx.closePath();
    ctx.stroke();
    ctx.strokeStyle = hexAlpha(colors.ink3, 0.5);
    ctx.lineWidth = 0.5;
    ctx.beginPath();
    ctx.moveTo(cx - size, cyc);
    ctx.lineTo(cx + size, cyc);
    ctx.moveTo(cx, cyc - size);
    ctx.lineTo(cx, cyc + size);
    ctx.stroke();
    ctx.font = TW10;
    ctx.fillStyle = colors.ink2;
    ctx.textAlign = "center";
    ctx.fillText("+L", cx - size / 2 - 12, cyc - size / 2 - 6);
    ctx.fillText("+R", cx + size / 2 + 12, cyc - size / 2 - 6);
    for (let g = 0; g < trail.length; g += 1) {
      const pts = trail[g];
      const age = trail.length - 1 - g;
      const alpha = TRAIL_ALPHA[age] ?? 0.06;
      ctx.fillStyle = hexAlpha(colors.ink, alpha);
      for (let k = 0; k + 1 < pts.length; k += 2) {
        const left = pts[k];
        const right = pts[k + 1];
        const gx = ((right - left) / Math.SQRT2 / 1.35) * size;
        const gy = ((left + right) / Math.SQRT2 / 1.35) * size;
        ctx.fillRect(cx + gx - 1.1, cyc - gy - 1.1, 2.2, 2.2);
      }
    }
  }

  // 相关度表
  const corr = view.stereo.corr;
  const y0 = h - corrH + 8;
  const bx0 = 86;
  const bx1 = w - 56;
  if (bx1 <= bx0) return;
  ctx.font = TW9;
  ctx.textAlign = "left";
  ctx.fillStyle = colors.ink3;
  ctx.fillText("CORRELATION 相关", 2, y0 + 8);
  ctx.strokeStyle = colors.line;
  ctx.lineWidth = 1;
  ctx.strokeRect(bx0, y0, bx1 - bx0, 10);
  const midX = (bx0 + bx1) / 2;
  ctx.strokeStyle = colors.ink3;
  for (const tick of [-1, -0.5, 0, 0.5, 1]) {
    const x = lerp(bx0, bx1, (tick + 1) / 2);
    ctx.beginPath();
    ctx.moveTo(x, y0 + 10);
    ctx.lineTo(x, y0 + 14);
    ctx.stroke();
  }
  const corrX = lerp(bx0, bx1, (clamp(corr, -1, 1) + 1) / 2);
  ctx.fillStyle = corr >= 0 ? hexAlpha(colors.ink, 0.78) : hexAlpha(colors.stamp, 0.9);
  ctx.fillRect(Math.min(midX, corrX), y0 + 2, Math.abs(corrX - midX), 6);
  ctx.strokeStyle = colors.ink;
  ctx.beginPath();
  ctx.moveTo(midX, y0 - 2);
  ctx.lineTo(midX, y0 + 12);
  ctx.stroke();
  ctx.font = TW11;
  ctx.textAlign = "right";
  ctx.fillStyle = corr < 0 ? colors.stamp : colors.ink;
  ctx.fillText(`${corr >= 0 ? "+" : ""}${corr.toFixed(2)}`, w - 8, y0 + 9);
}

// ============================================================
// No.04 频谱
// ============================================================
const SPECTRUM_MINOR = [30, 40, 60, 80, 150, 300, 400, 600, 800, 1500, 3000, 4000, 6000, 8000, 15000];
const SPECTRUM_MAJOR = [100, 1000, 10000];
const SPECTRUM_LABELS = [50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];

export interface SpectrumGeometry {
  x0: number;
  x1: number;
}

export function drawSpectrumChart(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  view: AnalysisView,
  colors: ArchiveColors,
  cursorBin: number | null
): SpectrumGeometry {
  ctx.clearRect(0, 0, w, h);
  const x0 = 6;
  const x1 = w - 30;
  const y0 = 6;
  const y1 = h - 17;
  const dbY = (db: number) =>
    lerp(y0, y1, clamp(-db, 0, -SPECTRUM_DB_FLOOR) / -SPECTRUM_DB_FLOOR);

  for (const freq of SPECTRUM_MINOR) {
    const x = freqToX(Math.log10(freq), x0, x1);
    ctx.strokeStyle = hexAlpha(colors.line, 0.55);
    ctx.lineWidth = 0.5;
    ctx.beginPath();
    ctx.moveTo(x, y0);
    ctx.lineTo(x, y1);
    ctx.stroke();
  }
  for (const freq of SPECTRUM_MAJOR) {
    const x = freqToX(Math.log10(freq), x0, x1);
    ctx.strokeStyle = hexAlpha(colors.ink3, 0.55);
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(x, y0);
    ctx.lineTo(x, y1);
    ctx.stroke();
  }
  ctx.font = TW9;
  ctx.textAlign = "left";
  for (let db = 0; db >= SPECTRUM_DB_FLOOR; db -= 12) {
    const y = dbY(db);
    ctx.strokeStyle = db === 0 ? hexAlpha(colors.ink3, 0.7) : hexAlpha(colors.line, 0.7);
    ctx.lineWidth = 0.5;
    ctx.beginPath();
    ctx.moveTo(x0, y);
    ctx.lineTo(x1, y);
    ctx.stroke();
    if (db % 24 === 0) {
      ctx.fillStyle = colors.ink3;
      ctx.fillText(String(db), x1 + 4, y + 3);
    }
  }
  ctx.textAlign = "center";
  ctx.fillStyle = colors.ink2;
  const labels = w < 520 ? SPECTRUM_MAJOR : SPECTRUM_LABELS;
  for (const freq of labels) {
    ctx.fillText(formatFreq(freq), freqToX(Math.log10(freq), x0, x1), h - 5);
  }

  const count = view.spectrumDb.length;
  const px = (index: number) => freqToX(binLogF(index, count), x0, x1);

  // 峰值保持（墨灰虚线）
  ctx.strokeStyle = hexAlpha(colors.ink2, 0.75);
  ctx.lineWidth = 1;
  ctx.setLineDash([3, 3]);
  ctx.beginPath();
  for (let i = 0; i < count; i += 1) {
    const y = dbY(view.peakHoldDb[i]);
    if (i === 0) ctx.moveTo(px(i), y);
    else ctx.lineTo(px(i), y);
  }
  ctx.stroke();
  ctx.setLineDash([]);

  // 主频谱迹线 + 淡填充
  ctx.beginPath();
  ctx.moveTo(x0, y1);
  for (let i = 0; i < count; i += 1) ctx.lineTo(px(i), dbY(view.spectrumDb[i]));
  ctx.lineTo(x1, y1);
  ctx.closePath();
  ctx.fillStyle = hexAlpha(colors.stamp, 0.08);
  ctx.fill();
  ctx.strokeStyle = colors.stamp;
  ctx.lineWidth = 2;
  ctx.beginPath();
  for (let i = 0; i < count; i += 1) {
    const y = dbY(view.spectrumDb[i]);
    if (i === 0) ctx.moveTo(px(i), y);
    else ctx.lineTo(px(i), y);
  }
  ctx.stroke();

  // 游标
  if (cursorBin !== null && cursorBin >= 0 && cursorBin < count) {
    const cx = px(cursorBin);
    const cy = dbY(view.spectrumDb[cursorBin]);
    ctx.strokeStyle = hexAlpha(colors.ink, 0.8);
    ctx.lineWidth = 1;
    ctx.setLineDash([2, 3]);
    ctx.beginPath();
    ctx.moveTo(cx, y0);
    ctx.lineTo(cx, y1);
    ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = colors.stamp;
    ctx.beginPath();
    ctx.arc(cx, cy, 3.2, 0, Math.PI * 2);
    ctx.fill();
    ctx.strokeStyle = colors.card;
    ctx.lineWidth = 1.2;
    ctx.stroke();
  }
  return { x0, x1 };
}

export function spectrumBinFromX(
  x: number,
  geometry: SpectrumGeometry,
  binCount: number
): number {
  const ratio = clamp((x - geometry.x0) / (geometry.x1 - geometry.x0), 0, 1);
  return Math.round(ratio * (binCount - 1));
}

// ============================================================
// No.05 频谱瀑布（山脊 / 热图）
// ============================================================
export type SpectrogramMode = "ridge" | "heat";

/** 热图色带：纸 → 浅褐 → 棕 → 印章红 → 墨（墨渍渗纸档案色阶） */
const HEAT_STOPS: Array<[number, number, number, number]> = [
  [0, 0xfb, 0xf9, 0xf3],
  [0.3, 0xec, 0xdf, 0xc4],
  [0.52, 0xcb, 0xb4, 0x89],
  [0.68, 0x9a, 0x7a, 0x52],
  [0.8, 0x7a, 0x5c, 0x3e],
  [0.9, 0xb5, 0x48, 0x2a],
  [1, 0x2b, 0x27, 0x22],
];

const HEAT_LUT = (() => {
  const lut = new Uint8ClampedArray(256 * 3);
  for (let v = 0; v < 256; v += 1) {
    const t = v / 255;
    let s0 = HEAT_STOPS[0];
    let s1 = HEAT_STOPS[HEAT_STOPS.length - 1];
    for (let k = 0; k < HEAT_STOPS.length - 1; k += 1) {
      if (t >= HEAT_STOPS[k][0] && t <= HEAT_STOPS[k + 1][0]) {
        s0 = HEAT_STOPS[k];
        s1 = HEAT_STOPS[k + 1];
        break;
      }
    }
    const f = (t - s0[0]) / Math.max(1e-6, s1[0] - s0[0]);
    lut[v * 3] = lerp(s0[1], s1[1], f);
    lut[v * 3 + 1] = lerp(s0[2], s1[2], f);
    lut[v * 3 + 2] = lerp(s0[3], s1[3], f);
  }
  return lut;
})();

const ridgeNorm = (db: number) =>
  clamp((db - SPECTRUM_DB_FLOOR + -6) / (-SPECTRUM_DB_FLOOR - 6), 0, 1);

export function drawSpectrogram(
  ctx: CanvasRenderingContext2D,
  w: number,
  h: number,
  view: AnalysisView,
  colors: ArchiveColors,
  mode: SpectrogramMode,
  heatCanvas: HTMLCanvasElement
) {
  ctx.clearRect(0, 0, w, h);
  const rows = HISTORY_ROWS;
  const count = view.spectrumDb.length;

  if (mode === "ridge") {
    const topY = 16;
    const botY = h - 24;
    for (let k = 0; k < rows; k += 1) {
      const row = view.history[(view.historyHead + 1 + k) % rows];
      const depth = k / (rows - 1);
      const xL = lerp(w * 0.16, w * 0.05, depth);
      const xR = lerp(w * 0.84, w * 0.95, depth);
      const base = lerp(topY, botY, depth);
      const amp = lerp(0.11, 0.24, depth) * h;
      // 遮挡填充
      ctx.beginPath();
      ctx.moveTo(xL, base);
      for (let i = 0; i < count; i += 1) {
        const v = Math.pow(ridgeNorm(row[i]), 1.3);
        ctx.lineTo(lerp(xL, xR, i / (count - 1)), base - v * amp);
      }
      ctx.lineTo(xR, base);
      ctx.closePath();
      ctx.fillStyle = colors.card;
      ctx.fill();
      // 墨线
      ctx.strokeStyle = hexAlpha(colors.ink, lerp(0.16, 0.85, depth));
      ctx.lineWidth = lerp(0.6, 1.3, depth);
      ctx.beginPath();
      for (let i = 0; i < count; i += 1) {
        const v = Math.pow(ridgeNorm(row[i]), 1.3);
        const x = lerp(xL, xR, i / (count - 1));
        const y = base - v * amp;
        if (i === 0) ctx.moveTo(x, y);
        else ctx.lineTo(x, y);
      }
      ctx.stroke();
      // 高能量段描印章红
      ctx.strokeStyle = hexAlpha(colors.stamp, lerp(0.14, 0.8, depth));
      ctx.lineWidth = lerp(0.7, 1.5, depth);
      let inSegment = false;
      ctx.beginPath();
      for (let i = 0; i < count; i += 1) {
        const v = Math.pow(ridgeNorm(row[i]), 1.3);
        const x = lerp(xL, xR, i / (count - 1));
        const y = base - v * amp;
        if (v > 0.52) {
          if (inSegment) ctx.lineTo(x, y);
          else ctx.moveTo(x, y);
          inSegment = true;
        } else {
          inSegment = false;
        }
      }
      ctx.stroke();
    }
    ctx.font = TW9;
    ctx.textAlign = "center";
    ctx.fillStyle = colors.ink2;
    for (const freq of SPECTRUM_MAJOR) {
      const x = freqToX(Math.log10(freq), w * 0.05, w * 0.95);
      ctx.fillText(formatFreq(freq), x, h - 8);
      ctx.strokeStyle = colors.line;
      ctx.lineWidth = 1;
      ctx.beginPath();
      ctx.moveTo(x, botY);
      ctx.lineTo(x, botY + 4);
      ctx.stroke();
    }
    ctx.textAlign = "left";
    ctx.fillStyle = colors.ink3;
    ctx.fillText("PAST ↑", 6, topY + 2);
  } else {
    const x0 = 8;
    const x1 = w - 44;
    const y0 = 8;
    const y1 = h - 22;
    if (x1 <= x0 || y1 <= y0) return;
    const offCtx = heatCanvas.getContext("2d");
    if (!offCtx) return;
    if (heatCanvas.width !== count || heatCanvas.height !== rows) {
      heatCanvas.width = count;
      heatCanvas.height = rows;
    }
    const image = offCtx.createImageData(count, rows);
    for (let k = 0; k < rows; k += 1) {
      const row = view.history[(view.historyHead + 1 + k) % rows];
      for (let i = 0; i < count; i += 1) {
        // 热图专用地板 -66dB + gamma 1.25：底噪回归纸色
        const v = Math.round(
          Math.pow(clamp((row[i] + 66) / 66, 0, 1), 1.25) * 255
        );
        const offset = (k * count + i) * 4;
        image.data[offset] = HEAT_LUT[v * 3];
        image.data[offset + 1] = HEAT_LUT[v * 3 + 1];
        image.data[offset + 2] = HEAT_LUT[v * 3 + 2];
        image.data[offset + 3] = 255;
      }
    }
    offCtx.putImageData(image, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(heatCanvas, x0, y0, x1 - x0, y1 - y0);
    ctx.strokeStyle = colors.ink;
    ctx.lineWidth = 1.5;
    ctx.strokeRect(x0, y0, x1 - x0, y1 - y0);
    ctx.font = TW9;
    ctx.textAlign = "center";
    ctx.fillStyle = colors.ink2;
    for (const freq of SPECTRUM_MAJOR) {
      const x = freqToX(Math.log10(freq), x0, x1);
      ctx.fillText(formatFreq(freq), x, h - 8);
      ctx.strokeStyle = hexAlpha(colors.ink, 0.5);
      ctx.beginPath();
      ctx.moveTo(x, y1);
      ctx.lineTo(x, y1 + 4);
      ctx.stroke();
    }
    ctx.textAlign = "left";
    ctx.fillStyle = colors.ink3;
    ctx.fillText("NOW", x1 + 6, y1 - 2);
    ctx.fillText("-5.8S", x1 + 6, y0 + 9);
  }
}
