import { Loader2, Trash2 } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { isStreamingTrack } from "@/components/pages/main-pages/trackFilters";
import { useContextMenuStore, type DeleteTracksRequest } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import type { DeleteTracksResult } from "@/types/track";

/** 单首、多选和全部删除共用确认框，失败后只重试未删除的曲目。 */
export function DeleteTrackConfirmDialog() {
  const request = useContextMenuStore((s) => s.deleteRequest);
  return request ? <DeleteConfirmation key={request.id} request={request} /> : null;
}

function DeleteConfirmation({ request }: { request: DeleteTracksRequest }) {
  const closeDeleteTrack = useContextMenuStore((s) => s.closeDeleteTrack);
  const playlist = usePlayerStore((s) => s.playlist);
  const deleteTracks = usePlayerStore((s) => s.deleteTracks);
  const [pendingIds, setPendingIds] = useState(request.trackIds);
  const [result, setResult] = useState<DeleteTracksResult | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const busy = useRef(false);
  const tracks = useMemo(() => {
    const ids = new Set(pendingIds);
    return playlist.filter((track) => ids.has(track.id));
  }, [playlist, pendingIds]);
  const streamingCount = tracks.filter(isStreamingTrack).length;
  const localCount = tracks.length - streamingCount;

  const handleClose = () => {
    if (!busy.current) closeDeleteTrack();
  };

  const handleDelete = async () => {
    if (pendingIds.length === 0 || busy.current) return;
    busy.current = true;
    setIsDeleting(true);
    try {
      const outcome = await deleteTracks(pendingIds);
      if (useContextMenuStore.getState().deleteRequest !== request) return;
      if (outcome.failures.length === 0) {
        closeDeleteTrack();
      } else {
        setResult(outcome);
        setPendingIds(outcome.failures.map((failure) => failure.id));
      }
    } finally {
      busy.current = false;
      setIsDeleting(false);
    }
  };

  return (
    <Dialog open onClose={handleClose} className="max-w-md">
      <div className="space-y-4">
        <div>
          <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
            {request.all ? "Delete All Records" : "Delete Records"}
          </p>
          <h2 className="mt-1 font-serif text-lg font-bold text-ink">
            {result ? "删除未完成" : request.all ? "全部删除" : tracks.length > 1 ? "删除所选曲目" : "删除曲目"}
          </h2>
          <p className="mt-2 break-words font-tw text-xs leading-relaxed text-ink2">
            范围：{request.scope} · {tracks.length} 首
          </p>
          {tracks.length === 1 && (
            <p className="mt-1 break-words font-serif text-sm text-ink">「{tracks[0].title}」</p>
          )}
        </div>
        <div className="space-y-2 border-[1.5px] border-line bg-card p-3 font-tw text-xs leading-relaxed text-ink2">
          {streamingCount > 0 && (
            <p className="text-stamp">流媒体 {streamingCount} 首：删除曲库记录，并永久删除本机缓存音频文件。</p>
          )}
          {localCount > 0 && <p>本地音乐 {localCount} 首：移除曲库记录，保留原始音频文件。</p>}
          <p>对应的收藏、最近播放和歌单引用会一并移除。</p>
          {streamingCount > 0 && <p>文件删除失败的曲目会保留记录，便于重试。</p>}
        </div>
        {result && (
          <div role="alert" className="max-h-48 space-y-2 overflow-y-auto border-l-2 border-stamp pl-3 font-tw text-xs leading-relaxed text-ink2">
            <p className="font-bold text-stamp">本次已删除 {result.deletedIds.length} 首，{result.failures.length} 首未删除。</p>
            {result.failures.slice(0, 10).map((failure) => (
              <p key={failure.id} className="break-words"><b>{failure.title}</b>：{failure.message}</p>
            ))}
            {result.failures.length > 10 && <p>另有 {result.failures.length - 10} 首未删除，可一并重试。</p>}
          </div>
        )}
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={handleClose}
            disabled={isDeleting}
            className="stamp-btn h-9 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-60"
          >
            {result ? "关闭" : "取消"}
          </button>
          <button
            type="button"
            onClick={() => void handleDelete()}
            disabled={isDeleting || pendingIds.length === 0}
            className="inline-flex h-9 items-center gap-2 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper transition-colors hover:brightness-110 disabled:cursor-wait disabled:opacity-70"
          >
            {isDeleting ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Trash2 className="h-3.5 w-3.5" />}
            <span>{isDeleting ? "删除中…" : result ? `重试 ${pendingIds.length} 首` : `确认删除 ${tracks.length} 首`}</span>
          </button>
        </div>
      </div>
    </Dialog>
  );
}
