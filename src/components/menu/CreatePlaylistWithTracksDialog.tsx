import { useEffect, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";

/**
 * 右键菜单「加入歌单 → 新建歌单…」弹窗：
 * 创建歌单并立即把触发菜单时锁定的曲目（单曲或整组）加入其中。
 */
export function CreatePlaylistWithTracksDialog() {
  const trackIds = useContextMenuStore((s) => s.createPlaylistTrackIds);
  const close = useContextMenuStore((s) => s.closeCreatePlaylistWith);
  const [name, setName] = useState("");
  const open = trackIds !== null && trackIds.length > 0;

  // 打开时生成不重名的默认歌单名（与歌单页「新增歌单」同规则）
  useEffect(() => {
    if (!open) return;
    const names = new Set(
      usePlayerStore.getState().userPlaylists.map((item) => item.name)
    );
    let index = names.size + 1;
    let candidate = `新歌单 ${index}`;
    while (names.has(candidate)) {
      index += 1;
      candidate = `新歌单 ${index}`;
    }
    setName(candidate);
  }, [open]);

  const handleSubmit = (event: FormEvent) => {
    event.preventDefault();
    if (!trackIds || trackIds.length === 0) return;
    const trimmedName = name.trim();
    if (!trimmedName) return;

    const live = usePlayerStore.getState();
    const playlistId = live.createUserPlaylist(trimmedName);
    if (!playlistId) return;
    trackIds.forEach((trackId) =>
      live.addTrackToUserPlaylist(playlistId, trackId)
    );
    live.showNotification(
      `已创建歌单「${trimmedName}」并加入 ${trackIds.length} 首`
    );
    close();
  };

  return (
    <Dialog open={open} onClose={close} className="max-w-sm">
      <form onSubmit={handleSubmit} className="space-y-4">
        <div>
          <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
            New Playlist
          </p>
          <h2 className="mt-1 font-serif text-lg font-bold text-ink">
            新建歌单并加入
          </h2>
          <p className="mt-2 font-tw text-xs leading-relaxed text-ink2">
            创建后将把选中的 {trackIds?.length ?? 0} 首曲目加入新歌单。
          </p>
        </div>
        <label className="block space-y-1.5">
          <span className="font-tw text-[11px] font-bold text-ink2">
            歌单名称
          </span>
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            autoFocus
            className="h-10 w-full border-[1.5px] border-ink bg-card px-3 font-tw text-sm font-semibold text-ink outline-none transition-colors placeholder:text-ink3 focus:border-stamp"
            placeholder="输入歌单名称"
          />
        </label>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            className="stamp-btn h-9 px-3 font-tw text-xs font-bold"
          >
            取消
          </button>
          <button
            type="submit"
            disabled={!name.trim()}
            className="h-9 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:cursor-not-allowed disabled:bg-line disabled:border-line disabled:text-ink2"
          >
            创建并加入
          </button>
        </div>
      </form>
    </Dialog>
  );
}
