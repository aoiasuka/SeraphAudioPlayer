import {
  ArrowDown,
  ArrowLeft,
  ArrowUp,
  Download,
  FileUp,
  Heart,
  ListMusic,
  Music2,
  Play,
  Plus,
  Trash2,
  X,
} from "lucide-react";
import { useMemo, useState, type FormEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";
import type { LibraryView, Track } from "@/types/track";
import { isLocalTrack } from "./trackFilters";

export function PlaylistsPage() {
  const setActiveView = usePlayerStore((s) => s.setActiveView);
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const userPlaylists = usePlayerStore((s) => s.userPlaylists);
  const createUserPlaylist = usePlayerStore((s) => s.createUserPlaylist);
  const deleteUserPlaylist = usePlayerStore((s) => s.deleteUserPlaylist);
  const importPlaylistFromM3u8 = usePlayerStore((s) => s.importPlaylistFromM3u8);
  const [createDialogOpen, setCreateDialogOpen] = useState(false);
  const [newPlaylistName, setNewPlaylistName] = useState("");
  const [playlistToDeleteId, setPlaylistToDeleteId] = useState<string | null>(
    null
  );
  const [selectedPlaylistId, setSelectedPlaylistId] = useState<string | null>(
    null
  );
  const [isImportingM3u8, setIsImportingM3u8] = useState(false);
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
  const handleImportM3u8 = async () => {
    if (isImportingM3u8) return;
    setIsImportingM3u8(true);
    try {
      await importPlaylistFromM3u8();
    } finally {
      setIsImportingM3u8(false);
    }
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
  const selectedPlaylist = useMemo(
    () =>
      userPlaylists.find((playlist) => playlist.id === selectedPlaylistId) ??
      null,
    [selectedPlaylistId, userPlaylists]
  );

  if (selectedPlaylist) {
    return (
      <UserPlaylistDetail
        playlistId={selectedPlaylist.id}
        onBack={() => setSelectedPlaylistId(null)}
      />
    );
  }

  return (
    <>
      <div className="mb-4 flex items-center justify-between">
        <span className="font-tw text-[10px] tracking-[3px] text-ink3">
          DRAWER B — 歌单档案
        </span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void handleImportM3u8()}
            disabled={isImportingM3u8}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-50"
            title="从 M3U8 清单导入本地歌单"
          >
            <FileUp className="h-4 w-4" />
            <span>{isImportingM3u8 ? "导入中…" : "导入 M3U8"}</span>
          </button>
          <button
            type="button"
            onClick={openCreateDialog}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
          >
            <Plus className="h-4 w-4" />
            <span>新增歌单</span>
          </button>
        </div>
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
            role="button"
            tabIndex={0}
            onClick={() => setSelectedPlaylistId(item.id)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                setSelectedPlaylistId(item.id);
              }
            }}
            className="archive-card group relative cursor-pointer p-4 text-left"
          >
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                setPlaylistToDeleteId(item.id);
              }}
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

