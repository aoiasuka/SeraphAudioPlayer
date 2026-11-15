import { describe, expect, it } from "vitest";
import { bilibiliImportErrorMessage } from "./bilibiliActions";
import { playbackErrorMessage } from "./playbackActions";

describe("结构化错误的提示文案（BE-03 / FE3-01）", () => {
  it("播放错误接受字符串、Error 与 { code, message }", () => {
    expect(playbackErrorMessage("设备被占用")).toBe("播放失败：设备被占用");
    expect(playbackErrorMessage(new Error("boom"))).toBe("播放失败：boom");
    expect(playbackErrorMessage({ code: "internal", message: "format not supported" })).toBe(
      "播放失败：format not supported"
    );
    expect(playbackErrorMessage(undefined)).toBe("播放失败");
    expect(playbackErrorMessage({})).toBe("播放失败");
  });

  it("B 站错误按错误码给文案", () => {
    const message = (code: string, text = "x") => bilibiliImportErrorMessage({ code, message: text });
    expect(message("auth_required")).toContain("需要登录");
    expect(message("link_expired")).toContain("已失效");
    expect(message("timeout")).toContain("超时");
    expect(message("no_audio_stream")).toContain("DASH");
    expect(message("invalid_input", "没有找到有效的 B 站 BV 号或视频链接")).toBe(
      "没有找到有效的 B 站 BV 号或视频链接"
    );
  });

  it("消息里碰巧带 404 / BV 的普通失败不被误分类", () => {
    expect(
      bilibiliImportErrorMessage({ code: "internal", message: "写入 D:/cache/404/BV-live.m4a 失败" })
    ).toBe("导入 B 站音频失败：写入 D:/cache/404/BV-live.m4a 失败");
    expect(bilibiliImportErrorMessage(undefined)).toBe("导入 B 站音频失败");
  });
});
