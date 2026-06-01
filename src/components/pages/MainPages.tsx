import {
  Disc3,
  Heart,
  ListMusic,
  Music2,
  Pause,
  Play,
  User,
} from "lucide-react";
import { useMemo, useState, type UIEvent } from "react";
import { DeviceMenu } from "@/components/player/DeviceMenu";
import { PlaybackControls } from "@/components/player/PlaybackControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { WaveformProgress } from "@/components/player/WaveformProgress";
import { cn } from "@/lib/utils";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";
import type { LibraryView, Track } from "@/types/track";

interface PageCopy {
  title: string;
  kicker: string;
  description: string;
}

const TRACK_ROW_HEIGHT = 57;
const TRACK_ROW_OVERSCAN = 6;

const copy: Record<LibraryView, PageCopy> = {
  local: {
    title: "本地音乐",
    kicker: "Local Library",
    description: "管理本机已索引的无损曲目，点击任意曲目即可载入播放。",
  },
  recent: {
    title: "最近播放",
    kicker: "Recently Played",
    description: "按最近载入顺序排列，方便回到刚听过的曲目。",
  },
  liked: {
    title: "我喜欢",
    kicker: "Favorites",
    description: "这里保存你收藏的曲目，刷新或重启后会保留。",
  },
  playlists: {
    title: "歌单",
    kicker: "Playlists",
    description: "按听音场景组织曲库，后续可接入真实歌单文件。",
  },
  artists: {
    title: "艺术家",
    kicker: "Artists",
    description: "按艺术家聚合本地曲库。",
  },
  albums: {
    title: "专辑",
    kicker: "Albums",
    description: "按专辑浏览本地收藏。",
  },
};

function isTrack(track: Track | undefined): track is Track {
  return Boolean(track);
}

function compactQualityLabel(track: Track) {
  const prefix = `${track.format} `;
  return track.bitdepth.startsWith(prefix)
    ? track.bitdepth.slice(prefix.length)
    : track.bitdepth;
}

function PageHeader({ view }: { view: LibraryView }) {
  const meta = copy[view];

  return (
    <header className="flex items-end justify-between gap-6">
      <div className="space-y-1">
        <p className="text-[10px] uppercase tracking-[0.18em] text-cyan-700 font-bold">
          {meta.kicker}
        </p>
        <h1 className="text-3xl font-bold text-slate-800 tracking-tight">
          {meta.title}
        </h1>
        <p className="text-xs text-slate-500 leading-relaxed">
          {meta.description}
        </p>
      </div>
      <NowPlayingBadge />
    </header>
  );
}

function NowPlayingBadge() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  if (!track) return null;

  return (
    <div className="hidden xl:flex items-center rounded-lg bg-white/55 border border-black/[0.04] px-3 py-2 shadow-[0_8px_24px_rgba(15,23,42,0.04)]">
      <div className="min-w-0">
        <p className="text-[10px] text-slate-400">
          {isPlaying ? "正在播放" : "当前曲目"}
        </p>
        <p className="max-w-[220px] truncate text-xs font-semibold text-slate-800">
          {track.title}
        </p>
      </div>
    </div>
  );
}

