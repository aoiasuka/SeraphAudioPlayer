/**
 * 仅供测试使用：把一帧编码成后端 `visualizer_wire.rs` 的二进制布局，
 * 让组件测试可以像真实后端那样给 `invoke` 返回 `ArrayBuffer`。
 * 生产代码只解码（`wire.ts`），不要引用本文件。
 */

import type { AnalysisFrame } from "./types";
import type { FrameIdentity, SpectrumFrame } from "./wire";

const utf8 = new TextEncoder();

const optional = (value: number | null) => (value === null ? Number.NaN : value);

export function encodeSpectrumFrame(
  frame: Omit<SpectrumFrame, "bins"> & { bins: ArrayLike<number> } & FrameIdentity,
): ArrayBuffer {
  const id = utf8.encode(frame.requestId);
  const buffer = new ArrayBuffer(24 + frame.bins.length * 4 + id.length);
  const view = new DataView(buffer);
  new Uint8Array(buffer).set(utf8.encode("SSP1"), 0);
  view.setUint32(4, frame.bins.length, true);
  view.setBigUint64(8, BigInt(frame.sequence), true);
  view.setFloat32(16, frame.peakLeft, true);
  view.setFloat32(20, frame.peakRight, true);
  let offset = 24;
  for (let i = 0; i < frame.bins.length; i += 1, offset += 4) {
    view.setFloat32(offset, frame.bins[i], true);
  }
  new Uint8Array(buffer).set(id, offset);
  return buffer;
}

export function encodeAnalysisFrame(frame: AnalysisFrame & FrameIdentity): ArrayBuffer {
  const id = utf8.encode(frame.requestId);
  const buffer = new ArrayBuffer(
    72 + (frame.spectrum.length + frame.scatter.length) * 4 + frame.waveform.length * 2 + id.length,
  );
  const view = new DataView(buffer);
  new Uint8Array(buffer).set(utf8.encode("SAN1"), 0);
  view.setUint32(4, frame.spectrum.length, true);
  view.setUint32(8, frame.scatter.length, true);
  view.setUint32(12, frame.waveform.length, true);
  view.setBigUint64(16, BigInt(frame.sequence), true);
  view.setUint32(24, frame.sampleRate, true);
  const scalars = [
    frame.peakLeft,
    frame.peakRight,
    frame.rmsLeft,
    frame.rmsRight,
    frame.correlation,
    optional(frame.momentaryLufs),
    optional(frame.shortTermLufs),
    optional(frame.integratedLufs),
    optional(frame.loudnessRangeLu),
    optional(frame.truePeakDb),
    optional(frame.truePeakMaxDb),
  ];
  let offset = 28;
  for (const value of scalars) {
    view.setFloat32(offset, value, true);
    offset += 4;
  }
  for (const values of [frame.spectrum, frame.scatter]) {
    for (let i = 0; i < values.length; i += 1, offset += 4) {
      view.setFloat32(offset, values[i], true);
    }
  }
  for (let i = 0; i < frame.waveform.length; i += 1, offset += 2) {
    view.setInt16(offset, frame.waveform[i], true);
  }
  new Uint8Array(buffer).set(id, offset);
  return buffer;
}
