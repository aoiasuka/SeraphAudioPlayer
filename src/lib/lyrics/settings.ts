import type { LyricsSourcePriority } from "@/types/track";

/** 歌词设置的默认值与选项表（前后端共用口径）。 */

/**
 * AMLL TTML DB 地址是**模板**：`{dir}` = ncm-lyrics / qq-lyrics，`{id}` 或 `%s` = 平台歌曲 ID。
 * 不含占位符的地址按 base URL 处理，后端自动补 `/{dir}/{id}.ttml`。
 */
export const DEFAULT_AMLL_TTML_DB_URL =
  "https://raw.githubusercontent.com/amll-dev/amll-ttml-db/main";

/** 预设镜像：与后端 `AMLL_TTML_HOST_SUFFIXES` 白名单一致；预设模式下只能选这些。 */
export const AMLL_TTML_DB_MIRRORS: { label: string; url: string }[] = [
  { label: "GitHub Raw（默认）", url: DEFAULT_AMLL_TTML_DB_URL },
  { label: "jsDelivr", url: "https://cdn.jsdelivr.net/gh/amll-dev/amll-ttml-db@main" },
  { label: "jsDelivr（Fastly）", url: "https://fastly.jsdelivr.net/gh/amll-dev/amll-ttml-db@main" },
  { label: "jsDelivr（Gcore）", url: "https://gcore.jsdelivr.net/gh/amll-dev/amll-ttml-db@main" },
];

/** 自定义模式下的常用地址（社区镜像等），只是填入快捷方式，仍走自定义模式校验。 */
export const AMLL_TTML_DB_CUSTOM_PRESETS: { label: string; url: string }[] = [
  { label: "amlldb.bikonoo.com", url: "https://amlldb.bikonoo.com/{dir}/{id}.ttml" },
];

/** 相关站点（设置页里的说明链接）。 */
export const AMLL_LINKS = {
  repo: "https://github.com/amll-dev/amll-ttml-db",
  api: "https://github.com/amll-dev/amll-ttml-api",
  apiDocs: "https://amll.dev/reference/http-api/overview",
  search: "https://amlldb.bikonoo.com/",
  tool: "https://tool.amll.dev/",
};

const AMLL_ALLOWED_HOST_SUFFIXES = ["raw.githubusercontent.com", ".jsdelivr.net"];
const PRIVATE_HOST_SUFFIXES = [".local", ".localhost", ".internal", ".lan", ".home.arpa"];

function parseHttpsUrl(raw: string): URL | null {
  let url: URL;
  try {
    url = new URL(raw.trim().replace(/\{dir\}/g, "ncm-lyrics").replace(/\{id\}|%s/g, "1"));
  } catch {
    return null;
  }
  if (url.protocol !== "https:" || url.username || url.password) return null;
  return url;
}

/** 预设镜像模式：host 必须命中白名单（带点只认子域，不带点裸域与子域都认）。 */
export function isAllowedAmllTtmlDbUrl(raw: string): boolean {
  const url = parseHttpsUrl(raw);
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  return AMLL_ALLOWED_HOST_SUFFIXES.some((suffix) =>
    suffix.startsWith(".")
      ? host.length > suffix.length && host.endsWith(suffix)
      : host === suffix || host.endsWith(`.${suffix}`)
  );
}

/** 自定义模式：任意公网 https 域名；拒绝 IP 直连、localhost、内网后缀与无点主机名。 */
export function isPublicHttpsUrl(raw: string): boolean {
  const url = parseHttpsUrl(raw);
  if (!url) return false;
  const host = url.hostname.toLowerCase();
  if (host.startsWith("[") || /^\d{1,3}(\.\d{1,3}){3}$/.test(host)) return false;
  if (!host.includes(".") || host === "localhost") return false;
  return !PRIVATE_HOST_SUFFIXES.some((suffix) => host.endsWith(suffix));
}

/** 按模式校验地址。 */
export function isValidAmllTtmlDbUrl(raw: string, custom: boolean): boolean {
  return custom ? isPublicHttpsUrl(raw) : isAllowedAmllTtmlDbUrl(raw);
}

export function normalizeAmllTtmlDbUrl(raw: string): string {
  const trimmed = raw.trim().replace(/\/+$/, "");
  return trimmed || DEFAULT_AMLL_TTML_DB_URL;
}

/** 预览：模板展开后的实际请求地址（设置弹窗里给用户看）。 */
export function previewAmllTtmlUrl(template: string, dir = "ncm-lyrics", id = "2116462216"): string {
  const base = normalizeAmllTtmlDbUrl(template);
  const withTemplate = /\{id\}|%s/.test(base) ? base : `${base}/{dir}/{id}.ttml`;
  return withTemplate.replace(/\{dir\}/g, dir).replace(/\{id\}|%s/g, id);
}

export const LYRICS_SOURCE_OPTIONS: { value: LyricsSourcePriority; label: string }[] = [
  { value: "auto", label: "自动" },
  { value: "netease", label: "网易云音乐优先" },
  { value: "kugou", label: "酷狗音乐优先" },
  { value: "qq", label: "QQ 音乐优先" },
];

export function sanitizeLyricsSourcePriority(value: unknown): LyricsSourcePriority {
  return value === "netease" || value === "kugou" || value === "qq" ? value : "auto";
}
