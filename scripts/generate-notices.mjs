// 收集第三方依赖许可证，不读取用户曲库或凭据。
import { readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, resolve, relative, isAbsolute } from "node:path";
import { execFileSync } from "node:child_process";
const root = resolve(import.meta.dirname, "..");
const lock = JSON.parse(readFileSync(resolve(root, "package-lock.json"), "utf8"));
const notices = ["Seraph Audio Player — 前端第三方组件许可\n\n以下许可文本来自已安装的生产依赖，包括随应用分发的 Courier Prime 与 Noto Sans SC 字体。\n组件保留各自版权与许可证，项目 MIT 许可不替代这些许可。\n"];
for (const [location, item] of Object.entries(lock.packages).sort(([a], [b]) => a.localeCompare(b))) {
  if (!location || item.dev || !location.startsWith("node_modules/")) continue;
  const dir = resolve(root, location);
  const rel = relative(resolve(root, "node_modules"), dir);
  if (rel.startsWith("..") || isAbsolute(rel)) throw new Error("依赖路径越界");
  let packageJson;
  try { packageJson = JSON.parse(readFileSync(resolve(dir, "package.json"), "utf8")); }
  catch { if (item.optional) continue; throw new Error(`依赖未安装：${location}`); }
  const names = readdirSync(dir).filter((name) => /^(licen[sc]e|copying|notice)([._-]|$)/i.test(name) && statSync(resolve(dir, name)).isFile());
  notices.push(`\n${"=".repeat(72)}\n${packageJson.name} ${packageJson.version} — ${packageJson.license ?? item.license ?? "请参阅上游"}\n`);
  if (names.length) {
    for (const name of names.sort()) notices.push(`\n--- ${name} ---\n${readFileSync(resolve(dir, name), "utf8").trim()}\n`);
  } else {
    notices.push(`上游 ${typeof packageJson.repository === "string" ? packageJson.repository : packageJson.repository?.url ?? "https://www.npmjs.com/package/" + packageJson.name}\n未在包根目录找到许可文件，请查阅上游完整条款。\n`);
  }
}
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--locked", "--format-version", "1", "--filter-platform", "x86_64-pc-windows-msvc"], {
  cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024,
}));
notices.push("\nRust 依赖许可（包含构建时依赖，部分组件只用于工具链）\n");
const seen = new Set();
for (const pkg of metadata.packages.filter((pkg) => pkg.source).sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version))) {
  const dir = dirname(pkg.manifest_path);
  notices.push(`\n${"=".repeat(72)}\n${pkg.name} ${pkg.version} — ${pkg.license ?? "请参阅上游"}\n`);
  const names = readdirSync(dir).filter((name) => /^(licen[sc]e|copying|notice)([._-]|$)/i.test(name) && statSync(resolve(dir, name)).isFile());
  if (pkg.license_file) {
    const file = resolve(dir, pkg.license_file);
    const rel = relative(dir, file);
    if (rel.startsWith("..") || isAbsolute(rel)) throw new Error(`许可文件越界：${pkg.name}`);
    if (!names.includes(rel)) names.push(rel);
  }
  for (const name of names.sort()) {
    const content = readFileSync(resolve(dir, name), "utf8").trim();
    if (seen.has(content)) {
      notices.push(`许可 ${name} 与本文件前文相同。\n`);
    } else {
      seen.add(content);
      notices.push(`\n--- ${name} ---\n${content}\n`);
    }
  }
  if (!names.length) notices.push(`许可全文见上游源码包：https://crates.io/crates/${pkg.name}/${pkg.version}\n`);
}
// 只规范化行尾空白，保留上游许可正文与版权文字。
const output = notices.join("").replace(/[\t ]+$/gm, "");
writeFileSync(resolve(root, "THIRD-PARTY-NOTICES.txt"), output);
console.log(`已生成第三方依赖许可，${output.length} 字符`);
