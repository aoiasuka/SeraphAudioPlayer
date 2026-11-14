import { describe, expect, it } from "vitest";
import { combinedResponseDb, logFreqPoints } from "@/lib/eqResponse";
import { GENRE_EQ_PRESETS } from "@/lib/eqPresets";
import { emptyLyricDocument } from "@/lib/lyrics/document";
import { recacheOptionsForTrack } from "@/store/player/bilibiliActions";
import type { Track } from "@/types/track";

function streamingTrack(format: string): Track {
  return {
    id: "bilibili-bv1xx411c7md-1", title: "t", artist: "up", album: "Bilibili", cover: "",
    format, bitdepth: "", bitrate: "", channels: "", size: "", duration: 100,
    path: "C:/cache/BV1xx411c7mD-1.m4a", glowColor: "", lyrics: emptyLyricDocument(),
    sourceId: "BV1xx411c7mD", cacheMissing: true,
  } as Track;
}

describe("自动重新缓存沿用原来的音质（BILI5-01）", () => {
  it("普通音质的曲目不会被升成无损或杜比", () => {
    expect(recacheOptionsForTrack(streamingTrack("M4A"))).toMatchObject({
      preferFlac: false,
      preferDolbyAtmos: false,
    });
    expect(recacheOptionsForTrack(streamingTrack("OPUS"))).toMatchObject({
      preferFlac: false,
      preferDolbyAtmos: false,
    });
  });

  it("原来是 FLAC 的仍按 FLAC，原来是杜比的仍按杜比", () => {
    expect(recacheOptionsForTrack(streamingTrack("FLAC"))).toMatchObject({
      preferFlac: true,
      preferDolbyAtmos: false,
    });
    expect(recacheOptionsForTrack(streamingTrack("eac3"))).toMatchObject({
      preferFlac: false,
      preferDolbyAtmos: true,
    });
  });

  it("带上原曲目 id，下载期间被删除的记录不会被重新插入", () => {
    expect(recacheOptionsForTrack(streamingTrack("M4A")).existingTrackId)
      .toBe("bilibili-bv1xx411c7md-1");
  });
});

describe("内置曲风预设的预放大（EQ6-01）", () => {
  // 频段在常见采样率下的合成响应都不应高于 0 dB，否则满幅素材会被削波
  const SAMPLE_RATES = [44_100, 48_000, 96_000, 192_000];

  it.each(GENRE_EQ_PRESETS.map((preset) => [preset.id, preset] as const))(
    "%s：含预放大的合成响应峰值不超过 0 dB",
    (_id, preset) => {
      for (const sampleRate of SAMPLE_RATES) {
        const freqs = logFreqPoints(600, 10, Math.min(22_000, sampleRate / 2 - 1));
        const peak = Math.max(
          ...combinedResponseDb(preset.preamp, preset.bands, freqs, sampleRate)
        );
        expect(peak, `${preset.id} @ ${sampleRate} Hz`).toBeLessThanOrEqual(0.01);
      }
    }
  );

  it("预放大不过度保守：留出的余量不超过 1 dB；平直预设保持 0", () => {
    for (const preset of GENRE_EQ_PRESETS) {
      const peaks = SAMPLE_RATES.map((sampleRate) =>
        Math.max(
          ...combinedResponseDb(
            preset.preamp,
            preset.bands,
            logFreqPoints(600, 10, Math.min(22_000, sampleRate / 2 - 1)),
            sampleRate
          )
        )
      );
      expect(Math.max(...peaks), preset.id).toBeGreaterThan(-1);
    }
    expect(GENRE_EQ_PRESETS.find((preset) => preset.id === "flat")?.preamp).toBe(0);
  });
});
