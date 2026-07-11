export function formatSeconds(sec: number): string {
  // 审2-R12：NaN/Infinity 直接回退，避免 UI 显示 "NaN:NaN"
  if (!Number.isFinite(sec)) return "00:00";
  const total = Math.max(0, Math.floor(sec));
  // 审2-R12：超过 1 小时的曲目显示 H:MM:SS，不再显示 "75:30" 这类分钟溢出
  if (total >= 3600) {
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60)
      .toString()
      .padStart(2, "0");
    const s = (total % 60).toString().padStart(2, "0");
    return `${h}:${m}:${s}`;
  }
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