function TrackRows({ tracks, empty }: { tracks: Track[]; empty: string }) {
  const playlist = usePlayerStore((s) => s.playlist);
  const currentTrack = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const liked = usePlayerStore((s) => s.liked);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const toggleLike = usePlayerStore((s) => s.toggleLike);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(420);
  const trackIndexById = useMemo(() => {
    const indexById = new Map<string, number>();
    playlist.forEach((track, index) => indexById.set(track.id, index));
    return indexById;
  }, [playlist]);
  const visibleRows = useMemo(() => {
    const start = Math.max(
      0,
      Math.floor(scrollTop / TRACK_ROW_HEIGHT) - TRACK_ROW_OVERSCAN
    );
    const end = Math.min(
      tracks.length,
      Math.ceil((scrollTop + viewportHeight) / TRACK_ROW_HEIGHT) +
        TRACK_ROW_OVERSCAN
    );

    return {
      tracks: tracks.slice(start, end),
      paddingTop: start * TRACK_ROW_HEIGHT,
      paddingBottom: (tracks.length - end) * TRACK_ROW_HEIGHT,
    };
  }, [scrollTop, tracks, viewportHeight]);
  const handleScroll = (event: UIEvent<HTMLDivElement>) => {
    const element = event.currentTarget;
    setScrollTop(element.scrollTop);
    setViewportHeight(element.clientHeight);
  };

  if (tracks.length === 0) {
    return (
      <div className="flex min-h-[260px] items-center justify-center rounded-lg border border-dashed border-black/[0.08] bg-white/35 text-sm text-slate-400">
        {empty}
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border border-black/[0.04] bg-white/45">
      <div className="grid grid-cols-[minmax(0,1fr)_120px_84px_44px] gap-4 border-b border-black/[0.04] px-4 py-2 text-[10px] font-bold uppercase tracking-wider text-slate-400">
        <span>曲目</span>
        <span>格式</span>
        <span>时长</span>
        <span />
      </div>
      <div
        className="max-h-[min(48vh,420px)] overflow-y-auto"
        onScroll={handleScroll}
      >
        <div style={{ paddingTop: visibleRows.paddingTop, paddingBottom: visibleRows.paddingBottom }}>
        {visibleRows.tracks.map((track) => {
          const index = trackIndexById.get(track.id) ?? -1;
          const active = currentTrack?.id === track.id;
          const favorite = !!liked[track.id];

          return (
            <div
              key={track.id}
              className={cn(
                "grid h-[57px] grid-cols-[minmax(0,1fr)_120px_84px_44px] items-center gap-4 border-b border-black/[0.03] px-4",
                active ? "bg-cyan-600/10" : "hover:bg-white/55"
              )}
            >
              <button
                type="button"
                onClick={() => {
                  if (index >= 0) loadTrack(index);
                }}
                disabled={index < 0}
                className="min-w-0 text-left disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={`播放 ${track.title}`}
              >
                <span className="min-w-0">
                  <span className="flex min-w-0 items-center gap-1.5 text-sm font-semibold text-slate-800">
                    {active ? (
                      isPlaying ? (
                        <Pause className="h-3 w-3 shrink-0 text-cyan-700" />
                      ) : (
                        <Play className="h-3 w-3 shrink-0 text-cyan-700" />
                      )
                    ) : null}
                    <span className="block min-w-0 truncate">
                      {track.title}
                    </span>
                  </span>
                  <span className="block truncate text-[11px] text-slate-500">
                    {track.artist} · {track.album}
                  </span>
                </span>
              </button>

              <span
                className="truncate text-[11px] font-semibold text-slate-500"
                title={track.bitdepth}
              >
                {compactQualityLabel(track)}
              </span>
              <span className="text-[11px] font-mono text-slate-500">
                {formatSeconds(track.duration)}
              </span>
              <button
                type="button"
                onClick={() => toggleLike(track.id)}
                className={cn(
                  "w-8 h-8 rounded-md flex items-center justify-center transition-colors",
                  favorite
                    ? "text-rose-500 bg-rose-500/10"
                    : "text-slate-400 hover:text-rose-500 hover:bg-rose-500/10"
                )}
                aria-label={favorite ? "取消收藏" : "收藏"}
              >
                <Heart className="w-3.5 h-3.5" fill={favorite ? "currentColor" : "none"} />
              </button>
            </div>
          );
        })}
        </div>
      </div>
    </div>
  );
}

function LocalPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  return <TrackRows tracks={playlist} empty="暂无本地曲目" />;
}

function RecentPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const trackById = useMemo(() => {
    const tracks = new Map<string, Track>();
    playlist.forEach((track) => tracks.set(track.id, track));
    return tracks;
  }, [playlist]);
  const tracks = useMemo(
    () => recentTrackIds.map((id) => trackById.get(id)).filter(isTrack),
    [recentTrackIds, trackById]
  );

  return <TrackRows tracks={tracks} empty="播放过的曲目会显示在这里" />;
}

function LikedPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const tracks = useMemo(
    () => playlist.filter((track) => liked[track.id]),
    [playlist, liked]
  );

  return <TrackRows tracks={tracks} empty="还没有收藏曲目" />;
}

