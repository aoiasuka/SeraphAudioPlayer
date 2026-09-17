import { describe, expect, it } from "vitest";
import {
  isAllowedAmllTtmlDbUrl,
  isPublicHttpsUrl,
  isValidAmllTtmlDbUrl,
  normalizeAmllTtmlDbUrl,
  previewAmllTtmlUrl,
  sanitizeLyricsSourcePriority,
  DEFAULT_AMLL_TTML_DB_URL,
} from "./settings";

describe("AMLL TTML DB 地址（预设模式）", () => {
  it("只接受 amlldb.bikonoo.com，含模板占位也可", () => {
    expect(isAllowedAmllTtmlDbUrl(DEFAULT_AMLL_TTML_DB_URL)).toBe(true);
    expect(isAllowedAmllTtmlDbUrl("https://amlldb.bikonoo.com/{dir}/{id}.ttml")).toBe(true);
  });

  it("拒绝明文、GitHub raw / jsDelivr、伪装、userinfo 与白名单外域", () => {
    for (const url of [
      "http://amlldb.bikonoo.com/x",
      "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main",
      "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main",
      "https://amlldb.bikonoo.com.evil.com/x",
      "https://amlldb.bikonoo.com@evil.com/x",
      "https://bikonoo.com/x",
      "not a url",
      "",
    ]) {
      expect(isAllowedAmllTtmlDbUrl(url), url).toBe(false);
    }
  });
});

describe("AMLL TTML DB 地址（自定义模式）", () => {
  it("任意公网 https 域名放行，模板占位不影响解析", () => {
    expect(isPublicHttpsUrl("https://amlldb.bikonoo.com/ncm-lyrics/%s.ttml")).toBe(true);
    expect(isPublicHttpsUrl("https://amlldb.bikonoo.com/{dir}/{id}.ttml")).toBe(true);
    expect(isValidAmllTtmlDbUrl("https://amlldb.bikonoo.com", true)).toBe(true);
  });

  it("拒绝明文、IP 直连、localhost、内网后缀与无点主机", () => {
    for (const url of [
      "http://amlldb.bikonoo.com/x",
      "https://127.0.0.1/x",
      "https://[::1]/x",
      "https://10.0.0.5/x",
      "https://localhost/x",
      "https://nas/x",
      "https://printer.local/x",
      "https://db.internal/x",
      "https://user@amlldb.bikonoo.com/x",
    ]) {
      expect(isPublicHttpsUrl(url), url).toBe(false);
    }
  });
});

describe("模板展开与规整", () => {
  it("previewAmllTtmlUrl 展开 {dir}/{id}/%s，base URL 自动补路径", () => {
    expect(previewAmllTtmlUrl("https://amlldb.bikonoo.com", "qq-lyrics", "7")).toBe(
      "https://amlldb.bikonoo.com/qq-lyrics/7.ttml"
    );
    expect(previewAmllTtmlUrl(DEFAULT_AMLL_TTML_DB_URL, "qq-lyrics", "7")).toBe(
      "https://amlldb.bikonoo.com/ncm-lyrics/7.ttml"
    );
    expect(previewAmllTtmlUrl("https://a.b/{dir}/{id}.ttml/", "ncm-lyrics", "1")).toBe(
      "https://a.b/ncm-lyrics/1.ttml"
    );
  });

  it("规整尾部斜杠，空串回默认", () => {
    expect(normalizeAmllTtmlDbUrl(" https://a.example.org/x/// ")).toBe("https://a.example.org/x");
    expect(normalizeAmllTtmlDbUrl("")).toBe(DEFAULT_AMLL_TTML_DB_URL);
  });
});

describe("sanitizeLyricsSourcePriority", () => {
  it("未知值回 auto", () => {
    expect(sanitizeLyricsSourcePriority("qq")).toBe("qq");
    expect(sanitizeLyricsSourcePriority("spotify")).toBe("auto");
    expect(sanitizeLyricsSourcePriority(undefined)).toBe("auto");
  });
});
