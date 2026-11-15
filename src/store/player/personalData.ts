/**
 * 个人数据（收藏 + 自建歌单）的消毒、导出与导入合并（FE2-01）。
 *
 * 这两项只存在 WebView 的 localStorage 里，配置导出又明确排除它们，
 * 清理 WebView2 数据、重装或换机就全部丢失。这里提供独立的备份文件格式：
 * 导入是「合并」而不是「覆盖」——本机已有的收藏和歌单一条都不会少。
 */
import type { UserPlaylist } from "@/types/track";

export const PERSONAL_DATA_KIND = "seraph-personal-data";
export const PERSONAL_DATA_VERSION = 1;
/** 与后端 `MAX_CONFIG_FILE_BYTES` 同口径（复用 import_app_config 读文件）。 */
const MAX_PERSONAL_DATA_CHARS = 2 * 1024 * 1024;

export interface PersonalData {
  liked: Record<string, boolean>;
  likedAt: Record<string, number>;
  userPlaylists: UserPlaylist[];
}

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}

function stringArray(value: unknown) {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

export function sanitizeLiked(value: unknown): Record<string, boolean> {
  return Object.fromEntries(
    Object.entries(asRecord(value)).filter(
      (entry): entry is [string, boolean] => typeof entry[1] === "boolean"
    )
  );
}

export function sanitizeLikedAt(value: unknown): Record<string, number> {
  return Object.fromEntries(
    Object.entries(asRecord(value)).filter(
      (entry): entry is [string, number] =>
        typeof entry[1] === "number" && Number.isFinite(entry[1])
    )
  );
}

/**
 * W-03:歌单元素级消毒。localStorage 被改坏(或导入了畸形文件)时,元素可能是
 * null / 缺 trackIds,渲染期 `item.trackIds.includes(...)` 直接 TypeError 白屏。
 * 逐字段校验并丢弃不合规元素——歌单少一条,好过整个页面崩掉。
 */
export function sanitizeUserPlaylists(value: unknown): UserPlaylist[] {
  if (!Array.isArray(value)) return [];
  return value.flatMap((item): UserPlaylist[] => {
    if (!item || typeof item !== "object") return [];
    const raw = item as Record<string, unknown>;
    if (typeof raw.id !== "string" || !raw.id) return [];
    const createdAt =
      typeof raw.createdAt === "number" && Number.isFinite(raw.createdAt)
        ? raw.createdAt
        : 0;
    return [
      {
        id: raw.id,
        name: typeof raw.name === "string" ? raw.name : "未命名歌单",
        trackIds: stringArray(raw.trackIds),
        createdAt,
      },
    ];
  });
}

/** 收藏为空且没有歌单时返回 null（没有可导出的内容）。 */
export function buildPersonalDataExport(
  data: PersonalData,
  now = new Date()
): string | null {
  const liked = Object.fromEntries(
    Object.entries(data.liked).filter(([, value]) => value)
  );
  if (Object.keys(liked).length === 0 && data.userPlaylists.length === 0) {
    return null;
  }
  return JSON.stringify(
    {
      kind: PERSONAL_DATA_KIND,
      app: "Seraph Audio Player",
      version: PERSONAL_DATA_VERSION,
      exportedAt: now.toISOString(),
      liked,
      likedAt: Object.fromEntries(
        Object.entries(data.likedAt).filter(([id]) => liked[id])
      ),
      userPlaylists: data.userPlaylists,
    },
    null,
    2
  );
}

/** 抛出的 Error message 面向用户（中文），调用方直接展示。 */
export function parsePersonalDataImport(text: string): PersonalData {
  if (text.length > MAX_PERSONAL_DATA_CHARS) {
    throw new Error("个人数据文件过大（超过 2 MB）");
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error("不是有效的 JSON 文件");
  }
  const root = asRecord(parsed);
  if (root.kind !== PERSONAL_DATA_KIND) {
    throw new Error("不是 Seraph 个人数据文件（kind 不匹配）");
  }
  if (typeof root.version === "number" && root.version > PERSONAL_DATA_VERSION) {
    throw new Error(`个人数据文件版本较新（v${root.version}），请先升级应用`);
  }
  const liked = Object.fromEntries(
    Object.entries(sanitizeLiked(root.liked)).filter(([, value]) => value)
  );
  const userPlaylists = sanitizeUserPlaylists(root.userPlaylists);
  if (Object.keys(liked).length === 0 && userPlaylists.length === 0) {
    throw new Error("文件中没有可导入的收藏或歌单");
  }
  return { liked, likedAt: sanitizeLikedAt(root.likedAt), userPlaylists };
}

export interface PersonalDataMergeResult extends PersonalData {
  addedLikes: number;
  addedPlaylists: number;
  mergedPlaylists: number;
}

/**
 * 合并而不覆盖：收藏取并集（本机已有的收藏时间优先）；歌单按 id 对齐，
 * 同 id 的把导入方多出来的曲目接到末尾，新 id 的整张追加。
 */
export function mergePersonalData(
  current: PersonalData,
  imported: PersonalData
): PersonalDataMergeResult {
  const liked = { ...current.liked };
  const likedAt = { ...current.likedAt };
  let addedLikes = 0;
  for (const [id, value] of Object.entries(imported.liked)) {
    if (!value || liked[id]) continue;
    liked[id] = true;
    addedLikes += 1;
    const at = imported.likedAt[id];
    if (at !== undefined && likedAt[id] === undefined) likedAt[id] = at;
  }

  let addedPlaylists = 0;
  let mergedPlaylists = 0;
  const userPlaylists = current.userPlaylists.map((playlist) => ({ ...playlist }));
  for (const incoming of imported.userPlaylists) {
    const existing = userPlaylists.find((playlist) => playlist.id === incoming.id);
    if (!existing) {
      userPlaylists.push({ ...incoming, trackIds: [...new Set(incoming.trackIds)] });
      addedPlaylists += 1;
      continue;
    }
    const known = new Set(existing.trackIds);
    const extra = incoming.trackIds.filter((id) => !known.has(id));
    if (extra.length > 0) {
      existing.trackIds = [...existing.trackIds, ...new Set(extra)];
      mergedPlaylists += 1;
    }
  }
  return { liked, likedAt, userPlaylists, addedLikes, addedPlaylists, mergedPlaylists };
}
