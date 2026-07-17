import { Copy, FolderSearch } from "lucide-react";
import { useMemo } from "react";
import { Dialog } from "@/components/ui/dialog";
import { copyText } from "@/lib/clipboard";
import { formatSeconds } from "@/lib/format";
import { revealTrackFile } from "@/lib/system";
import { useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";

/** 右键菜单「曲目信息…」弹窗：完整展示 Track 上的规格与来源字段。 */
export function TrackInfoDialog() {
  const infoTrackId = useContextMenuStore((s) => s.infoTrackId);
  const closeTrackInfo = useContextMenuStore((s) => s.closeTrackInfo);
  const playlist = usePlayerStore((s) => s.playlist);
  const showNotification = usePlayerStore((s) => s.showNotification);
  const track = useMemo(
    () => playlist.find((item) => item.id === infoTrackId) ?? null,
    [playlist, infoTrackId]
  );

  const rows: Array<[string, string]> = track
    ? [
        ["标题", track.title],
        ["艺术家", track.artist],
        [
          "专辑",
          track.albumYear ? `${track.album}（${track.albumYear}）` : track.album,
        ],
        ["格式", track.format],
        ["规格", track.bitdepth],
        ["采样率", track.sampleRate ?? "未知"],
        ["码率", track.bitrate],
        ["声道", track.channels],
        ["文件大小", track.size],
        ["时长", formatSeconds(track.duration)],
        ["歌词", track.lyrics.length > 0 ? `${track.lyrics.length} 行` : "无"],
        [
          "来源",
          track.sourceUrl || track.sourceId ? "Bilibili 缓存" : "本地文件",
        ],
        ["文件路径", track.path || "—"],
      ]
    : [];

  const handleCopyPath = async () => {
    if (!track?.path) return;
    const copied = await copyText(track.path);
    showNotification(copied ? "已复制文件路径" : "复制失败");
  };

  return (
    <Dialog open={Boolean(track)} onClose={closeTrackInfo} className="max-w-md">
      {track ? (
        <div className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              Track Info
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">
              曲目信息
            </h2>
          </div>
          <dl className="max-h-[52vh] overflow-y-auto border-[1.5px] border-line bg-card">
            {rows.map(([label, value], index) => (
              <div
                key={label}
                className={
                  index > 0
                    ? "grid grid-cols-[76px_minmax(0,1fr)] gap-3 border-t border-dashed border-line px-3 py-1.5"
                    : "grid grid-cols-[76px_minmax(0,1fr)] gap-3 px-3 py-1.5"
                }
              >
                <dt className="pt-0.5 font-tw text-[10px] font-bold tracking-wider text-ink3">
                  {label}
                </dt>
                <dd className="min-w-0 break-all font-tw text-xs font-semibold leading-relaxed text-ink">
                  {value}
                </dd>
              </div>
            ))}
          </dl>
          <div className="flex flex-wrap justify-end gap-2">
            {track.path ? (
              <>
                <button
                  type="button"
                  onClick={() => void handleCopyPath()}
                  className="stamp-btn inline-flex h-9 items-center gap-1.5 px-3 font-tw text-xs font-bold"
                >
                  <Copy className="h-3.5 w-3.5" />
                  复制路径
                </button>
                <button
                  type="button"
                  onClick={() => void revealTrackFile(track.path)}
                  disabled={!!track.cacheMissing}
                  className="stamp-btn inline-flex h-9 items-center gap-1.5 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <FolderSearch className="h-3.5 w-3.5" />
                  打开所在位置
                </button>
              </>
            ) : null}
            <button
              type="button"
              onClick={closeTrackInfo}
              className="h-9 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp"
            >
              关闭
            </button>
          </div>
        </div>
      ) : null}
    </Dialog>
  );
}
