/**
 * 应用配置一键导出 / 导入（v0.4.8 设置备份）。
 *
 * 导出：打包 localStorage 里的 persist store——播放偏好（仅设置字段，不含
 * 收藏/歌单/播放痕迹等个人数据）、EQ 与 DSP、声学分析设置——为一个 JSON。
 * 导入：结构校验后先暂存 sessionStorage 并重载界面；下次启动最早阶段
 * （main.tsx 首个 import 的 boot 模块调用 applyPendingConfigImport）才写回
 * localStorage——绕开 player persist 300ms 防抖在 pagehide flush 时用旧状态
 * 覆盖刚写入数据的竞态，也保证所有 store 水合时读到的已是导入值。
 */

export const CONFIG_EXPORT_KIND = "seraph-config";
export const CONFIG_EXPORT_VERSION = 1;
const PENDING_IMPORT_KEY = "seraph-config-import-pending";

const PLAYER_STATE_KEY = "seraph-player-state";
const EQ_STATE_KEY = "seraph-eq-state";
const ANALYSIS_STATE_KEY = "seraph-analysis-settings";

/** 纳入导出的 persist store（localStorage 键名） */
export const CONFIG_STORE_KEYS = [
  PLAYER_STATE_KEY,
  EQ_STATE_KEY,
  ANALYSIS_STATE_KEY,
] as const;

/**
 * player store 里属于"设置"的字段与类型（收藏 / 歌单 / 最近播放 / 播放位置
 * 等个人数据不导出，导入时也不会覆盖本机数据）。
 */
const PLAYER_SETTINGS_FIELDS: Record<string, "boolean" | "number" | "string"> = {
  volume: "number",
  isMuted: "boolean",
  previousVolume: "number",
  shuffleMode: "boolean",
  loopMode: "boolean",
  currentDeviceId: "string",
  driverKind: "string",
  activeView: "string",
  smtcEnabled: "boolean",
  rememberPlayback: "boolean",
};

interface PersistedEnvelope {
  state: Record<string, unknown>;
  version?: number;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

/** 解析 zustand persist 落盘格式 { state, version }；坏结构返回 null */
function parseEnvelope(raw: string | null): PersistedEnvelope | null {
  if (!raw) return null;
  try {
    const parsed = asRecord(JSON.parse(raw));
    const state = parsed ? asRecord(parsed.state) : null;
    if (!parsed || !state) return null;
    return {
      state,
      version: typeof parsed.version === "number" ? parsed.version : undefined,
    };
  } catch {
    return null;
  }
}

/** 逐字段类型校验地挑出 player 设置字段 */
function pickPlayerSettings(state: Record<string, unknown>) {
  const picked: Record<string, unknown> = {};
  for (const [field, kind] of Object.entries(PLAYER_SETTINGS_FIELDS)) {
    if (typeof state[field] === kind) picked[field] = state[field];
  }
  return picked;
}

export interface ConfigExportFile {
  kind: string;
  app: string;
  version: number;
  exportedAt: string;
  stores: Record<string, PersistedEnvelope>;
}

/** 打包当前设置为导出 JSON 文本；一个 store 都没有（异常环境）返回 null */
export function buildConfigExport(now = new Date()): string | null {
  if (typeof window === "undefined") return null;
  const stores: Record<string, PersistedEnvelope> = {};
  for (const key of CONFIG_STORE_KEYS) {
    const envelope = parseEnvelope(window.localStorage.getItem(key));
    if (!envelope) continue;
    stores[key] =
      key === PLAYER_STATE_KEY
        ? { state: pickPlayerSettings(envelope.state), version: envelope.version }
        : envelope;
  }
  if (Object.keys(stores).length === 0) return null;
  const file: ConfigExportFile = {
    kind: CONFIG_EXPORT_KIND,
    app: "Seraph Audio Player",
    version: CONFIG_EXPORT_VERSION,
    exportedAt: now.toISOString(),
    stores,
  };
  return JSON.stringify(file, null, 2);
}

/**
 * 校验导入文本并抽出可用的 store 数据。
 * 抛出的 Error message 面向用户（中文），调用方直接展示。
 */
export function parseConfigImport(text: string): Record<string, PersistedEnvelope> {
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("不是有效的 JSON 文件");
  }
  const root = asRecord(parsed);
  if (!root || root.kind !== CONFIG_EXPORT_KIND) {
    throw new Error("不是 Seraph 配置文件（kind 不匹配）");
  }
  if (typeof root.version === "number" && root.version > CONFIG_EXPORT_VERSION) {
    throw new Error(`配置文件版本较新（v${root.version}），请先升级应用`);
  }
  const rawStores = asRecord(root.stores);
  if (!rawStores) throw new Error("配置文件缺少 stores 数据");
  const stores: Record<string, PersistedEnvelope> = {};
  for (const key of CONFIG_STORE_KEYS) {
    const entry = asRecord(rawStores[key]);
    const state = entry ? asRecord(entry.state) : null;
    if (!entry || !state) continue;
    stores[key] = {
      state,
      version: typeof entry.version === "number" ? entry.version : undefined,
    };
  }
  if (Object.keys(stores).length === 0) {
    throw new Error("配置文件中没有可导入的设置");
  }
  return stores;
}

/** 把校验后的导入数据暂存到 sessionStorage，等下次启动应用 */
export function stashPendingImport(stores: Record<string, PersistedEnvelope>) {
  window.sessionStorage.setItem(PENDING_IMPORT_KEY, JSON.stringify(stores));
}

/**
 * 启动最早阶段调用：若上次会话留有待应用的导入配置，写入 localStorage。
 * player store 只覆盖设置字段（本机收藏 / 歌单 / 播放痕迹保留）。
 * 返回是否应用了导入。
 */
export function applyPendingConfigImport(): boolean {
  if (typeof window === "undefined") return false;
  let raw: string | null = null;
  try {
    raw = window.sessionStorage.getItem(PENDING_IMPORT_KEY);
    if (raw !== null) window.sessionStorage.removeItem(PENDING_IMPORT_KEY);
  } catch {
    return false;
  }
  if (!raw) return false;
  const pending = parseEnvelopeMap(raw);
  if (!pending) return false;

  for (const key of CONFIG_STORE_KEYS) {
    const imported = pending[key];
    if (!imported) continue;
    if (key === PLAYER_STATE_KEY) {
      const existing = parseEnvelope(window.localStorage.getItem(key));
      const mergedState = {
        ...(existing?.state ?? {}),
        ...pickPlayerSettings(imported.state),
      };
      const version = existing?.version ?? imported.version;
      window.localStorage.setItem(
        key,
        JSON.stringify({ state: mergedState, version })
      );
    } else {
      window.localStorage.setItem(
        key,
        JSON.stringify({ state: imported.state, version: imported.version })
      );
    }
  }
  return true;
}

function parseEnvelopeMap(raw: string): Record<string, PersistedEnvelope> | null {
  try {
    const parsed = asRecord(JSON.parse(raw));
    if (!parsed) return null;
    const out: Record<string, PersistedEnvelope> = {};
    for (const key of CONFIG_STORE_KEYS) {
      const entry = asRecord(parsed[key]);
      const state = entry ? asRecord(entry.state) : null;
      if (!entry || !state) continue;
      out[key] = {
        state,
        version: typeof entry.version === "number" ? entry.version : undefined,
      };
    }
    return out;
  } catch {
    return null;
  }
}
