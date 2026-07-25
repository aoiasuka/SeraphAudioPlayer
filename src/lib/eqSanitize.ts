import type { CrossfeedSettings, EqBand, EqBandKind, EqPreset } from "@/types/dsp";

/**
 * EQ 数据消毒：所有能把频段/预放大写进 store 的入口（JSON 导入、APO 导入、
 * 高级编辑输入框、配置备份整包写入 → persist 水合）都必须经过这里。
 * NaN/Infinity/字符串数字/越界值一旦持久化，重启水合后 `toFixed` 会对
 * null/字符串抛 TypeError 造成启动白屏（H-1），因此宁可回落默认也不放行。
 */

/** 频段数量硬上限（导入路径防异常文件生成巨量频段拖垮 UI/引擎） */
export const MAX_EQ_BANDS = 64;

export const EQ_FREQ_MIN = 1;
export const EQ_FREQ_MAX = 96_000;
export const EQ_GAIN_LIMIT = 24;
export const EQ_Q_MIN = 0.01;
export const EQ_Q_MAX = 30;
export const EQ_PREAMP_LIMIT = 24;

const BAND_KINDS: readonly EqBandKind[] = [
  "peaking",
  "lowshelf",
  "highshelf",
  "lowpass",
  "highpass",
];

/** 宽容转数：number 直取，非空字符串尝试 Number()；NaN/Infinity 判无效 */
function toFinite(value: unknown): number | null {
  const num =
    typeof value === "number"
      ? value
      : typeof value === "string" && value.trim() !== ""
        ? Number(value)
        : Number.NaN;
  return Number.isFinite(num) ? num : null;
}

function clampNum(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

export function sanitizeEqPreamp(value: unknown): number {
  const num = toFinite(value);
  return num === null ? 0 : clampNum(num, -EQ_PREAMP_LIMIT, EQ_PREAMP_LIMIT);
}

/** 单频段消毒：freq 无效（频响曲线与后端 serde 的关键字段）则整段判废返回 null */
export function sanitizeEqBand(value: unknown): EqBand | null {
  if (!value || typeof value !== "object") return null;
  const raw = value as Record<string, unknown>;
  const freq = toFinite(raw.freq);
  if (freq === null) return null;
  const gain = toFinite(raw.gain);
  const q = toFinite(raw.q);
  const kind = BAND_KINDS.includes(raw.kind as EqBandKind)
    ? (raw.kind as EqBandKind)
    : "peaking";
  return {
    kind,
    freq: clampNum(freq, EQ_FREQ_MIN, EQ_FREQ_MAX),
    gain: gain === null ? 0 : clampNum(gain, -EQ_GAIN_LIMIT, EQ_GAIN_LIMIT),
    q: q === null ? 0.707 : clampNum(q, EQ_Q_MIN, EQ_Q_MAX),
    enabled: raw.enabled !== false,
  };
}

/** 频段数组消毒：无效项丢弃、截断上限；不是数组或全无效返回 null（调用方回落默认） */
export function sanitizeEqBands(value: unknown): EqBand[] | null {
  if (!Array.isArray(value)) return null;
  const bands: EqBand[] = [];
  for (const item of value) {
    const band = sanitizeEqBand(item);
    if (band) bands.push(band);
    if (bands.length >= MAX_EQ_BANDS) break;
  }
  return bands.length > 0 ? bands : null;
}

/**
 * 高级编辑的增量 patch 消毒：无效数值字段（清空输入框的 NaN 等）直接丢弃，
 * 即“该字段本次不修改”，其余合法字段照常生效。
 */
export function sanitizeEqBandPatch(patch: Partial<EqBand>): Partial<EqBand> {
  const clean: Partial<EqBand> = {};
  if (patch.kind !== undefined && BAND_KINDS.includes(patch.kind)) {
    clean.kind = patch.kind;
  }
  if (patch.freq !== undefined) {
    const freq = toFinite(patch.freq);
    if (freq !== null) clean.freq = clampNum(freq, EQ_FREQ_MIN, EQ_FREQ_MAX);
  }
  if (patch.gain !== undefined) {
    const gain = toFinite(patch.gain);
    if (gain !== null) clean.gain = clampNum(gain, -EQ_GAIN_LIMIT, EQ_GAIN_LIMIT);
  }
  if (patch.q !== undefined) {
    const q = toFinite(patch.q);
    if (q !== null) clean.q = clampNum(q, EQ_Q_MIN, EQ_Q_MAX);
  }
  if (patch.enabled !== undefined) {
    clean.enabled = patch.enabled === true;
  }
  return clean;
}

export function sanitizeCrossfeed(
  value: unknown,
  fallback: CrossfeedSettings
): CrossfeedSettings {
  if (!value || typeof value !== "object") return { ...fallback };
  const raw = value as Record<string, unknown>;
  const amount = toFinite(raw.amount);
  const cutoff = toFinite(raw.cutoffHz);
  return {
    enabled: raw.enabled === true,
    amount: amount === null ? fallback.amount : clampNum(amount, 0, 1),
    cutoffHz: cutoff === null ? fallback.cutoffHz : clampNum(cutoff, 20, 20_000),
  };
}

/** 用户预设列表消毒：结构坏/频段全废的预设整条丢弃 */
export function sanitizeUserPresets(value: unknown): EqPreset[] {
  if (!Array.isArray(value)) return [];
  const presets: EqPreset[] = [];
  for (const item of value) {
    if (!item || typeof item !== "object") continue;
    const raw = item as Record<string, unknown>;
    if (typeof raw.id !== "string" || typeof raw.name !== "string") continue;
    const bands = sanitizeEqBands(raw.bands);
    if (!bands) continue;
    presets.push({
      id: raw.id,
      name: raw.name,
      preamp: sanitizeEqPreamp(raw.preamp),
      bands,
      createdAt: toFinite(raw.createdAt) ?? 0,
    });
  }
  return presets;
}
