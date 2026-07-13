#!/usr/bin/env node
/**
 * 版本号统一升级脚本：一次同步三处版本声明 + 刷新两个 lock 文件。
 *
 * 用法：npm run bump 0.4.0
 *
 * 覆盖：
 * - package.json            "version"
 * - src-tauri/tauri.conf.json "version"
 * - Cargo.toml              [workspace.package] version
 * - package-lock.json / Cargo.lock（分别经 npm / cargo 刷新）
 */
import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";

const version = process.argv[2]?.trim();
if (!version || !/^\d+\.\d+\.\d+$/.test(version)) {
  console.error("用法: npm run bump <x.y.z>  例如: npm run bump 0.4.0");
  process.exit(1);
}

const root = resolve(import.meta.dirname, "..");

function patchJson(relPath) {
  const path = resolve(root, relPath);
  const source = readFileSync(path, "utf8");
  const patched = source.replace(
    /("version"\s*:\s*")\d+\.\d+\.\d+(")/,
    `$1${version}$2`
  );
  if (patched === source) {
    throw new Error(`${relPath} 中未找到 version 字段`);
  }
  writeFileSync(path, patched);
  console.log(`✓ ${relPath} -> ${version}`);
}

function patchWorkspaceCargo() {
  const path = resolve(root, "Cargo.toml");
  const source = readFileSync(path, "utf8");
  // 只替换 [workspace.package] 段内的 version
  const patched = source.replace(
    /(\[workspace\.package\][^[]*?version\s*=\s*")\d+\.\d+\.\d+(")/s,
    `$1${version}$2`
  );
  if (patched === source) {
    throw new Error("Cargo.toml 中未找到 [workspace.package] version");
  }
  writeFileSync(path, patched);
  console.log(`✓ Cargo.toml [workspace.package] -> ${version}`);
}

patchJson("package.json");
patchJson("src-tauri/tauri.conf.json");
patchWorkspaceCargo();

console.log("刷新 lock 文件…");
execSync("npm install --package-lock-only", { cwd: root, stdio: "inherit" });
// --workspace 只更新本仓库成员的版本记录，不动第三方依赖
execSync("cargo update --workspace --offline", { cwd: root, stdio: "inherit" });
console.log(`✓ 全部完成：版本已同步为 ${version}`);
