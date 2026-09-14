import { clamp, LEVEL_DB_FLOOR, SPECTRUM_DB_FLOOR, type AnalysisView } from "@/lib/analysis/types";
import { formatFreq, type ArchiveColors, type SpectrumGeometry } from "@/lib/analysis/render";

const FONT = '10px "Courier Prime", "Courier New", monospace';
const FIELD_TRAIL_ALPHA = [0.85, 0.24, 0.09, 0.035];

/** 小尺寸声场：缩减内边距，像素对齐细散点，淡化历史帧。 */
export function drawImmersiveField(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  colors: ArchiveColors, trail: Float32Array[],
) {
  ctx.clearRect(0, 0, w, h);
  if (w < 40 || h < 40) return;
  const cx = w / 2, cy = h / 2;
  const size = Math.max(10, Math.min(w, h) / 2 - 10);
  const transform = ctx.getTransform();
  const scaleX = transform.a || 1, scaleY = transform.d || 1;
  const pointPixels = Math.max(1, Math.round(Math.min(scaleX, scaleY) * 1.1));
  const pointW = pointPixels / scaleX, pointH = pointPixels / scaleY;
  const outline = () => {
    ctx.beginPath();
    ctx.moveTo(cx, cy - size);
    ctx.lineTo(cx + size, cy);
    ctx.lineTo(cx, cy + size);
    ctx.lineTo(cx - size, cy);
    ctx.closePath();
  };

  ctx.save();
  outline();
  ctx.clip();
  ctx.strokeStyle = colors.line;
  ctx.lineWidth = 0.75;
  ctx.beginPath();
  ctx.moveTo(cx - size, cy); ctx.lineTo(cx + size, cy);
  ctx.moveTo(cx, cy - size); ctx.lineTo(cx, cy + size);
  ctx.stroke();
  ctx.fillStyle = colors.ink;
  for (let frame = Math.max(0, trail.length - FIELD_TRAIL_ALPHA.length); frame < trail.length; frame += 1) {
    ctx.globalAlpha = FIELD_TRAIL_ALPHA[trail.length - 1 - frame];
    const points = trail[frame];
    for (let index = 0; index + 1 < points.length; index += 2) {
      // 与分析页保持相同的 L/R 幅度映射，不对安静片段自动放大增益。
      const gx = (points[index + 1] - points[index]) / Math.SQRT2 / 1.35 * size;
      const gy = (points[index] + points[index + 1]) / Math.SQRT2 / 1.35 * size;
      const x = Math.round((cx + gx - pointW / 2) * scaleX) / scaleX;
      const y = Math.round((cy - gy - pointH / 2) * scaleY) / scaleY;
      ctx.fillRect(x, y, pointW, pointH);
    }
  }
  ctx.restore();

  outline();
  ctx.strokeStyle = colors.ink;
  ctx.lineWidth = 1.1;
  ctx.stroke();
  ctx.font = '11px "Courier Prime", "Courier New", monospace';
  ctx.fillStyle = colors.ink;
  ctx.textAlign = "center";
  ctx.fillText("+L", cx - size / 2 - 14, cy - size / 2 - 8);
  ctx.fillText("+R", cx + size / 2 + 14, cy - size / 2 - 8);
}

export function levelLabel(value: number, hasData: boolean) {
  if (!hasData) return "—";
  return value <= LEVEL_DB_FLOOR + 0.1 ? "−∞" : value.toFixed(1);
}

