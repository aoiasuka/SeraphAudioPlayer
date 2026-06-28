import { Heart, ListMusic, Music2, Plus, Trash2 } from "lucide-react";
import { useMemo, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { usePlayerStore } from "@/store/player";
import type { LibraryView } from "@/types/track";
import { isLocalTrack } from "./trackFilters";

export function PlaylistsPage() {
  const setActiveView = usePlayerStore((s) => s.setActiveView);
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const userPlaylists = usePlayerStore((s) => s.userPlaylists);
  const createUserPlaylist = usePlayerStore((s) => s.createUserPlaylist);
  const deleteUserPlaylist = usePlayerStore((s) => s.deleteUserPlaylist);
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [newPlaylistName, setNewPlaylistName] = useState("");
  const [playlistToDeleteId, setPlaylistToDeleteId] = useState<string | null>(
    null
  );
  const localCount = useMemo(
    () => playlist.filter(isLocalTrack).length,
    [playlist]
  );
  const likedCount = useMemo(
    () => playlist.filter((track) => liked[track.id]).length,
    [liked, playlist]
  );
  const cards = useMemo(() => [
    {
      title: "Hi-Res 本地精选",
      count: localCount,
      view: "local" as LibraryView,
      icon: Music2,
      color: "text-cyan-700 bg-cyan-600/10",
    },
    {
      title: "我喜欢",
      count: likedCount,
      view: "liked" as LibraryView,
      icon: Heart,
      color: "text-rose-600 bg-rose-500/10",
    },
    {
      title: "最近播放",
      count: recentTrackIds.length,
      view: "recent" as LibraryView,
      icon: ListMusic,
      color: "text-indigo-600 bg-indigo-500/10",
    },
  ], [likedCount, localCount, recentTrackIds.length]);
  const openCreateDialog = () => {
    const names = new Set(userPlaylists.map((item) => item.name));
    let index = userPlaylists.length + 1;
    let name = `新歌单 ${index}`;
    while (names.has(name)) {
      index += 1;
      name = `新歌单 ${index}`;
    }
    setNewPlaylistName(name);
    setCreateDialogOpen(true);
  };
  const handleCreatePlaylist = (event: FormEvent) => {
    event.preventDefault();
    const name = newPlaylistName.trim();
    if (!name) return;
    createUserPlaylist(name);
    setCreateDialogOpen(false);
    setNewPlaylistName("");
  };
  const playlistToDelete = useMemo(
    () =>
      userPlaylists.find((playlist) => playlist.id === playlistToDeleteId) ??
      null,
    [playlistToDeleteId, userPlaylists]
  );
  const handleDeletePlaylist = () => {
    if (!playlistToDelete) return;
    deleteUserPlaylist(playlistToDelete.id);
    setPlaylistToDeleteId(null);
  };

  return (
    <>
      <div className="mb-4 flex items-center justify-between">
        <span className="font-tw text-[10px] tracking-[3px] text-ink3">
          DRAWER B — 歌单档案
        </span>
        <button
          type="button"
          onClick={openCreateDialog}
          className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
        >
          <Plus className="h-4 w-4" />
          <span>新增歌单</span>
        </button>
      </div>
      <div className="grid grid-cols-3 gap-4 overflow-y-auto pr-1">
        {cards.map((card) => {
          const Icon = card.icon;
          return (
            <button
              key={card.title}
              type="button"
              onClick={() => setActiveView(card.view)}
              className="archive-card p-4 text-left"
            >
              <span className="mb-6 flex h-10 w-10 items-center justify-center border-[1.5px] border-ink bg-paper2 text-ink">
                <Icon className="h-4 w-4" />
              </span>
              <span className="block font-serif text-sm font-bold text-ink">{card.title}</span>
              <span className="mt-1 block font-tw text-[11px] text-ink2">{card.count} 首曲目</span>
            </button>
          );
        })}
        {userPlaylists.map((item) => (
          <div
            key={item.id}
            className="archive-card group relative p-4 text-left"
          >
            <button
              type="button"
              onClick={() => setPlaylistToDeleteId(item.id)}
              className="absolute right-3 top-3 flex h-8 w-8 items-center justify-center text-ink3 opacity-0 transition-all hover:text-stamp group-hover:opacity-100 focus:opacity-100"
              aria-label={`删除歌单 ${item.name}`}
              title="删除歌单"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
            <span className="mb-6 flex h-10 w-10 items-center justify-center border-[1.5px] border-ink bg-paper2 text-brown">
              <ListMusic className="h-4 w-4" />
            </span>
            <span className="block truncate font-serif text-sm font-bold text-ink">
              {item.name}
            </span>
            <span className="mt-1 block font-tw text-[11px] text-ink2">
              {item.trackIds.length} 首曲目
            </span>
          </div>
        ))}
      </div>
      <Dialog
        open={createDialogOpen}
        onClose={() => setCreateDialogOpen(false)}
        className="max-w-sm"
      >
        <form onSubmit={handleCreatePlaylist} className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              New Playlist
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">
              新增歌单
            </h2>
          </div>
          <label className="block space-y-1.5">
            <span className="font-tw text-[11px] font-bold text-ink2">
              歌单名称
            </span>
            <input
              value={newPlaylistName}
              onChange={(event) => setNewPlaylistName(event.target.value)}
              autoFocus
              className="h-10 w-full border-[1.5px] border-ink bg-card px-3 font-tw text-sm font-semibold text-ink outline-none transition-colors placeholder:text-ink3 focus:border-stamp"
              placeholder="输入歌单名称"
            />
          </label>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setCreateDialogOpen(false)}
              className="stamp-btn h-9 px-3 font-tw text-xs font-bold"
            >
              取消
            </button>
            <button
              type="submit"
              disabled={!newPlaylistName.trim()}
              className="h-9 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:cursor-not-allowed disabled:bg-line disabled:border-line disabled:text-ink2"
            >
              创建
            </button>
          </div>
        </form>
      </Dialog>
      <Dialog
        open={Boolean(playlistToDelete)}
        onClose={() => setPlaylistToDeleteId(null)}
        className="max-w-sm"
      >
        <div className="space-y-4">
          <div>
            <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
              Delete Playlist
            </p>
            <h2 className="mt-1 font-serif text-lg font-bold text-ink">
              删除歌单
            </h2>
            <p className="mt-2 font-tw text-xs leading-relaxed text-ink2">
              确定删除“{playlistToDelete?.name}”吗？歌单内的曲目不会从曲库中删除。
            </p>
          </div>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setPlaylistToDeleteId(null)}
              className="stamp-btn h-9 px-3 font-tw text-xs font-bold"
            >
              取消
            </button>
            <button
              type="button"
              onClick={handleDeletePlaylist}
              className="h-9 border-[1.5px] border-stamp bg-stamp px-3 font-tw text-xs font-bold text-paper transition-colors hover:brightness-110"
            >
              删除
            </button>
          </div>
        </div>
      </Dialog>
    </>
  );
}