function PlaylistsPage() {
  const setActiveView = usePlayerStore((s) => s.setActiveView);
  const playlist = usePlayerStore((s) => s.playlist);
  const liked = usePlayerStore((s) => s.liked);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const likedCount = useMemo(
    () => Object.values(liked).filter(Boolean).length,
    [liked]
  );
  const cards = useMemo(() => [
    {
      title: "Hi-Res 本地精选",
      count: playlist.length,
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
  ], [likedCount, playlist.length, recentTrackIds.length]);

  return (
    <div className="grid grid-cols-3 gap-4">
      {cards.map((card) => {
        const Icon = card.icon;
        return (
          <button
            key={card.title}
            type="button"
            onClick={() => setActiveView(card.view)}
            className="rounded-lg border border-black/[0.04] bg-white/50 p-4 text-left shadow-[0_8px_24px_rgba(15,23,42,0.04)] hover:bg-white/70 transition-colors"
          >
            <span className={cn("mb-6 flex h-10 w-10 items-center justify-center rounded-lg", card.color)}>
              <Icon className="h-4 w-4" />
            </span>
            <span className="block text-sm font-bold text-slate-800">{card.title}</span>
            <span className="mt-1 block text-[11px] text-slate-500">{card.count} 首曲目</span>
          </button>
        );
      })}
    </div>
  );
}

function ArtistsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const artists = useMemo(() => {
    const groups = new Map<
      string,
      { name: string; tracks: Track[]; firstIndex: number }
    >();

    playlist.forEach((track, index) => {
      const group = groups.get(track.artist);
      if (group) {
        group.tracks.push(track);
        return;
      }

      groups.set(track.artist, {
        name: track.artist,
        tracks: [track],
        firstIndex: index,
      });
    });

    return Array.from(groups.values());
  }, [playlist]);

  return (
    <div className="grid grid-cols-2 gap-4 overflow-y-auto pr-1">
      {artists.map((artist) => {
        const first = artist.tracks[0];
        return (
          <button
            key={artist.name}
            type="button"
            onClick={() => loadTrack(artist.firstIndex)}
            className="rounded-lg border border-black/[0.04] bg-white/50 p-4 text-left hover:bg-white/70 transition-colors"
          >
            <span className="mb-4 flex h-10 w-10 items-center justify-center rounded-lg bg-cyan-600/10 text-cyan-700">
              <User className="h-4 w-4" />
            </span>
            <span className="block text-sm font-bold text-slate-800">{artist.name}</span>
            <span className="mt-1 block text-[11px] text-slate-500">
              {artist.tracks.length} 首曲目 · {first.album}
            </span>
          </button>
        );
      })}
    </div>
  );
}

function AlbumsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const albums = useMemo(() => {
    const seen = new Set<string>();
    const albumTracks: Array<{ track: Track; index: number }> = [];

    playlist.forEach((track, index) => {
      if (seen.has(track.album)) return;
      seen.add(track.album);
      albumTracks.push({ track, index });
    });

    return albumTracks;
  }, [playlist]);

  return (
    <div className="grid grid-cols-2 gap-4 overflow-y-auto pr-1">
      {albums.map(({ track: album, index }) => {
        return (
          <button
            key={album.album}
            type="button"
            onClick={() => loadTrack(index)}
            className="flex gap-4 rounded-lg border border-black/[0.04] bg-white/50 p-3 text-left hover:bg-white/70 transition-colors"
          >
            {album.cover ? (
              <img
                src={album.cover}
                alt=""
                className="h-20 w-20 rounded-lg object-cover border border-black/[0.04]"
              />
            ) : null}
            <span className="min-w-0 self-center">
              <span className="block truncate text-sm font-bold text-slate-800">
                {album.album}
              </span>
              <span className="mt-1 block truncate text-[11px] text-slate-500">
                {album.artist} · {album.albumYear ?? "Unknown"}
              </span>
              <span className="mt-2 inline-flex items-center gap-1 rounded border border-seraph-gold/50 bg-seraph-gold-light px-1.5 py-0.5 text-[9px] font-bold text-seraph-gold-dark">
                <Disc3 className="h-2.5 w-2.5" />
                {album.format}
              </span>
            </span>
          </button>
        );
      })}
    </div>
  );
}

function MiniPlayer() {
  const track = usePlayerStore((s) => s.currentTrack());

  return (
    <footer className="rounded-lg border border-black/[0.04] bg-white/50 p-3 shadow-[0_8px_24px_rgba(15,23,42,0.04)]">
      <div className="flex items-center justify-between gap-5">
        <div className="min-w-0">
          <div className="min-w-0">
            <p className="truncate text-sm font-bold text-slate-800">
              {track ? track.title : "未选择曲目"}
            </p>
            <p className="truncate text-[11px] text-slate-500">
              {track ? track.artist : "添加本地音乐后可播放"}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-4">
          <PlaybackControls />
          <div className="flex items-center gap-3">
            <VolumeControl />
            <DeviceMenu />
          </div>
        </div>
      </div>
      <div className="mt-2">
        <WaveformProgress />
      </div>
    </footer>
  );
}

function PageBody({ view }: { view: LibraryView }) {
  if (view === "recent") return <RecentPage />;
  if (view === "liked") return <LikedPage />;
  if (view === "playlists") return <PlaylistsPage />;
  if (view === "artists") return <ArtistsPage />;
  if (view === "albums") return <AlbumsPage />;
  return <LocalPage />;
}

export function MainPages() {
  const activeView = usePlayerStore((s) => s.activeView);

  return (
    <main className="flex-1 min-w-0 min-h-0 flex flex-col gap-5 p-[clamp(18px,2.5vw,34px)] overflow-hidden z-10">
      <PageHeader view={activeView} />
      <section className="min-h-0 flex-1 overflow-hidden">
        <PageBody view={activeView} />
      </section>
      <MiniPlayer />
    </main>
  );
}