/** 沉浸稿的柱形频谱。沿用后端 96 个对数频段及真实 -72 dBFS 地板。 */
export function drawImmersiveSpectrum(
  ctx: CanvasRenderingContext2D, w: number, h: number,
  view: AnalysisView, colors: ArchiveColors, cursorBin: number | null,
): SpectrumGeometry {
  ctx.clearRect(0, 0, w, h);
  const x0 = 34;
  const x1 = Math.max(x0 + 1, w - 16);
  const y0 = 10;
  const y1 = Math.max(y0 + 1, h - 24);
  const yForDb = (db: number) => y0 + clamp(db / SPECTRUM_DB_FLOOR, 0, 1) * (y1 - y0);
  const xForFreq = (freq: number) => x0 + Math.log10(freq / 20) / 3 * (x1 - x0);
  ctx.font = FONT;
  ctx.lineWidth = 0.5;
  ctx.textAlign = "right";
  for (let db = 0; db >= SPECTRUM_DB_FLOOR; db -= 12) {
    const y = yForDb(db);
    ctx.strokeStyle = colors.line;
    ctx.setLineDash([1, 4]);
    ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke();
    ctx.fillStyle = colors.ink2;
    ctx.fillText(String(db), x0 - 9, y + 3);
  }
  ctx.setLineDash([]);
  const frequencies = w < 480 ? [20, 100, 500, 2000, 10000, 20000] : [20, 50, 100, 200, 500, 1000, 2000, 5000, 10000, 20000];
  ctx.textAlign = "center";
  for (const frequency of frequencies) {
    const x = xForFreq(frequency);
    ctx.strokeStyle = colors.line;
    ctx.beginPath(); ctx.moveTo(x, y0); ctx.lineTo(x, y1); ctx.stroke();
    ctx.fillStyle = colors.ink2;
    ctx.fillText(formatFreq(frequency), x, h - 7);
  }

  const count = view.spectrumDb.length;
  const step = (x1 - x0) / count;
  const xForBin = (index: number) => x0 + index / (count - 1) * (x1 - x0);
  if (view.hasData) {
    ctx.save();
    ctx.beginPath(); ctx.rect(x0, y0 - 1, x1 - x0, y1 - y0 + 2); ctx.clip();
    ctx.fillStyle = colors.brown;
    ctx.globalAlpha = 0.58;
    for (let index = 0; index < count; index += 1) {
      const y = yForDb(view.spectrumDb[index]);
      ctx.fillRect(xForBin(index) - step * 0.33, y, Math.max(1, step * 0.66), y1 - y);
    }
    ctx.globalAlpha = 1;
    ctx.strokeStyle = colors.stamp;
    ctx.lineWidth = 1.2;
    ctx.beginPath();
    for (let index = 0; index < count; index += 1) {
      const x = xForBin(index), y = yForDb(view.peakHoldDb[index]);
      if (index === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y);
    }
    ctx.stroke();
    ctx.restore();
  } else {
    ctx.fillStyle = colors.ink2;
    ctx.textAlign = "center";
    ctx.font = '12px "Noto Sans SC", sans-serif';
    ctx.fillText("等待音频信号", (x0 + x1) / 2, (y0 + y1) / 2);
  }

  if (cursorBin !== null) {
    const x = xForBin(cursorBin);
    ctx.strokeStyle = colors.ink2;
    ctx.lineWidth = 1;
    ctx.setLineDash([2, 3]);
    ctx.beginPath(); ctx.moveTo(x, y0); ctx.lineTo(x, y1); ctx.stroke();
    ctx.setLineDash([]);
    if (view.hasData) {
      ctx.fillStyle = colors.stamp;
      ctx.beginPath(); ctx.arc(x, yForDb(view.spectrumDb[cursorBin]), 2.5, 0, Math.PI * 2); ctx.fill();
    }
  }
  return { x0, x1 };
}

/** 横向双声道表：棕色为 RMS，朱红刻线为峰值保持；未收到帧时不填充。 */
export function drawImmersiveLevels(ctx: CanvasRenderingContext2D, w: number, h: number, view: AnalysisView, colors: ArchiveColors) {
  ctx.clearRect(0, 0, w, h);
  ctx.font = FONT;
  const x0 = 0, x1 = w - 2;
  const rowHeight = Math.max(20, (h - 21) / 2);
  const dbToX = (db: number) => x0 + clamp((db + 60) / 60, 0, 1) * (x1 - x0);
  [view.levels.l, view.levels.r].forEach((channel, index) => {
    const labelY = index * rowHeight + 17;
    const barY = labelY + 10;
    const barH = Math.max(5, Math.min(11, rowHeight - 30));
    ctx.fillStyle = colors.ink2;
    ctx.textAlign = "left";
    ctx.fillText(index === 0 ? "L" : "R", x0, labelY);
    ctx.textAlign = "right";
    ctx.fillText(levelLabel(channel.rmsDb, view.hasData), x1, labelY);
    for (let x = x0; x < x1; x += 5) {
      ctx.fillStyle = view.hasData && x < dbToX(channel.rmsDb) ? colors.brown : colors.paper2;
      ctx.globalAlpha = view.hasData && x < dbToX(channel.rmsDb) ? 0.75 : 1;
      ctx.fillRect(x, barY, Math.min(3, x1 - x), barH);
    }
    ctx.globalAlpha = 1;
    if (view.hasData && channel.holdDb > -60) {
      ctx.fillStyle = colors.stamp;
      ctx.fillRect(Math.min(x1 - 1, dbToX(channel.holdDb)), barY - 3, 1, barH + 6);
    }
  });
  ctx.fillStyle = colors.ink2;
  for (const db of [-60, -36, -12, 0]) {
    ctx.textAlign = db === -60 ? "left" : db === 0 ? "right" : "center";
    ctx.fillText(String(db), dbToX(db), h - 2);
  }
}
