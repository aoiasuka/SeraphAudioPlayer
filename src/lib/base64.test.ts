import { describe, expect, it } from "vitest";
import { bytesToBase64 } from "./base64";

describe("bytesToBase64（FE3-04）", () => {
  it("与标准 base64 一致，含补齐与非文本字节", () => {
    expect(bytesToBase64(new Uint8Array())).toBe("");
    expect(bytesToBase64(new TextEncoder().encode("a"))).toBe("YQ==");
    expect(bytesToBase64(new TextEncoder().encode("ab"))).toBe("YWI=");
    expect(bytesToBase64(new TextEncoder().encode("abc"))).toBe("YWJj");
    expect(bytesToBase64(new Uint8Array([0, 255, 128, 10, 13]))).toBe("AP+ACg0=");
    expect(bytesToBase64(new TextEncoder().encode("[00:01.00]第一行"))).toBe(
      Buffer.from("[00:01.00]第一行", "utf8").toString("base64")
    );
  });

  it("跨越分块边界的大输入逐字节无误（2 MB 的歌词文件上限）", () => {
    const bytes = new Uint8Array(2 * 1024 * 1024 + 7);
    for (let index = 0; index < bytes.length; index += 1) {
      bytes[index] = (index * 31 + 7) & 0xff;
    }
    const encoded = bytesToBase64(bytes);
    expect(encoded.length).toBe(Math.ceil(bytes.length / 3) * 4);
    expect(Buffer.from(encoded, "base64").equals(Buffer.from(bytes))).toBe(true);
  });
});
