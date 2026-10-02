#!/usr/bin/env node
// 零第三方依赖的发布预检；不刷新 lock、不安装工具、不修改版本。
// TOML 只读取本仓库使用的普通表、字符串字段和显式成员列表；不支持的写法直接失败。
import { appendFileSync, readFileSync } from "node:fs";
import { dirname, isAbsolute, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const versionPattern = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?![\s\S])/;
const read = (base, path) => readFileSync(resolve(base, path), "utf8");
const json = (base, path) => JSON.parse(read(base, path));

function uncomment(text) {
  return text.replace(/"(?:\\.|[^"\\])*"|'[^']*'|#[^\r\n]*/g, (part) =>
    part.startsWith("#") ? "" : part,
  );
}

function table(text, name) {
  const lines = uncomment(text).split(/\r?\n/);
  const header = `[${name}]`;
  const starts = lines.flatMap((line, i) => line.trim() === header ? [i] : []);
  if (starts.length !== 1) throw new Error(`缺少或重复 TOML 表 ${header}`);
  const end = lines.findIndex((line, i) => i > starts[0] && /^\s*\[/.test(line));
  return lines.slice(starts[0] + 1, end < 0 ? undefined : end).join("\n");
}

function field(text, key) {
  const escaped = key.replaceAll(".", "\\.");
  const matches = [...text.matchAll(new RegExp(`^\\s*${escaped}\\s*=\\s*(.+?)\\s*$`, "gm"))];
  if (matches.length !== 1) throw new Error(`缺少或重复 TOML 字段 ${key}`);
  const raw = matches[0][1];
  if (/^"(?:[^"\\]|\\.)*"$/.test(raw)) return JSON.parse(raw);
  if (/^'[^']*'$/.test(raw)) return raw.slice(1, -1);
  if (raw === "true") return true;
  throw new Error(`不支持的 TOML 字段写法：${key}`);
}

export function toolchainVersion(base = root) {
  const version = field(table(read(base, "rust-toolchain.toml"), "toolchain"), "channel");
  if (!versionPattern.test(version)) throw new Error("Rust 工具链必须钉到 x.y.z，不接受 stable 等浮动通道");
  return version;
}

export function checkRelease(base = root, tag) {
  if (typeof tag !== "string" || !tag.startsWith("v") || !versionPattern.test(tag.slice(1))) {
    throw new Error("发布 tag 必须是 vX.Y.Z（不接受路径、预发布后缀或前导零）");
  }
  const version = tag.slice(1);
  const equal = (label, actual) => {
    if (actual !== version) throw new Error(`${label} 版本 ${String(actual)} 与 ${tag} 不一致；请先运行 npm run bump ${version}`);
  };
  equal("package.json", json(base, "package.json").version);
  const packageLock = json(base, "package-lock.json");
  equal("package-lock.json", packageLock.version);
  equal('package-lock.json packages[""]', packageLock.packages?.[""]?.version);
  equal("tauri.conf.json", json(base, "src-tauri/tauri.conf.json").version);
  const cargo = read(base, "Cargo.toml");
  const workspaceVersion = field(table(cargo, "workspace.package"), "version");
  equal("Cargo.toml [workspace.package]", workspaceVersion);
  const workspace = table(cargo, "workspace");
  const lists = [...workspace.matchAll(/^\s*members\s*=\s*\[([^\]]*)\]/gm)];
  if (lists.length !== 1) throw new Error("workspace.members 必须是唯一的显式字符串列表");
  const memberText = lists[0][1];
  const members = [...memberText.matchAll(/"([^"\\]*)"|'([^']*)'/g)].map((m) => m[1] ?? m[2]);
  if (!members.length || memberText.replace(/"[^"\\]*"|'[^']*'|[\s,]/g, "")) {
    throw new Error("workspace.members 包含不支持的写法");
  }
  const lock = uncomment(read(base, "Cargo.lock"));
  const lockPackages = lock.split(/^\s*\[\[package\]\]\s*$/m).slice(1).map((block) => {
    // 只读取 package 本表，避免把后续 metadata 子表当作包字段。
    const body = block.split(/^\s*\[/m)[0];
    return { name: field(body, "name"), version: field(body, "version"), external: /^\s*source\s*=/m.test(body) };
  });
  const names = new Set();
  for (const member of members) {
    const path = relative(base, resolve(base, member));
    if (isAbsolute(member) || path.startsWith("..") || /[*?\[\]\\]/.test(member)) {
      throw new Error(`workspace 成员必须是仓库内显式相对路径：${member}`);
    }
    const pkg = table(read(base, `${member}/Cargo.toml`), "package");
    const name = field(pkg, "name");
    if (names.has(name)) throw new Error(`重复 workspace 包名：${name}`);
    names.add(name);
    const inherited = /^\s*version\.workspace\s*=/m.test(pkg);
    const memberVersion = inherited && field(pkg, "version.workspace") === true
      ? workspaceVersion : field(pkg, "version");
    equal(`${member}/Cargo.toml (${name})`, memberVersion);
    const entries = lockPackages.filter((entry) => entry.name === name && !entry.external);
    if (entries.length !== 1) throw new Error(`Cargo.lock 中 workspace 包 ${name} 缺失或重复`);
    equal(`Cargo.lock (${name})`, entries[0].version);
  }
  const notesPath = `docs/releases/${tag}.md`;
  const notes = read(base, notesPath).trim();
  if (!notes) throw new Error(`发布说明为空：${notesPath}`);
  return { tag, version, notesPath, workspacePackages: [...names] };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv[2] === "--toolchain" && process.argv.length === 3) {
      const version = toolchainVersion();
      if (process.env.GITHUB_OUTPUT) appendFileSync(process.env.GITHUB_OUTPUT, `version=${version}\n`);
      if (process.env.GITHUB_ENV) appendFileSync(process.env.GITHUB_ENV, `RUSTUP_TOOLCHAIN=${version}\n`);
      console.log(version);
    } else if (process.argv[2] === "--tag" && process.argv.length === 4) {
      console.log(JSON.stringify(checkRelease(root, process.argv[3]), null, 2));
    } else {
      throw new Error("用法：node scripts/release-check.mjs --toolchain | --tag vX.Y.Z");
    }
  } catch (error) {
    console.error(`发布预检失败：${error.message}`);
    process.exitCode = 1;
  }
}
