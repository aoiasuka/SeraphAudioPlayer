// 合成数据微基准：不读取音乐、不连接 Tauri、不启动浏览器。
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { createHash } from "node:crypto";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = fileURLToPath(new URL("../", import.meta.url));
const warmups = 5;
const samples = 25;
const sourceHashes = {};
let checksum = 0;

async function loadSource(file, stubInvoke = false) {
  const source = fs.readFileSync(path.join(root, file), "utf8");
  sourceHashes[file] = createHash("sha256").update(source).digest("hex");
  let js = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  if (stubInvoke) {
    const declaration = 'import { invoke } from "@/lib/tauri";';
    if (js.split(declaration).length !== 2) throw new Error("请同步更新基准的 IPC 适配入口");
    js = js.replace(declaration,
      "let invoke; export function setBenchmarkInvoke(callback) { invoke = callback; }");
  }
  return import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));
}

function summarize(durations) {
  durations.sort((a, b) => a - b);
  return {
    p50Ms: +durations[Math.ceil(samples * 0.50) - 1].toFixed(4),
    p95Ms: +durations[Math.ceil(samples * 0.95) - 1].toFixed(4),
  };
}

function measure(prepare, run) {
  const durations = [];
  for (let index = 0; index < warmups + samples; index++) {
    prepare(index);
    const start = performance.now();
    const result = run();
    const elapsed = performance.now() - start;
    checksum += result.length;
    if (index >= warmups) durations.push(elapsed);
  }
  return summarize(durations);
}

async function measureAsync(prepare, run) {
  const durations = [];
  for (let index = 0; index < warmups + samples; index++) {
    prepare(index);
    const start = performance.now();
    await run();
    const elapsed = performance.now() - start;
    if (index >= warmups) durations.push(elapsed);
  }
  return summarize(durations);
}

function makePlaylist(size) {
  let seed = 20260913;
  const random = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0);
  return Array.from({ length: size }, (_, index) => ({
    id: "local-" + String(index).padStart(8, "0"),
    path: "D:/Music/Album " + Math.floor(index / 12) + "/Track " + index + ".flac",
    title: "曲目 " + random(),
    artist: "艺术家 " + (index % 1000),
    album: "专辑 " + Math.floor(index / 12),
    cover: "C:/MusicCache/covers/" + String(index % 3000).padStart(16, "0") + ".jpg",
    duration: 180 + index % 120,
  }));
}

function buildAssets() {
  const directory = path.join(root, "dist", "assets");
  if (!fs.existsSync(directory)) return null;
  const files = fs.readdirSync(directory, { withFileTypes: true })
    .filter((entry) => entry.isFile())
    .map((entry) => ({ name: entry.name, bytes: fs.statSync(path.join(directory, entry.name)).size }));
  const group = (suffix) => {
    const matching = files.filter((file) => file.name.endsWith(suffix));
    return { count: matching.length, bytes: matching.reduce((total, file) => total + file.bytes, 0) };
  };
  const html = fs.readFileSync(path.join(root, "dist", "index.html"), "utf8");
  const entry = html.match(/<script\b[^>]*\bsrc="\/assets\/([^"]+\.js)"/);
  return {
    mainEntry: files.find((file) => file.name === entry?.[1]) ?? null,
    javascript: group(".js"), css: group(".css"),
    woff: group(".woff"), woff2: group(".woff2"),
    all: { count: files.length, bytes: files.reduce((total, file) => total + file.bytes, 0) },
    deferredPages: files.filter((file) => /^(AnalysisPage|EqPage|StreamingPage)-.*\.js$/.test(file.name)),
  };
}

const { filterAndSortTracks } = await loadSource("src/components/pages/main-pages/trackFilters.ts");
const queue = await loadSource("src/store/player/queueSync.ts", true);
let state;
let serialized = "";
let lastArgs;
queue.setBenchmarkInvoke(async (command, args) => {
  if (command !== "sync_playback_queue") throw new Error("基准意外调用其他命令");
  lastArgs = args;
  serialized = JSON.stringify(args);
  return {
    currentTrackId: state.playlist[state.currentTrackIndex]?.id ?? null,
    nextTrackId: null, shuffleMode: state.shuffleMode,
  };
});
const get = () => state;
const set = (update) => { state = { ...state, ...update }; };
const sync = () => queue.syncPlaybackQueue(get, set);
const reset = (playlist) => {
  queue.resetPlaybackQueueSync();
  state = {
    playlist, currentTrackIndex: 0, recentTrackIds: [],
    shuffleMode: false, loopMode: false, playbackQueuePreview: null,
  };
};
const payload = (timings, expectFull) => {
  if (Object.hasOwn(lastArgs, "tracks") !== expectFull) throw new Error("队列请求不符合当前基准场景");
  checksum += serialized.length;
  return { ...timings, utf8Bytes: Buffer.byteLength(serialized) };
};

const results = [];
for (const size of [1000, 10000, 50000]) {
  const playlist = makePlaylist(size);
  let coldPlaylist;
  const coldTitleQuery = measure(
    () => { coldPlaylist = playlist.map((track) => ({ ...track })); },
    () => filterAndSortTracks(coldPlaylist, "曲目", "title"),
  );
  const hotDefaultQuery = measure(() => {}, () => filterAndSortTracks(playlist, "曲目", "default"));
  const hotTitleQuery = measure(() => {}, () => filterAndSortTracks(playlist, "曲目", "title"));
  const fullQueue = payload(await measureAsync(() => reset(playlist.slice()), sync), true);
  reset(playlist);
  await sync();
  const incrementalQueue = payload(await measureAsync((index) => {
    const currentTrackIndex = (index + 1) % size;
    state = { ...state, currentTrackIndex, recentTrackIds: [playlist[currentTrackIndex].id] };
  }, sync), false);
  const lyricsQueue = payload(await measureAsync((index) => {
    // store 的不可变数组更新在计时外；计时包括生产代码的队列内容复核。
    const next = state.playlist.slice();
    next[index % size] = { ...next[index % size], lyricsLoaded: true, lyrics: [{ time: 1, text: "歌词 " + index }] };
    state = { ...state, playlist: next };
  }, sync), false);
  results.push({ tracks: size, coldTitleQuery, hotDefaultQuery, hotTitleQuery, fullQueue, incrementalQueue, lyricsQueue });
}

const report = {
  measuredAt: new Date().toISOString(), node: process.version,
  platform: process.platform, arch: process.arch, cpu: os.cpus()[0]?.model,
  seed: 20260913, warmups, samples, sourceHashes, checksum,
  notes: [
    "首次排序每轮使用全新曲目引用，合成数据创建不计时；连续查询保留同一不可变曲库版本。",
    "队列计时包含生产同步函数和 JSON 编码；不含 store 更新、WebView IPC、Rust 反序列化或声卡。",
    "全量场景每轮清除确认状态并使用新数组；增量和歌词场景保留已确认版本。",
    "资源为最近一次 dist/assets 构建的未压缩字节数，不代表启动下载量或安装包体积。",
  ],
  library: results, assets: buildAssets(),
};
fs.mkdirSync(path.join(root, "target"), { recursive: true });
fs.writeFileSync(path.join(root, "target", "optimization-frontend-benchmark.json"), JSON.stringify(report, null, 2) + "\n");
console.log(JSON.stringify(report, null, 2));
