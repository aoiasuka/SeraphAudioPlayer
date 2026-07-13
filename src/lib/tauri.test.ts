// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { coverSrc, normalizeIpcError } from "./tauri";

describe("coverSrc", () => {
  it("空值返回空串", () => {
    expect(coverSrc("")).toBe("");
    expect(coverSrc(null)).toBe("");
    expect(coverSrc(undefined)).toBe("");
  });

  it("http/https/data/asset 地址原样返回", () => {
    expect(coverSrc("https://i0.hdslb.com/x.jpg")).toBe(
      "https://i0.hdslb.com/x.jpg"
    );
    expect(coverSrc("data:image/png;base64,AAA")).toBe(
      "data:image/png;base64,AAA"
    );
    expect(coverSrc("asset://localhost/x.jpg")).toBe(
      "asset://localhost/x.jpg"
    );
  });

  it("纯浏览器环境（无 Tauri internals）下本地路径降级为空串", () => {
    expect(coverSrc("C:\\Users\\x\\covers\\a.jpg")).toBe("");
  });

  it("Tauri 环境下本地路径经 convertFileSrc 转换", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      convertFileSrc: (path: string) =>
        `http://asset.localhost/${encodeURIComponent(path)}`,
    };
    try {
      expect(coverSrc("C:\\covers\\a.jpg")).toBe(
        `http://asset.localhost/${encodeURIComponent("C:\\covers\\a.jpg")}`
      );
    } finally {
      delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    }
  });
});

describe("normalizeIpcError", () => {
  it("结构化 { code, message } 对象透传", () => {
    expect(
      normalizeIpcError({ code: "not_found", message: "曲目不存在" })
    ).toEqual({ code: "not_found", message: "曲目不存在" });
  });

  it("字符串错误归一为 internal", () => {
    expect(normalizeIpcError("plain failure")).toEqual({
      code: "internal",
      message: "plain failure",
    });
  });

  it("Error 实例取 message", () => {
    expect(normalizeIpcError(new Error("boom"))).toEqual({
      code: "internal",
      message: "boom",
    });
  });

  it("异常形态兜底 String()", () => {
    expect(normalizeIpcError(42)).toEqual({ code: "internal", message: "42" });
    expect(normalizeIpcError({ code: 1, message: 2 }).code).toBe("internal");
  });
});
