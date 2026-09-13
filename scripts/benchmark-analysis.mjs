// 在 target 中附加入口并编译生产分析引擎；只用合成历史，不打开声卡。
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = path.join(root, "target");
fs.mkdirSync(target, { recursive: true });
const source = fs.readFileSync(path.join(root, "crates/seraph-visualizer/src/analysis.rs"), "utf8");
const harness = fs.readFileSync(path.join(root, "scripts/benchmark-analysis-harness.rs"), "utf8");
const generated = path.join(target, "optimization-analysis-current.rs");
const binary = path.join(target, "optimization-analysis-current" + (process.platform === "win32" ? ".exe" : ""));
fs.writeFileSync(generated, source + "\n" + harness);
execFileSync("rustc", ["--edition=2021", "-O", generated, "-o", binary], { cwd: root, stdio: "inherit" });
const stdout = execFileSync(binary, [], { cwd: root, encoding: "utf8" });
const report = {
  measuredAt: new Date().toISOString(),
  rustc: execFileSync("rustc", ["--version"], { cwd: root, encoding: "utf8" }).trim(),
  platform: process.platform, arch: process.arch, cpu: os.cpus()[0]?.model,
  sourceSha256: createHash("sha256").update(source).digest("hex"),
  warmups: 10, samples: 100, hotBatchSize: 1000, coldBatchSize: 1,
  notes: [
    "热读每组 1000 次快照取均值，以降低计时器分辨率影响；冷读每次在计时前清除 I/LRA 缓存。",
    "只填充合成响度历史，没有 PCM 输入，不含波形、FFT、IPC、设备输出或线程调度。",
    "入口依赖 analysis.rs 内部字段；字段变化时应同步维护 harness。",
  ],
  history: stdout.trim().split(/\r?\n/).filter(Boolean).map((line) => JSON.parse(line)),
};
fs.writeFileSync(path.join(target, "optimization-analysis-benchmark.json"), JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify(report, null, 2));
