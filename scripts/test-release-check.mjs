import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { checkRelease, toolchainVersion } from "./release-check.mjs";

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "seraph-release-check-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const put = (path, content) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), content);
  };
  put("package.json", JSON.stringify({ version: "1.2.3" }));
  put("package-lock.json", JSON.stringify({ version: "1.2.3", packages: { "": { version: "1.2.3" } } }));
  put("src-tauri/tauri.conf.json", JSON.stringify({ version: "1.2.3" }));
  put("Cargo.toml", '[workspace]\nmembers = [\n "crates/engine", # 成员\n "src-tauri",\n]\n[workspace.package]\nversion = "1.2.3"\n');
  put("crates/engine/Cargo.toml", '[package]\nname = "engine"\nversion.workspace = true\n');
  put("src-tauri/Cargo.toml", '[package]\nname = "app"\nversion = "1.2.3"\n');
  put("Cargo.lock", 'version = 4\n[[package]]\nname = "engine"\nversion = "1.2.3"\n[[package]]\nname = "app"\nversion = "1.2.3"\n[[package]]\nname = "external"\nversion = "9.9.9"\nsource = "registry+https://example.invalid"\n');
  put("rust-toolchain.toml", '[toolchain]\nchannel = "1.98.1"\n');
  put("docs/releases/v1.2.3.md", "# 更新\n\n修复问题。\n");
  const replace = (path, before, after) => put(path, readFileSync(join(root, path), "utf8").replace(before, after));
  return { root, put, replace };
}

test("一致的五类版本、继承和显式 workspace 版本通过，第三方包无需同版", (t) => {
  const { root } = fixture(t);
  assert.deepEqual(checkRelease(root, "v1.2.3").workspacePackages, ["engine", "app"]);
  assert.equal(toolchainVersion(root), "1.98.1");
});

for (const [label, path, before, after] of [
  ["package", "package.json", "1.2.3", "1.2.4"],
  ["npm lock 顶层", "package-lock.json", '"version":"1.2.3"', '"version":"1.2.4"'],
  ["npm lock 根包", "package-lock.json", '"":{"version":"1.2.3"}', '"":{"version":"1.2.4"}'],
  ["Tauri", "src-tauri/tauri.conf.json", "1.2.3", "1.2.4"],
  ["workspace", "Cargo.toml", "1.2.3", "1.2.4"],
  ["成员清单", "src-tauri/Cargo.toml", "1.2.3", "1.2.4"],
  ["Cargo lock 成员", "Cargo.lock", "1.2.3", "1.2.4"],
]) {
  test(`${label} 漂移必须失败`, (t) => {
    const { root, replace } = fixture(t);
    replace(path, before, after);
    assert.throws(() => checkRelease(root, "v1.2.3"), /不一致/);
  });
}

for (const tag of [undefined, "1.2.3", "v01.2.3", "v1.2", "v1.2.3-beta.1", "v1.2.3/../../secret", "v1.2.3\n"]) {
  test(`拒绝非法 tag ${JSON.stringify(tag)}`, (t) => {
    const { root } = fixture(t);
    assert.throws(() => checkRelease(root, tag), /tag 必须/);
  });
}

for (const content of ["", " \n\t\r\n"]) {
  test("空发布说明失败", (t) => {
    const { root, put } = fixture(t);
    put("docs/releases/v1.2.3.md", content);
    assert.throws(() => checkRelease(root, "v1.2.3"), /发布说明为空/);
  });
}

test("缺少发布说明失败", (t) => {
  const { root } = fixture(t);
  rmSync(join(root, "docs/releases/v1.2.3.md"));
  assert.throws(() => checkRelease(root, "v1.2.3"), /ENOENT/);
});

for (const mode of ["缺失", "重复", "同名外部包"]) {
  test(`Cargo.lock 成员${mode}不能冒充正确记录`, (t) => {
    const { root, put } = fixture(t);
    const app = '[[package]]\nname = "app"\nversion = "1.2.3"\n';
    const engine = '[[package]]\nname = "engine"\nversion = "1.2.3"\n';
    put("Cargo.lock", app + (mode === "缺失" ? "" : mode === "重复" ? engine + engine : engine + 'source = "registry+https://example.invalid"\n'));
    assert.throws(() => checkRelease(root, "v1.2.3"), /缺失或重复/);
  });
}

for (const channel of ["stable", "nightly", "1.98", "1.98.1\nBAD=1"]) {
  test(`工具链拒绝浮动或非法版本 ${JSON.stringify(channel)}`, (t) => {
    const { root, put } = fixture(t);
    put("rust-toolchain.toml", `[toolchain]\nchannel = ${JSON.stringify(channel)}\n`);
    assert.throws(() => toolchainVersion(root), /必须钉到/);
  });
}

test("兼容 CRLF、单引号和行尾注释", (t) => {
  const { root, put } = fixture(t);
  put("rust-toolchain.toml", "[toolchain]\r\nchannel = '1.98.1' # 钉版\r\n");
  assert.equal(toolchainVersion(root), "1.98.1");
});

test("缺少 npm lock 根包也失败", (t) => {
  const { root, put } = fixture(t);
  put("package-lock.json", '{"version":"1.2.3"}');
  assert.throws(() => checkRelease(root, "v1.2.3"), /不一致/);
});

test("不支持的 workspace 通配路径失败而非漏检", (t) => {
  const { root, replace } = fixture(t);
  replace("Cargo.toml", "crates/engine", "crates/*");
  assert.throws(() => checkRelease(root, "v1.2.3"), /显式相对路径/);
});
