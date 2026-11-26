import { describe, expect, it } from "vitest";
import { m3u8BlockedNote, m3u8ImportSummary } from "./libraryActions";

describe("M3U8 导入提示（PL-01）", () => {
  it("没有被拦下的条目时与原来的文案一致", () => {
    expect(m3u8ImportSummary(12, { skipped: 0 })).toBe("12 首");
    expect(m3u8ImportSummary(12, { skipped: 3 })).toBe("12 首，跳过 3 条");
    expect(m3u8ImportSummary(12, { skipped: 3, truncated: true })).toBe(
      "12 首，跳过 3 条，清单超长已截断"
    );
    expect(m3u8BlockedNote({})).toBe("");
    expect(m3u8BlockedNote({ blocked: 0, blockedHosts: ["nas"] })).toBe("");
  });

  it("被拦下的网络路径单独说明原因与补救办法，不混在「跳过」里", () => {
    const summary = m3u8ImportSummary(5, {
      skipped: 6,
      blocked: 4,
      blockedHosts: ["attacker.example", "nas"],
    });
    expect(summary).toContain("5 首，跳过 2 条，");
    expect(summary).toContain("4 条指向网络共享或设备路径（attacker.example、nas）");
    expect(summary).toContain("未导入");
    expect(summary).toContain("拖进窗口");
    expect(summary).toContain("重新导入清单");
  });

  it("全部跳过的条目都是被拦下的时，不再报「跳过 0 条」", () => {
    const summary = m3u8ImportSummary(1, { skipped: 2, blocked: 2, blockedHosts: [] });
    expect(summary).not.toContain("跳过");
    expect(summary).toContain("2 条指向网络共享或设备路径，");
  });

  it("设备路径没有主机名，空白主机名不显示", () => {
    expect(m3u8BlockedNote({ blocked: 1, blockedHosts: ["  ", ""] })).not.toContain("（");
    expect(m3u8BlockedNote({ blocked: 1 })).toMatch(/^1 条指向网络共享或设备路径，/);
  });

  it("旧后端没有 blocked 字段、或计数不自洽时不出现负数", () => {
    expect(m3u8ImportSummary(3, { skipped: 1, blocked: 5 })).not.toContain("跳过");
    expect(m3u8ImportSummary(3, { skipped: 2, blocked: -1 })).toBe("3 首，跳过 2 条");
  });
});
