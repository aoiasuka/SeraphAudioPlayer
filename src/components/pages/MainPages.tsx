import {
  ArrowLeft,
  BadgeCheck,
  Disc3,
  DownloadCloud,
  FolderHeart,
  Headphones,
  Heart,
  Link2,
  ListMusic,
  Loader2,
  LogIn,
  LogOut,
  Music2,
  Pause,
  Play,
  Plus,
  QrCode,
  RefreshCw,
  Settings2,
  Sparkles,
  Trash2,
  User,
} from "lucide-react";
import * as QRCode from "qrcode";
import { useEffect, useMemo, useState, type FormEvent, type UIEvent } from "react";
import { Dialog } from "@/components/ui/dialog";
import { DeviceMenu } from "@/components/player/DeviceMenu";
import { PlaybackControls } from "@/components/player/PlaybackControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { WaveformProgress } from "@/components/player/WaveformProgress";
import { cn } from "@/lib/utils";
import { formatSeconds } from "@/lib/format";
import { invoke } from "@/lib/tauri";
import { usePlayerStore, type BilibiliImportOptions } from "@/store/player";
import type { LibraryView, Track } from "@/types/track";

interface PageCopy {
  title: string;
  kicker: string;
  description: string;
}

interface BilibiliLoginStatus {
  loggedIn: boolean;
  username?: string | null;
  mid?: number | null;
  face?: string | null;
}

interface BilibiliLoginQrCode {
  url: string;
  qrcodeKey: string;
}

interface BilibiliLoginPollResult {
  code: number;
  message: string;
  loggedIn: boolean;
  profile?: BilibiliLoginStatus | null;
}

interface BilibiliFfmpegStatus {
  available: boolean;
  path?: string | null;
}

const TRACK_ROW_HEIGHT = 57;
const TRACK_ROW_OVERSCAN = 6;

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
                "grid h-[57px] grid-cols-[minmax(0,1fr)_120px_84px_44px] items-center gap-4 border-b border-black/[0.03] px-4 transition-all duration-300 relative overflow-hidden",
                active ? "bg-cyan-600/8" : "hover:bg-white/60 hover:translate-x-1"
              )}
            >
              {active && (
                <div className="absolute left-0 top-0 bottom-0 w-[3px] bg-gradient-to-b from-cyan-500 to-blue-600 shadow-[0_0_8px_rgba(8,145,178,0.6)]" />
              )}
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
                {track.cacheMissing ? "需重缓存" : compactQualityLabel(track)}
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

function isStreamingTrack(track: Track) {
  return (
    track.id.startsWith("bilibili-") ||
    track.sourceId?.trim().toLowerCase().startsWith("bv") ||
    track.sourceUrl?.trim().toLowerCase().includes("bilibili.com") ||
    track.album === "Bilibili"
  );
}

function isLocalTrack(track: Track) {
  return !isStreamingTrack(track);
}

function LocalPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  return <TrackRows tracks={localTracks} empty="暂无本地曲目" />;
}

function StreamingPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const streamingTracks = useMemo(
    () => playlist.filter(isStreamingTrack),
    [playlist]
  );
  const importBilibiliAudio = usePlayerStore((s) => s.importBilibiliAudio);
  const importBilibiliFavorites = usePlayerStore((s) => s.importBilibiliFavorites);
  const showNotification = usePlayerStore((s) => s.showNotification);
  const [bilibiliInput, setBilibiliInput] = useState("");
  const [favoriteInput, setFavoriteInput] = useState("");
  const [isImporting, setIsImporting] = useState(false);
  const [isBatchImporting, setIsBatchImporting] = useState(false);
  const [preferDolbyAtmos, setPreferDolbyAtmos] = useState(true);
  const [preferFlac, setPreferFlac] = useState(true);
  const [remuxWithFfmpeg, setRemuxWithFfmpeg] = useState(true);
  const [loginStatus, setLoginStatus] = useState<BilibiliLoginStatus>({
    loggedIn: false,
  });
  const [ffmpegStatus, setFfmpegStatus] = useState<BilibiliFfmpegStatus>({
    available: false,
  });
  const [loginQr, setLoginQr] = useState<BilibiliLoginQrCode | null>(null);
  const [loginQrDataUrl, setLoginQrDataUrl] = useState("");
  const [loginPollMessage, setLoginPollMessage] = useState("");
  const [isLoginBusy, setIsLoginBusy] = useState(false);

  const importOptions: BilibiliImportOptions = {
    preferFlac,
    preferDolbyAtmos,
    remuxWithFfmpeg,
  };

  const refreshBilibiliState = async () => {
    try {
      const [status, ffmpeg] = await Promise.all([
        invoke<BilibiliLoginStatus>("bilibili_login_status"),
        invoke<BilibiliFfmpegStatus>("bilibili_ffmpeg_status"),
      ]);
      setLoginStatus(status);
      setFfmpegStatus(ffmpeg);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: bilibili status", err);
    }
  };

  useEffect(() => {
    void refreshBilibiliState();
  }, []);

  useEffect(() => {
    let canceled = false;
    if (!loginQr?.url) {
      setLoginQrDataUrl("");
      return;
    }

    void QRCode.toDataURL(loginQr.url, {
      width: 184,
      margin: 1,
      color: { dark: "#0f172a", light: "#ffffff" },
    }).then((dataUrl) => {
      if (!canceled) setLoginQrDataUrl(dataUrl);
    });

    return () => {
      canceled = true;
    };
  }, [loginQr]);

  useEffect(() => {
    if (!loginQr?.qrcodeKey) return;

    let stopped = false;
    const timer = window.setInterval(() => {
      void invoke<BilibiliLoginPollResult>("bilibili_poll_login", {
        qrcodeKey: loginQr.qrcodeKey,
      })
        .then((result) => {
          if (stopped) return;
          setLoginPollMessage(result.message);
          if (result.loggedIn || result.code === 0) {
            setLoginStatus(
              result.profile ?? {
                loggedIn: true,
              }
            );
            setLoginQr(null);
            showNotification("B 站登录成功");
          } else if (result.code === 86038) {
            setLoginQr(null);
            showNotification("B 站二维码已过期");
          }
        })
        .catch((err) => {
          if (stopped) return;
          // eslint-disable-next-line no-console
          console.warn("Tauri command failed: bilibili_poll_login", err);
          setLoginPollMessage("登录轮询失败");
        });
    }, 1800);

    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [loginQr, showNotification]);

  const startBilibiliLogin = async () => {
    if (isLoginBusy) return;
    setIsLoginBusy(true);
    try {
      const qrcode = await invoke<BilibiliLoginQrCode>("bilibili_login_qrcode");
      setLoginQr(qrcode);
      setLoginPollMessage("等待扫码");
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: bilibili_login_qrcode", err);
      showNotification("无法生成 B 站登录二维码");
    } finally {
      setIsLoginBusy(false);
    }
  };

  const logoutBilibili = async () => {
    try {
      await invoke("bilibili_logout");
      setLoginStatus({ loggedIn: false });
      showNotification("已退出 B 站登录");
    } catch (err) {
      // eslint-disable-next-line no-console
      console.warn("Tauri command failed: bilibili_logout", err);
      showNotification("退出 B 站登录失败");
    }
  };

  const handleImportBilibili = async (event: FormEvent) => {
    event.preventDefault();
    const input = bilibiliInput.trim();
    if (!input || isImporting) return;

    setIsImporting(true);
    try {
      await importBilibiliAudio(input, importOptions);
      setBilibiliInput("");
    } finally {
      setIsImporting(false);
    }
  };

  const handleImportFavorites = async (event: FormEvent) => {
    event.preventDefault();
    const input = favoriteInput.trim();
    if (!input || isBatchImporting) return;

    setIsBatchImporting(true);
    try {
      const result = await importBilibiliFavorites(input, importOptions);
      if (result && result.tracks.length > 0) {
        setFavoriteInput("");
      }
    } finally {
      setIsBatchImporting(false);
    }
  };

  return (
    <div className="relative flex min-h-0 flex-col gap-3">
      <div className="grid gap-2 rounded-lg border border-black/[0.04] bg-white/50 p-2 shadow-[0_8px_24px_rgba(15,23,42,0.04)]">
        <div className="grid gap-2 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-center">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span
              className={cn(
                "inline-flex h-9 min-w-0 items-center gap-2 rounded-md px-3 text-xs font-bold",
                loginStatus.loggedIn
                  ? "bg-emerald-500/10 text-emerald-700"
                  : "bg-slate-900/5 text-slate-500"
              )}
            >
              {loginStatus.face ? (
                <img
                  src={loginStatus.face}
                  alt=""
                  className="h-5 w-5 rounded-full object-cover"
                />
              ) : loginStatus.loggedIn ? (
                <BadgeCheck className="h-4 w-4" />
              ) : (
                <QrCode className="h-4 w-4" />
              )}
              <span className="max-w-[180px] truncate">
                {loginStatus.loggedIn
                  ? loginStatus.username ?? "已登录"
                  : "未登录"}
              </span>
            </span>
            <button
              type="button"
              onClick={startBilibiliLogin}
              disabled={isLoginBusy}
              className="inline-flex h-9 items-center gap-2 rounded-md bg-cyan-700 px-3 text-xs font-bold text-white transition-colors hover:bg-cyan-800 disabled:cursor-not-allowed disabled:bg-slate-300"
            >
              {isLoginBusy ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <LogIn className="h-4 w-4" />
              )}
              <span>扫码登录</span>
            </button>
            {loginStatus.loggedIn ? (
              <button
                type="button"
                onClick={logoutBilibili}
                className="inline-flex h-9 items-center justify-center rounded-md bg-white/60 px-3 text-xs font-bold text-slate-500 transition-colors hover:bg-white hover:text-slate-700"
              >
                <LogOut className="h-4 w-4" />
              </button>
            ) : null}
            <button
              type="button"
              onClick={() => void refreshBilibiliState()}
              className="inline-flex h-9 items-center justify-center rounded-md bg-white/60 px-3 text-xs font-bold text-slate-500 transition-colors hover:bg-white hover:text-slate-700"
            >
              <RefreshCw className="h-4 w-4" />
            </button>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => setPreferDolbyAtmos((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 rounded-md px-3 text-xs font-bold transition-colors",
                preferDolbyAtmos
                  ? "bg-violet-500/10 text-violet-700"
                  : "bg-white/60 text-slate-500 hover:bg-white"
              )}
            >
              <Headphones className="h-4 w-4" />
              <span>Dolby Atmos</span>
            </button>
            <button
              type="button"
              onClick={() => setPreferFlac((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 rounded-md px-3 text-xs font-bold transition-colors",
                preferFlac
                  ? "bg-amber-400/20 text-amber-700"
                  : "bg-white/60 text-slate-500 hover:bg-white"
              )}
            >
              <Sparkles className="h-4 w-4" />
              <span>FLAC/Hi-Res</span>
            </button>
            <button
              type="button"
              onClick={() => setRemuxWithFfmpeg((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 rounded-md px-3 text-xs font-bold transition-colors",
                remuxWithFfmpeg
                  ? "bg-cyan-600/10 text-cyan-700"
                  : "bg-white/60 text-slate-500 hover:bg-white"
              )}
            >
              <Settings2 className="h-4 w-4" />
              <span>Remux</span>
            </button>
            <span
              className={cn(
                "inline-flex h-9 items-center rounded-md px-3 text-[11px] font-bold",
                ffmpegStatus.available
                  ? "bg-emerald-500/10 text-emerald-700"
                  : "bg-rose-500/10 text-rose-600"
              )}
              title={ffmpegStatus.path ?? undefined}
            >
              ffmpeg {ffmpegStatus.available ? "可用" : "未找到"}
            </span>
          </div>
        </div>

        <div className="grid gap-2 xl:grid-cols-2">
          <form
            onSubmit={handleImportBilibili}
            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2"
          >
            <label className="grid min-w-0 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 rounded-md bg-white/55 px-3 py-2 text-slate-500">
              <Link2 className="h-4 w-4 shrink-0 text-cyan-700" />
              <input
                value={bilibiliInput}
                onChange={(event) => setBilibiliInput(event.target.value)}
                placeholder="B 站视频链接或 BV 号"
                className="min-w-0 bg-transparent text-sm font-medium text-slate-800 outline-none placeholder:text-slate-400"
                disabled={isImporting}
              />
            </label>
            <button
              type="submit"
              disabled={!bilibiliInput.trim() || isImporting}
              className="inline-flex h-10 items-center gap-2 rounded-lg bg-cyan-700 px-3 text-xs font-bold text-white shadow-[0_8px_20px_rgba(8,145,178,0.18)] transition-colors hover:bg-cyan-800 disabled:cursor-not-allowed disabled:bg-slate-300 disabled:shadow-none"
            >
              {isImporting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <DownloadCloud className="h-4 w-4" />
              )}
              <span>{isImporting ? "导入中" : "添加"}</span>
            </button>
          </form>

          <form
            onSubmit={handleImportFavorites}
            className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2"
          >
            <label className="grid min-w-0 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 rounded-md bg-white/55 px-3 py-2 text-slate-500">
              <FolderHeart className="h-4 w-4 shrink-0 text-cyan-700" />
              <input
                value={favoriteInput}
                onChange={(event) => setFavoriteInput(event.target.value)}
                placeholder="收藏夹链接、media_id 或 fid"
                className="min-w-0 bg-transparent text-sm font-medium text-slate-800 outline-none placeholder:text-slate-400"
                disabled={isBatchImporting}
              />
            </label>
            <button
              type="submit"
              disabled={!favoriteInput.trim() || isBatchImporting}
              className="inline-flex h-10 items-center gap-2 rounded-lg bg-slate-800 px-3 text-xs font-bold text-white shadow-[0_8px_20px_rgba(15,23,42,0.14)] transition-colors hover:bg-slate-900 disabled:cursor-not-allowed disabled:bg-slate-300 disabled:shadow-none"
            >
              {isBatchImporting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <FolderHeart className="h-4 w-4" />
              )}
              <span>{isBatchImporting ? "批量中" : "批量"}</span>
            </button>
          </form>
        </div>
      </div>

      {loginQr ? (
        <div className="absolute left-2 top-12 z-20 w-[236px] rounded-lg border border-black/[0.06] bg-white p-4 shadow-[0_18px_48px_rgba(15,23,42,0.18)]">
          <div className="flex items-center justify-between">
            <span className="text-xs font-bold text-slate-800">哔哩哔哩登录</span>
            <button
              type="button"
              onClick={() => setLoginQr(null)}
              className="rounded-md px-2 py-1 text-xs font-bold text-slate-400 hover:bg-slate-100 hover:text-slate-700"
            >
              关闭
            </button>
          </div>
          <div className="mt-3 flex h-[184px] w-[184px] items-center justify-center rounded-lg bg-white">
            {loginQrDataUrl ? (
              <img src={loginQrDataUrl} alt="B 站登录二维码" className="h-[184px] w-[184px]" />
            ) : (
              <Loader2 className="h-5 w-5 animate-spin text-cyan-700" />
            )}
          </div>
          <p className="mt-2 truncate text-center text-xs font-semibold text-slate-500">
            {loginPollMessage || "等待扫码"}
          </p>
        </div>
      ) : null}

      <TrackRows tracks={streamingTracks} empty="暂无流媒体曲目" />
    </div>
  );
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
    () => Object.values(liked).filter(Boolean).length,
    [liked]
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
      <div className="mb-4 flex justify-end">
        <button
          type="button"
          onClick={openCreateDialog}
          className="inline-flex h-9 items-center gap-2 rounded-md bg-cyan-700 px-3 text-xs font-bold text-white shadow-[0_8px_22px_rgba(8,145,178,0.16)] transition-colors hover:bg-cyan-800 active:scale-[0.98]"
        >
          <Plus className="h-4 w-4" />
          <span>新增歌单</span>
        </button>
      </div>
      <div className="grid grid-cols-3 gap-4">
        {cards.map((card) => {
          const Icon = card.icon;
          return (
            <button
              key={card.title}
              type="button"
              onClick={() => setActiveView(card.view)}
              className="rounded-lg border border-black/[0.04] bg-white/50 p-4 text-left shadow-[0_8px_24px_rgba(15,23,42,0.04)] hover:bg-white/80 hover:-translate-y-1 hover:shadow-[0_12px_32px_rgba(15,23,42,0.08)] active:scale-98 transition-all duration-300"
            >
              <span className={cn("mb-6 flex h-10 w-10 items-center justify-center rounded-lg", card.color)}>
                <Icon className="h-4 w-4" />
              </span>
              <span className="block text-sm font-bold text-slate-800">{card.title}</span>
              <span className="mt-1 block text-[11px] text-slate-500">{card.count} 首曲目</span>
            </button>
          );
        })}
        {userPlaylists.map((item) => (
          <div
            key={item.id}
            className="group relative rounded-lg border border-black/[0.04] bg-white/50 p-4 text-left shadow-[0_8px_24px_rgba(15,23,42,0.04)] transition-all duration-300 hover:-translate-y-1 hover:bg-white/80 hover:shadow-[0_12px_32px_rgba(15,23,42,0.08)]"
          >
            <button
              type="button"
              onClick={() => setPlaylistToDeleteId(item.id)}
              className="absolute right-3 top-3 flex h-8 w-8 items-center justify-center rounded-md text-slate-300 opacity-0 transition-all hover:bg-rose-500/10 hover:text-rose-600 group-hover:opacity-100 focus:opacity-100"
              aria-label={`删除歌单 ${item.name}`}
              title="删除歌单"
            >
              <Trash2 className="h-3.5 w-3.5" />
            </button>
            <span className="mb-6 flex h-10 w-10 items-center justify-center rounded-lg bg-emerald-500/10 text-emerald-700">
              <ListMusic className="h-4 w-4" />
            </span>
            <span className="block truncate text-sm font-bold text-slate-800">
              {item.name}
            </span>
            <span className="mt-1 block text-[11px] text-slate-500">
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
            <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-cyan-700">
              New Playlist
            </p>
            <h2 className="mt-1 text-lg font-bold text-slate-800">
              新增歌单
            </h2>
          </div>
          <label className="block space-y-1.5">
            <span className="text-[11px] font-bold text-slate-500">
              歌单名称
            </span>
            <input
              value={newPlaylistName}
              onChange={(event) => setNewPlaylistName(event.target.value)}
              autoFocus
              className="h-10 w-full rounded-md border border-slate-200 bg-white px-3 text-sm font-semibold text-slate-800 outline-none transition-colors placeholder:text-slate-400 focus:border-cyan-300 focus:ring-2 focus:ring-cyan-100"
              placeholder="输入歌单名称"
            />
          </label>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setCreateDialogOpen(false)}
              className="h-9 rounded-md border border-slate-200 bg-white px-3 text-xs font-bold text-slate-600 transition-colors hover:border-slate-300 hover:bg-slate-50"
            >
              取消
            </button>
            <button
              type="submit"
              disabled={!newPlaylistName.trim()}
              className="h-9 rounded-md bg-cyan-700 px-3 text-xs font-bold text-white transition-colors hover:bg-cyan-800 disabled:cursor-not-allowed disabled:bg-slate-300"
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
            <p className="text-[10px] font-bold uppercase tracking-[0.18em] text-rose-600">
              Delete Playlist
            </p>
            <h2 className="mt-1 text-lg font-bold text-slate-800">
              删除歌单
            </h2>
            <p className="mt-2 text-xs leading-relaxed text-slate-500">
              确定删除“{playlistToDelete?.name}”吗？歌单内的曲目不会从曲库中删除。
            </p>
          </div>
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={() => setPlaylistToDeleteId(null)}
              className="h-9 rounded-md border border-slate-200 bg-white px-3 text-xs font-bold text-slate-600 transition-colors hover:border-slate-300 hover:bg-slate-50"
            >
              取消
            </button>
            <button
              type="button"
              onClick={handleDeletePlaylist}
              className="h-9 rounded-md bg-rose-600 px-3 text-xs font-bold text-white transition-colors hover:bg-rose-700"
            >
              删除
            </button>
          </div>
        </div>
      </Dialog>
    </>
  );
}

function ArtistsPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const [selectedArtist, setSelectedArtist] = useState<string | null>(null);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  const artists = useMemo(() => {
    const groups = new Map<
      string,
      { name: string; tracks: Track[] }
    >();

    localTracks.forEach((track) => {
      const group = groups.get(track.artist);
      if (group) {
        group.tracks.push(track);
        return;
      }

      groups.set(track.artist, {
        name: track.artist,
        tracks: [track],
      });
    });

    return Array.from(groups.values());
  }, [localTracks]);
  const activeArtist = useMemo(
    () => artists.find((artist) => artist.name === selectedArtist) ?? null,
    [artists, selectedArtist]
  );

  useEffect(() => {
    if (selectedArtist && !activeArtist) setSelectedArtist(null);
  }, [activeArtist, selectedArtist]);

  if (activeArtist) {
    return (
      <div className="flex min-h-0 flex-col gap-3">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-black/[0.04] bg-white/50 p-3">
          <button
            type="button"
            onClick={() => setSelectedArtist(null)}
            className="inline-flex h-9 items-center gap-2 rounded-md bg-white/65 px-3 text-xs font-bold text-slate-600 transition-colors hover:bg-white hover:text-cyan-700"
          >
            <ArrowLeft className="h-4 w-4" />
            返回艺术家
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate text-sm font-bold text-slate-800">
              {activeArtist.name}
            </p>
            <p className="text-[11px] text-slate-500">
              {activeArtist.tracks.length} 首曲目
            </p>
          </div>
        </div>
        <TrackRows
          tracks={activeArtist.tracks}
          empty={`${activeArtist.name} 暂无曲目`}
        />
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-4 overflow-y-auto pr-1">
      {artists.map((artist) => {
        const first = artist.tracks[0];
        return (
          <button
            key={artist.name}
            type="button"
            onClick={() => setSelectedArtist(artist.name)}
            className="rounded-lg border border-black/[0.04] bg-white/50 p-4 text-left shadow-[0_8px_24px_rgba(15,23,42,0.04)] hover:bg-white/80 hover:-translate-y-1 hover:shadow-[0_12px_32px_rgba(15,23,42,0.08)] active:scale-98 transition-all duration-300"
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
  const [selectedAlbumKey, setSelectedAlbumKey] = useState<string | null>(null);
  const localTracks = useMemo(() => playlist.filter(isLocalTrack), [playlist]);
  const albums = useMemo(() => {
    const groups = new Map<
      string,
      {
        key: string;
        title: string;
        artist: string;
        year?: string;
        format: string;
        cover: string;
        tracks: Track[];
      }
    >();

    localTracks.forEach((track) => {
      const key = `${track.artist}::${track.album}`;
      const group = groups.get(key);
      if (group) {
        group.tracks.push(track);
        return;
      }

      groups.set(key, {
        key,
        title: track.album,
        artist: track.artist,
        year: track.albumYear,
        format: track.format,
        cover: track.cover,
        tracks: [track],
      });
    });

    return Array.from(groups.values());
  }, [localTracks]);
  const activeAlbum = useMemo(
    () => albums.find((album) => album.key === selectedAlbumKey) ?? null,
    [albums, selectedAlbumKey]
  );

  useEffect(() => {
    if (selectedAlbumKey && !activeAlbum) setSelectedAlbumKey(null);
  }, [activeAlbum, selectedAlbumKey]);

  if (activeAlbum) {
    return (
      <div className="flex min-h-0 flex-col gap-3">
        <div className="flex items-center justify-between gap-3 rounded-lg border border-black/[0.04] bg-white/50 p-3">
          <button
            type="button"
            onClick={() => setSelectedAlbumKey(null)}
            className="inline-flex h-9 items-center gap-2 rounded-md bg-white/65 px-3 text-xs font-bold text-slate-600 transition-colors hover:bg-white hover:text-cyan-700"
          >
            <ArrowLeft className="h-4 w-4" />
            返回专辑
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate text-sm font-bold text-slate-800">
              {activeAlbum.title}
            </p>
            <p className="text-[11px] text-slate-500">
              {activeAlbum.artist} · {activeAlbum.tracks.length} 首曲目
            </p>
          </div>
        </div>
        <TrackRows
          tracks={activeAlbum.tracks}
          empty={`${activeAlbum.title} 暂无曲目`}
        />
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-4 overflow-y-auto pr-1">
      {albums.map((album) => {
        return (
          <button
            key={album.key}
            type="button"
            onClick={() => setSelectedAlbumKey(album.key)}
            className="flex gap-4 rounded-lg border border-black/[0.04] bg-white/50 p-3 text-left shadow-[0_8px_24px_rgba(15,23,42,0.04)] hover:bg-white/80 hover:-translate-y-1 hover:shadow-[0_12px_32px_rgba(15,23,42,0.08)] active:scale-98 transition-all duration-300 group"
          >
            {album.cover ? (
              <div className="overflow-hidden rounded-lg border border-black/[0.04] shrink-0">
                <img
                  src={album.cover}
                  alt=""
                  className="h-20 w-20 object-cover transition-transform duration-500 group-hover:scale-110"
                />
              </div>
            ) : null}
            <span className="min-w-0 self-center">
              <span className="block truncate text-sm font-bold text-slate-800">
                {album.title}
              </span>
              <span className="mt-1 block truncate text-[11px] text-slate-500">
                {album.artist} · {album.year ?? "Unknown"}
              </span>
              <span className="mt-1 block truncate text-[11px] text-slate-400">
                {album.tracks.length} 首曲目
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
