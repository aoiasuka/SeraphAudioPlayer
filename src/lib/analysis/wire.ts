/**
 * 可视化帧的二进制解码（VIZ-01）。
 *
 * `get_spectrum_frame` / `get_analysis_frame` 不再返回 JSON：后端把一帧写成小端
 * 字节，这里直接在响应缓冲上建 `Float32Array` / `Int16Array` 视图，不经过文本，
 * 也不逐个数字分配。布局与后端 `src-tauri/src/visualizer_wire.rs` 一一对应，
 * 两边各有一份回归测试对同一组黄金字节（`__fixtures__/*.hex`）——
 * **改布局必须两边一起改**。
 *
 * 约定：
 * - 全部小端（类型化数组按宿主字节序读，Windows 的 x64 / ARM64 都是小端）；
 * - 数组与必填标量由后端保证是有限值；
 * - 可空标量（响度各项）用 NaN 表示「暂无」，解码成 `null`；
 * - 请求身份（UTF-8）在帧尾；
 * - 空字节串 = 还没有帧，解码成 `null`。
 */

import type { AnalysisFrame } from "./types";

/** 每帧回带的请求身份与序号：轮询器靠它丢弃迟到帧和重复帧。 */
export interface FrameIdentity {
  requestId: string;
  sequence: number;
}

/** 侧栏频谱的一帧 */
export interface SpectrumFrame {
  bins: Float32Array;
  peakLeft: number;
  peakRight: number;
}

/** "SSP1" / "SAN1" 按小端 u32 读出的值 */
const SPECTRUM_MAGIC = 0x31505353;
const ANALYSIS_MAGIC = 0x314e4153;
const SPECTRUM_HEADER_BYTES = 24;
const ANALYSIS_HEADER_BYTES = 72;

const utf8 = new TextDecoder();

function malformed(reason: string) {
  return new Error(`可视化帧格式错误：${reason}`);
}

/**
 * invoke 的原始返回值 → 字节。
 *
 * 正常路径（自定义协议）拿到的是 `ArrayBuffer`；Tauri 退回 postMessage 通道时，
 * 二进制响应会变成普通的数字数组，这里一并接住。`null` / `undefined`（纯浏览器
 * 的 stub）按没有帧处理。
 */
function frameBytes(raw: unknown): Uint8Array | null {
  if (raw === null || raw === undefined) return null;
  if (ArrayBuffer.isView(raw)) {
    return new Uint8Array(raw.buffer, raw.byteOffset, raw.byteLength);
  }
  if (
    raw instanceof ArrayBuffer ||
    Object.prototype.toString.call(raw) === "[object ArrayBuffer]"
  ) {
    return new Uint8Array(raw as ArrayBuffer);
  }
  if (Array.isArray(raw)) return Uint8Array.from(raw as number[]);
  throw malformed("返回值不是二进制数据");
}

/**
 * 类型化数组视图要求起始偏移是元素大小的整数倍。整块响应缓冲的偏移是 0；
 * 万一拿到的是某个大缓冲中间的一段，就拷贝一份对齐的。
 */
function aligned(bytes: Uint8Array): Uint8Array {
  return bytes.byteOffset % 4 === 0 ? bytes : bytes.slice();
}

export function decodeSpectrumFrame(raw: unknown): (SpectrumFrame & FrameIdentity) | null {
  const bytes = frameBytes(raw);
  if (!bytes || bytes.byteLength === 0) return null;
  const data = aligned(bytes);
  if (data.byteLength < SPECTRUM_HEADER_BYTES) throw malformed("频谱帧头不完整");
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  if (view.getUint32(0, true) !== SPECTRUM_MAGIC) throw malformed("不是频谱帧");
  const binCount = view.getUint32(4, true);
  const idOffset = SPECTRUM_HEADER_BYTES + binCount * 4;
  if (idOffset > data.byteLength) throw malformed("频谱帧长度与柱数不符");
  return {
    bins: new Float32Array(data.buffer, data.byteOffset + SPECTRUM_HEADER_BYTES, binCount),
    peakLeft: view.getFloat32(16, true),
    peakRight: view.getFloat32(20, true),
    sequence: Number(view.getBigUint64(8, true)),
    requestId: utf8.decode(data.subarray(idOffset)),
  };
}

export function decodeAnalysisFrame(raw: unknown): (AnalysisFrame & FrameIdentity) | null {
  const bytes = frameBytes(raw);
  if (!bytes || bytes.byteLength === 0) return null;
  const data = aligned(bytes);
  if (data.byteLength < ANALYSIS_HEADER_BYTES) throw malformed("分析帧头不完整");
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  if (view.getUint32(0, true) !== ANALYSIS_MAGIC) throw malformed("不是分析帧");
  const spectrumCount = view.getUint32(4, true);
  const scatterCount = view.getUint32(8, true);
  const waveformCount = view.getUint32(12, true);
  const spectrumOffset = ANALYSIS_HEADER_BYTES;
  const scatterOffset = spectrumOffset + spectrumCount * 4;
  const waveformOffset = scatterOffset + scatterCount * 4;
  const idOffset = waveformOffset + waveformCount * 2;
  if (idOffset > data.byteLength) throw malformed("分析帧长度与数组长度不符");
  // NaN = 暂无（数据不足、对应仪表未开启）
  const optional = (offset: number) => {
    const value = view.getFloat32(offset, true);
    return Number.isFinite(value) ? value : null;
  };
  const base = data.byteOffset;
  return {
    sequence: Number(view.getBigUint64(16, true)),
    sampleRate: view.getUint32(24, true),
    peakLeft: view.getFloat32(28, true),
    peakRight: view.getFloat32(32, true),
    rmsLeft: view.getFloat32(36, true),
    rmsRight: view.getFloat32(40, true),
    correlation: view.getFloat32(44, true),
    momentaryLufs: optional(48),
    shortTermLufs: optional(52),
    integratedLufs: optional(56),
    loudnessRangeLu: optional(60),
    truePeakDb: optional(64),
    truePeakMaxDb: optional(68),
    spectrum: new Float32Array(data.buffer, base + spectrumOffset, spectrumCount),
    scatter: new Float32Array(data.buffer, base + scatterOffset, scatterCount),
    waveform: new Int16Array(data.buffer, base + waveformOffset, waveformCount),
    requestId: utf8.decode(data.subarray(idOffset)),
  };
}
