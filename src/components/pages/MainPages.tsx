import { usePlayerStore } from "@/store/player";
import type { LibraryView } from "@/types/track";
import { AlbumsPage } from "./main-pages/AlbumsPage";
import { ArtistsPage } from "./main-pages/ArtistsPage";
import { LocalPage } from "./main-pages/LocalPage";
import { MiniPlayer } from "./main-pages/MiniPlayer";
import { PlaylistsPage } from "./main-pages/PlaylistsPage";
import { RecentPage, LikedPage } from "./main-pages/RecentLikedPages";
import { StreamingPage } from "./main-pages/StreamingPage";

interface PageCopy {
  title: string;
  kicker: string;
  description: string;
}

const copy: Record<LibraryView, PageCopy> = {
  local: {
    title: "本地音乐",
    kicker: "Local Library",
    description: "管理本机已索引的无损曲目，点击任意曲目即可载入播放。",
  },
  streaming: {
    title: "流媒体",
    kicker: "Streaming",
    description: "粘贴 B 站视频链接或 BV 号，将音频缓存到本地后加入播放队列。",
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

function PageHeader({ view }: { view: LibraryView }) {
  const meta = copy[view];

  return (
    <header>
      <span className="file-tab">FILE — {meta.kicker.toUpperCase()}</span>
      <div className="folder-card px-8 py-6">
        <div className="absolute top-7 right-5 w-[70px] text-center font-tw text-[8px] font-bold tracking-wider text-stamp opacity-65 leading-[1.5] rotate-12 pointer-events-none">
          SERAPH
          <br />★<br />
          VERIFIED
        </div>
        <div className="flex items-end justify-between gap-6">
          <div>
            <div className="flex items-baseline gap-4">
              <h1 className="font-serif text-[34px] font-black text-ink leading-none">
                {meta.title}
              </h1>
              <span className="font-tw italic text-[13px] text-ink3">
                {meta.kicker.toLowerCase().replace(/\s+/g, "_")}.
              </span>
            </div>
            <p className="font-tw text-[12px] text-ink2 mt-2.5 max-w-[560px] leading-relaxed">
              &gt; {meta.description}
            </p>
          </div>
          <NowPlayingBadge />
        </div>
      </div>
    </header>
  );
}

function NowPlayingBadge() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  if (!track) return null;

  return (
    <div className="hidden xl:flex items-center border-[1.5px] border-ink bg-paper2 px-3 py-2">
      <div className="min-w-0">
        <p className="font-tw text-[9px] tracking-wider text-ink3">
          {isPlaying ? "● NOW PLAYING" : "○ CURRENT"}
        </p>
        <p className="max-w-[220px] truncate font-serif text-xs font-semibold text-ink mt-0.5">
          {track.title}
        </p>
      </div>
    </div>
  );
}



















function PageBody({ view }: { view: LibraryView }) {
  if (view === "streaming") return <StreamingPage />;
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
        <div key={activeView} className="h-full w-full animate-page-transition flex flex-col min-h-0">
          <PageBody view={activeView} />
        </div>
      </section>
      <MiniPlayer />
    </main>
  );
}
