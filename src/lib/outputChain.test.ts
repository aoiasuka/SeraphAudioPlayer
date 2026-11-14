import { describe, expect, it } from "vitest";
import {
  describeOutputChain,
  formatKiloHertz,
  isDsdPath,
  parseOutputFormatEvent,
  type OutputFormatInfo,
} from "./outputChain";

const NO_DSP = { eqEnabled: false, crossfeedEnabled: false, dspBypassedForDsd: false };

function format(overrides: Partial<OutputFormatInfo> = {}): OutputFormatInfo {
  return {
    trackId: "t",
    mode: "exclusive",
    sourceRate: 96_000,
    sourceBits: 24,
    sourceChannels: 2,
    outputRate: 96_000,
    outputBits: 24,
    outputFloat: false,
    outputChannels: 2,
    ...overrides,
  };
}

describe("parseOutputFormatEvent", () => {
  it("读取后端的 snake_case 字段", () => {
    expect(
      parseOutputFormatEvent({
        type: "output_format",
        track_id: "t",
        mode: "shared",
        source_rate: 44_100,
        source_bits: 16,
        source_channels: 2,
        output_rate: 48_000,
        output_bits: 32,
        output_float: true,
        output_channels: 2,
      })
    ).toEqual({
      trackId: "t",
      mode: "shared",
      sourceRate: 44_100,
      sourceBits: 16,
      sourceChannels: 2,
      outputRate: 48_000,
      outputBits: 32,
      outputFloat: true,
      outputChannels: 2,
    });
  });

  it("字段缺失、模式未知或数值越界时返回 null", () => {
    const base = {
      track_id: "t", mode: "shared", source_rate: 44_100, source_bits: 16,
      source_channels: 2, output_rate: 48_000, output_bits: 32, output_channels: 2,
    };
    expect(parseOutputFormatEvent({ ...base, mode: "asio" })).toBeNull();
    expect(parseOutputFormatEvent({ ...base, track_id: "" })).toBeNull();
    expect(parseOutputFormatEvent({ ...base, output_rate: 0 })).toBeNull();
    expect(parseOutputFormatEvent({ ...base, output_bits: Number.NaN })).toBeNull();
    expect(parseOutputFormatEvent({ ...base, source_channels: "2" })).toBeNull();
    // 源位深未知（0 或缺失）是合法情况
    expect(parseOutputFormatEvent({ ...base, source_bits: undefined })?.sourceBits).toBe(0);
  });
});

describe("describeOutputChain", () => {
  it("独占、采样率与声道一致、没有 DSP 时是直通", () => {
    expect(describeOutputChain(format(), NO_DSP)).toEqual({
      output: "独占 · 96 kHz / 24-bit",
      processing: "直通",
      resampled: false,
    });
  });

  it("共享模式写明系统混音，并列出重采样、下混与 DSP", () => {
    const description = describeOutputChain(
      format({
        mode: "shared", sourceRate: 44_100, sourceChannels: 6,
        outputRate: 48_000, outputBits: 32, outputFloat: true,
      }),
      { eqEnabled: true, crossfeedEnabled: true, dspBypassedForDsd: false }
    );
    expect(description.output).toBe("共享（系统混音） · 48 kHz / 32-bit float");
    expect(description.processing).toBe("重采样 44.1 → 48 kHz · 下混 6 → 2 声道 · EQ · Crossfeed");
    expect(description.resampled).toBe(true);
  });

  it("DSD 曲目未开启「对 DSD 生效」时不列出 EQ 与 Crossfeed", () => {
    expect(
      describeOutputChain(format(), {
        eqEnabled: true, crossfeedEnabled: true, dspBypassedForDsd: true,
      }).processing
    ).toBe("直通");
  });

  it("单声道扩展成立体声", () => {
    expect(describeOutputChain(format({ sourceChannels: 1 }), NO_DSP).processing)
      .toBe("扩展 1 → 2 声道");
  });
});

it("formatKiloHertz 保留必要的小数", () => {
  expect(formatKiloHertz(44_100)).toBe("44.1");
  expect(formatKiloHertz(48_000)).toBe("48");
  expect(formatKiloHertz(11_025)).toBe("11.025");
  expect(formatKiloHertz(176_400)).toBe("176.4");
});

it("isDsdPath 识别 DSF / DFF 扩展名", () => {
  expect(isDsdPath("C:/m/a.DSF")).toBe(true);
  expect(isDsdPath("C:/m/a.dff ")).toBe(true);
  expect(isDsdPath("C:/m/a.flac")).toBe(false);
});
