export function formatSeconds(sec: number): string {
  const total = Math.max(0, Math.floor(sec));
  const m = Math.floor(total / 60)
    .toString()
    .padStart(2, "0");
  const s = (total % 60).toString().padStart(2, "0");
  return `${m}:${s}`;
}

export function formatFileSize(bytes?: number | null): string {
  if (!bytes) return "—";
  const mb = bytes / (1024 * 1024);
  return `${mb.toFixed(1)} MB`;
}
