/**
 * 输出链路的文字描述（右侧「音频信息」卡片用）。
 *
 * 数据来自引擎建流成功后发布的 `output_format` 事件。共享模式下「输出格式」是交给
 * 系统混音器的格式，设备最终工作在什么格式由 Windows 决定，这里拿不到——所以共享
 * 模式的文案明确写「系统混音」，不暗示它就是设备格式。
 */

export interface OutputFormatInfo {
  trackId: string;
  mode: "shared" | "exclusive";
  sourceRate: number;
  /** 源位深；解码器不提供时为 0 */
  sourceBits: number;
  sourceChannels: number;
  outputRate: number;
  /** 输出有效位深 */
  outputBits: number;
  outputFloat: boolean;
  outputChannels: number;
}

export interface OutputChainDsp {
  /** 均衡器总开关 */
  eqEnabled: boolean;
  crossfeedEnabled: boolean;
  /** 当前曲目是 DSD 且「对 DSD 生效」未开启：整条 DSP 被旁路 */
  dspBypassedForDsd: boolean;
}

export interface OutputChainDescription {
  /** 例：`独占 · 96 kHz / 24-bit` */
  output: string;
  /** 例：`重采样 96 → 48 kHz · EQ`；没有任何处理时为 `直通` */
  processing: string;
  /** 是否发生了重采样（界面据此着色提示） */
  resampled: boolean;
}

function positiveInt(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0
    ? Math.trunc(value)
    : null;
}

/** 校验事件载荷；字段缺失或越界返回 null（旧后端、畸形事件一律不显示）。 */
export function parseOutputFormatEvent(event: Record<string, unknown>): OutputFormatInfo | null {
  const trackId = event.track_id ?? event.trackId;
  const mode = event.mode;
  const sourceRate = positiveInt(event.source_rate);
  const sourceChannels = positiveInt(event.source_channels);
  const outputRate = positiveInt(event.output_rate);
  const outputBits = positiveInt(event.output_bits);
  const outputChannels = positiveInt(event.output_channels);
  if (typeof trackId !== "string" || !trackId) return null;
  if (mode !== "shared" && mode !== "exclusive") return null;
  if (!sourceRate || !sourceChannels || !outputRate || !outputBits || !outputChannels) return null;
  const sourceBits =
    typeof event.source_bits === "number" && Number.isFinite(event.source_bits)
      ? Math.max(0, Math.trunc(event.source_bits))
      : 0;
  return {
    trackId,
    mode,
    sourceRate,
    sourceBits,
    sourceChannels,
    outputRate,
    outputBits,
    outputFloat: event.output_float === true,
    outputChannels,
  };
}

/** 44100 → `44.1`，48000 → `48`，11025 → `11.025`。 */
export function formatKiloHertz(rate: number): string {
  return String(Number((rate / 1000).toFixed(3)));
}

export function describeOutputChain(
  format: OutputFormatInfo,
  dsp: OutputChainDsp
): OutputChainDescription {
  const bits = `${format.outputBits}-bit${format.outputFloat ? " float" : ""}`;
  const modeLabel = format.mode === "exclusive" ? "独占" : "共享（系统混音）";
  const output = `${modeLabel} · ${formatKiloHertz(format.outputRate)} kHz / ${bits}`;

  const steps: string[] = [];
  const resampled = format.sourceRate !== format.outputRate;
  if (resampled) {
    steps.push(
      `重采样 ${formatKiloHertz(format.sourceRate)} → ${formatKiloHertz(format.outputRate)} kHz`
    );
  }
  if (format.sourceChannels > format.outputChannels) {
    steps.push(`下混 ${format.sourceChannels} → ${format.outputChannels} 声道`);
  } else if (format.sourceChannels < format.outputChannels) {
    steps.push(`扩展 ${format.sourceChannels} → ${format.outputChannels} 声道`);
  }
  if (!dsp.dspBypassedForDsd) {
    if (dsp.eqEnabled) steps.push("EQ");
    if (dsp.crossfeedEnabled) steps.push("Crossfeed");
  }
  return { output, processing: steps.length > 0 ? steps.join(" · ") : "直通", resampled };
}

/** DSD 源文件（DSF / DFF）：均衡器默认不对它生效。 */
export function isDsdPath(path: string): boolean {
  return /\.(dsf|dff)$/i.test(path.trim());
}
