import type { Track } from "@/types/track";

export function streamingSourceInput(track: Track) {
  const bvid = bvidFromTrack(track);
  return (
    track.sourceUrl?.trim() ||
    track.sourceId?.trim() ||
    (bvid ? `https://www.bilibili.com/video/${bvid}` : "")
  );
}

export function normalizePath(path: string) {
  // 分隔符归一：跨平台工具导出的 M3U8 常写正斜杠，库内 Windows 路径是反
  // 斜杠——不归一会让路径回映射 miss，旧库文件建歌单时静默丢曲（M-8）。
  return path.trim().replace(/\\/g, "/").toLowerCase();
}

function normalizeText(value: string | undefined | null) {
  return (value ?? "").trim().replace(/\s+/g, " ").toLowerCase();
}

function bvidFromTrack(track: Track) {
  const sourceId = track.sourceId?.trim();
  if (sourceId?.toLowerCase().startsWith("bv")) return sourceId.toLowerCase();

  const sourceUrl = track.sourceUrl ?? "";
  const sourceMatch = sourceUrl.match(/BV[a-zA-Z0-9]+/);
  if (sourceMatch) return sourceMatch[0].toLowerCase();

  const idMatch = track.id.match(/bilibili-(bv[a-zA-Z0-9]+)/i);
  if (idMatch) return idMatch[1].toLowerCase();

  const pathMatch = track.path.match(/(BV[a-zA-Z0-9]+)-\d+/i);
  if (pathMatch) return pathMatch[1].toLowerCase();

  return "";
}

function isBilibiliTrack(track: Track) {
  return track.id.startsWith("bilibili-") || track.album === "Bilibili";
}

export function trackMergeKey(track: Track) {
  const bvid = bvidFromTrack(track);
  if (bvid) return `bvid:${bvid}`;
  const sourceId = track.sourceId?.trim().toLowerCase();
  if (sourceId) return `source-id:${sourceId}`;
  const sourceUrl = track.sourceUrl?.trim().toLowerCase();
  if (sourceUrl) return `source-url:${sourceUrl}`;
  if (isBilibiliTrack(track)) {
    return [
      "bilibili-meta",
      normalizeText(track.title),
      normalizeText(track.artist),
      Math.round(track.duration || 0),
    ].join(":");
  }
  return `path:${normalizePath(track.path)}`;
}

