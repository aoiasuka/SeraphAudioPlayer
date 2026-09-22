// 歌词解析微基准：合成 LRC / 增强 LRC / QRC / YRC / KRC / TTML / 加密 QRC / 曲库歌词 JSON，
// 走生产解析函数（cargo test --release 的 #[ignore] 基准），输出到 target/optimization-lyrics-benchmark.json。
// 合成数据只代表解析器本身的相对耗时，不代表桌面端真实延迟。
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = path.join(root, "target");
fs.mkdirSync(target, { recursive: true });
const stdout = execFileSync(
  "cargo",
  ["test", "--release", "-p", "seraph-tauri", "--lib", "lyrics_bench", "--", "--ignored", "--nocapture", "--test-threads=1"],
  { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "inherit"], maxBuffer: 64 * 1024 * 1024 },
);
// 首条 JSON 与测试框架的 `test … ... ` 前缀同行，按对象字面量匹配而不是按行首
const cases = [...stdout.matchAll(/\{"case":[^\n]*?\}/g)].map((match) => JSON.parse(match[0]));
const report = {
  measuredAt: new Date().toISOString(),
  rustc: execFileSync("rustc", ["--version"], { cwd: root, encoding: "utf8" }).trim(),
  platform: process.platform,
  arch: process.arch,
  cpu: os.cpus()[0]?.model,
  notes: [
    "每项先预热再按目标 0.4 s 自适应迭代次数取均值；单位 µs/次。",
    "输入为合成歌词（300 行 × 24 音节），不含磁盘 I/O、IPC 与前端渲染。",
    "加密 QRC 用生产同款 QQ 变体 3DES 加密后再走解密 + 解析全链。",
  ],
  cases,
};
fs.writeFileSync(path.join(target, "optimization-lyrics-benchmark.json"), JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify(report, null, 2));