/** 用户歌单详情：曲目列表 + 上移/下移/移除 + 导出 M3U8。 */
function UserPlaylistDetail({
  playlistId,
  onBack,
}: {
  playlistId: string;
  onBack: () => void;
}) {
  const playlist = usePlayerStore((s) => s.playlist);
  const userPlaylists = usePlayerStore((s) => s.userPlaylists);
  const currentTrack = usePlayerStore((s) => s.currentTrack());
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const moveTrackInUserPlaylist = usePlayerStore((s) => s.moveTrackInUserPlaylist);
  const removeTrackFromUserPlaylist = usePlayerStore(
    (s) => s.removeTrackFromUserPlaylist
  );
  const exportUserPlaylistToM3u8 = usePlayerStore(
    (s) => s.exportUserPlaylistToM3u8
  );

  const userPlaylist = userPlaylists.find((item) => item.id === playlistId);
  const trackById = useMemo(
    () => new Map(playlist.map((track) => [track.id, track])),
    [playlist]
  );
  const indexById = useMemo(() => {
    const map = new Map<string, number>();
    playlist.forEach((track, index) => map.set(track.id, index));
    return map;
  }, [playlist]);

  if (!userPlaylist) {
    onBack();
    return null;
  }

  const tracks = userPlaylist.trackIds
    .map((id) => trackById.get(id))
    .filter((track): track is Track => !!track);

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-3">
      <div className="flex items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
        <button
          type="button"
          onClick={onBack}
          className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
        >
          <ArrowLeft className="h-4 w-4" />
          返回歌单
        </button>
        <div className="flex min-w-0 items-center gap-3">
          <div className="min-w-0 text-right">
            <p className="truncate font-serif text-sm font-bold text-ink">
              {userPlaylist.name}
            </p>
            <p className="font-tw text-[11px] text-ink2">
              {tracks.length} 首曲目
            </p>
          </div>
          <button
            type="button"
            onClick={() => void exportUserPlaylistToM3u8(playlistId)}
            disabled={tracks.length === 0}
            className="stamp-btn inline-flex h-9 items-center gap-1.5 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-50"
            title="导出为 M3U8 清单"
          >
            <Download className="h-4 w-4" />
            导出
          </button>
        </div>
      </div>

      {tracks.length === 0 ? (
        <div className="flex min-h-[200px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
          歌单还没有曲目——在曲目列表悬停行上点「加入歌单」按钮添加
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto pr-1">
          {tracks.map((track, index) => {
            const globalIndex = indexById.get(track.id) ?? -1;
            const active = currentTrack?.id === track.id;
            return (
              <div
                key={track.id}
                className="archive-card group mb-2 grid h-[46px] grid-cols-[44px_minmax(0,1fr)_64px_112px] items-center gap-3 px-3"
              >
                <span className="font-tw text-[11px] font-bold text-ink3">
                  {(index + 1).toString().padStart(3, "0")}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    if (globalIndex >= 0) loadTrack(globalIndex);
                  }}
                  disabled={globalIndex < 0}
                  className="min-w-0 text-left disabled:cursor-not-allowed disabled:opacity-50"
                  aria-label={`播放 ${track.title}`}
                >
                  <span className={`block truncate font-serif text-[13px] font-semibold leading-tight ${active ? "text-stamp" : "text-ink"}`}>
                    {track.title}
                  </span>
                  <span className="block truncate font-tw text-[10px] text-ink2">
                    {track.artist}
                  </span>
                </button>
                <span className="text-right font-tw text-[12px] font-bold text-ink2">
                  {formatSeconds(track.duration)}
                </span>
                <span className="flex items-center justify-end gap-1 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    type="button"
                    onClick={() => {
                      if (globalIndex >= 0) loadTrack(globalIndex);
                    }}
                    className="flex h-7 w-7 items-center justify-center text-ink3 hover:text-ink"
                    aria-label="播放"
                    title="播放"
                  >
                    <Play className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => moveTrackInUserPlaylist(playlistId, track.id, "up")}
                    disabled={index === 0}
                    className="flex h-7 w-7 items-center justify-center text-ink3 hover:text-ink disabled:opacity-30"
                    aria-label="上移"
                    title="上移"
                  >
                    <ArrowUp className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => moveTrackInUserPlaylist(playlistId, track.id, "down")}
                    disabled={index === tracks.length - 1}
                    className="flex h-7 w-7 items-center justify-center text-ink3 hover:text-ink disabled:opacity-30"
                    aria-label="下移"
                    title="下移"
                  >
                    <ArrowDown className="h-3.5 w-3.5" />
                  </button>
                  <button
                    type="button"
                    onClick={() => removeTrackFromUserPlaylist(playlistId, track.id)}
                    className="flex h-7 w-7 items-center justify-center text-ink3 hover:text-stamp"
                    aria-label="移出歌单"
                    title="移出歌单"
                  >
                    <X className="h-3.5 w-3.5" />
                  </button>
                </span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
