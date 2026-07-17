import { Loader2, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";

/**
 * 全局「删除曲库记录」确认弹窗。
 * v0.4.3：从 TrackRows 的局部弹窗提升为全局单例，右键菜单与行内悬停删除按钮
 * 共用同一入口（contextMenu store 的 requestDeleteTrack）。
 */
export function DeleteTrackConfirmDialog() {
  const confirmDeleteTrackId = useContextMenuStore(
    (s) => s.confirmDeleteTrackId
  );
  const closeDeleteTrack = useContextMenuStore((s) => s.closeDeleteTrack);
  const playlist = usePlayerStore((s) => s.playlist);
  const deleteTrack = usePlayerStore((s) => s.deleteTrack);
  const [isDeleting, setIsDeleting] = useState(false);
  const track = useMemo(
    () => playlist.find((item) => item.id === confirmDeleteTrackId) ?? null,
    [playlist, confirmDeleteTrackId]
  );

  const handleClose = () => {
    if (!isDeleting) closeDeleteTrack();
  };

  const handleDelete = async () => {
    if (!track || isDeleting) return;
    setIsDeleting(true);
    try {
      await deleteTrack(track.id);
      closeDeleteTrack();
    } finally {
      setIsDeleting(false);
    }
  };

  return (
    <Dialog open={Boolean(track)} onClose={handleClose} className="max-w-sm">
      <div className="space-y-4">
        <div>
          <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
            Delete Track
          </p>
          <h2 className="mt-1 font-serif text-lg font-bold text-ink">
            删除曲库记录
          </h2>
          <p className="mt-2 font-tw text-xs leading-relaxed text-ink2">
            确定从曲库中移除「{track?.title}」吗？这只会删除软件内记录，不会删除磁盘上的音频文件。
          </p>
        </div>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={handleClose}
            disabled={isDeleting}
            className="stamp-btn h-9 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-60"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void handleDelete()}
            disabled={isDeleting}
            className="inline-flex h-9 items-center gap-2 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper transition-colors hover:brightness-110 disabled:cursor-wait disabled:opacity-70"
          >
            {isDeleting ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
            <span>{isDeleting ? "删除中" : "删除记录"}</span>
          </button>
        </div>
      </div>
    </Dialog>
  );
}
