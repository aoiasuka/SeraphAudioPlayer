import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { decodeAnalysisFrame, decodeSpectrumFrame } from "./wire";
import { encodeAnalysisFrame, encodeSpectrumFrame } from "./wire.testutil";

/**
 * 与后端 `visualizer_wire.rs` 的测试共用同一组黄金字节：那边断言「编码结果 == 黄金
 * 字节」，这边断言「黄金字节解码出同一帧」。任何一边单独改布局都会红。
 */
function golden(name: string): ArrayBuffer {
  const hex = readFileSync(new URL(`./__fixtures__/${name}.hex`, import.meta.url), "utf8").trim();
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < bytes.length; i += 1) {
    bytes[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return bytes.buffer;
}

describe("可视化二进制帧解码（VIZ-01）", () => {
  it("频谱帧：黄金字节解码出后端写入的每个字段", () => {
    const frame = decodeSpectrumFrame(golden("spectrum-frame"));
    expect(frame).not.toBeNull();
    expect(frame!.bins).toBeInstanceOf(Float32Array);
    expect(Array.from(frame!.bins)).toEqual([0, 0.25, 1]);
    expect(frame!.peakLeft).toBe(0.5);
    expect(frame!.peakRight).toBe(0.75);
    // 序号超过 32 位
    expect(frame!.sequence).toBe(2 ** 32 + 2);
    expect(frame!.requestId).toBe("session-1:7");
  });

  it("分析帧：黄金字节解码出后端写入的每个字段，NaN 还原成 null", () => {
    const frame = decodeAnalysisFrame(golden("analysis-frame"));
    expect(frame).not.toBeNull();
    expect(frame!.spectrum).toBeInstanceOf(Float32Array);
    expect(frame!.scatter).toBeInstanceOf(Float32Array);
    expect(frame!.waveform).toBeInstanceOf(Int16Array);
    expect({
      ...frame!,
      spectrum: Array.from(frame!.spectrum),
      scatter: Array.from(frame!.scatter),
      waveform: Array.from(frame!.waveform),
    }).toEqual({
      spectrum: [0.5, 1],
      peakLeft: 0.5,
      peakRight: 0.25,
      rmsLeft: 0.125,
      rmsRight: 0.0625,
      momentaryLufs: -23.5,
      shortTermLufs: -20.25,
      integratedLufs: null,
      loudnessRangeLu: 6.5,
      truePeakDb: -1.25,
      truePeakMaxDb: null,
      correlation: -0.5,
      scatter: [0.25, -0.25, -1, 1],
      waveform: [0, 32767, -32768, -1],
      sampleRate: 96000,
      // 非 ASCII 的请求身份按 UTF-8 原样回来
      requestId: "曲目-α:12",
      // JS 能精确表示的最大整数
      sequence: Number.MAX_SAFE_INTEGER,
    });
  });

  it("数组是架在响应缓冲上的视图，不逐个拷贝", () => {
    const buffer = golden("analysis-frame");
    const frame = decodeAnalysisFrame(buffer)!;
    expect((frame.spectrum as Float32Array).buffer).toBe(buffer);
    expect((frame.waveform as Int16Array).buffer).toBe(buffer);
  });

  it("空字节串、null、undefined 都表示还没有帧", () => {
    for (const empty of [new ArrayBuffer(0), null, undefined, []]) {
      expect(decodeSpectrumFrame(empty)).toBeNull();
      expect(decodeAnalysisFrame(empty)).toBeNull();
    }
  });

  it("postMessage 回退通道给的数字数组、以及非对齐的字节视图同样能解", () => {
    const buffer = golden("analysis-frame");
    const expected = decodeAnalysisFrame(buffer)!;
    const plain = decodeAnalysisFrame(Array.from(new Uint8Array(buffer)))!;
    expect(Array.from(plain.scatter)).toEqual(Array.from(expected.scatter));
    expect(plain.requestId).toBe(expected.requestId);

    // 从偏移 1 开始的视图：直接建 Float32Array 会抛 RangeError，解码器要先对齐
    const shifted = new Uint8Array(buffer.byteLength + 1);
    shifted.set(new Uint8Array(buffer), 1);
    const offsetView = decodeAnalysisFrame(shifted.subarray(1))!;
    expect(Array.from(offsetView.waveform)).toEqual(Array.from(expected.waveform));
    expect(offsetView.sequence).toBe(expected.sequence);
  });

  it("畸形帧抛错而不是读出错位的数据", () => {
    const spectrum = golden("spectrum-frame");
    const analysis = golden("analysis-frame");
    // 两种帧互相喂错
    expect(() => decodeAnalysisFrame(spectrum)).toThrow(/格式错误/);
    expect(() => decodeSpectrumFrame(analysis)).toThrow(/不是频谱帧/);
    // 头部被截断
    expect(() => decodeAnalysisFrame(analysis.slice(0, 40))).toThrow(/帧头不完整/);
    // 声明的数组长度超出帧长
    const lying = analysis.slice(0);
    new DataView(lying).setUint32(4, 1_000_000, true);
    expect(() => decodeAnalysisFrame(lying)).toThrow(/长度/);
    const lyingSpectrum = spectrum.slice(0);
    new DataView(lyingSpectrum).setUint32(4, 0xffffffff, true);
    expect(() => decodeSpectrumFrame(lyingSpectrum)).toThrow(/长度/);
    // JSON 时代的对象形态不再接受
    expect(() => decodeAnalysisFrame({ spectrum: [] })).toThrow(/不是二进制数据/);
  });

  it("测试用编码器与解码器互逆（组件测试靠它模拟后端）", () => {
    const analysis = {
      spectrum: [0, 0.5, 1],
      peakLeft: 0.75,
      peakRight: 0.5,
      rmsLeft: 0.25,
      rmsRight: 0.125,
      momentaryLufs: -14,
      shortTermLufs: null,
      integratedLufs: -16.5,
      loudnessRangeLu: null,
      truePeakDb: -0.5,
      truePeakMaxDb: null,
      correlation: 0.25,
      scatter: [0.5, -0.5],
      waveform: [1, -2, 3, -4],
      sampleRate: 44100,
      requestId: "abc:3",
      sequence: 41,
    };
    const decoded = decodeAnalysisFrame(encodeAnalysisFrame(analysis))!;
    expect({
      ...decoded,
      spectrum: Array.from(decoded.spectrum),
      scatter: Array.from(decoded.scatter),
      waveform: Array.from(decoded.waveform),
    }).toEqual(analysis);

    const spectrum = { bins: [0.125, 0.875], peakLeft: 0.5, peakRight: 0.25, requestId: "s:1", sequence: 9 };
    const decodedSpectrum = decodeSpectrumFrame(encodeSpectrumFrame(spectrum))!;
    expect({ ...decodedSpectrum, bins: Array.from(decodedSpectrum.bins) }).toEqual(spectrum);
  });
});
