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

const TRACK_ROW_HEIGHT = 59;
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
      <div className="flex min-h-[260px] items-center justify-center border-[1.5px] border-dashed border-line bg-card font-tw text-sm text-ink3">
        {empty}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-col">
      <div className="font-tw text-[10px] tracking-[3px] text-ink3 mb-3 flex justify-between">
        <span>INDEX — 播放队列</span>
        <span>{tracks.length} RECORDS</span>
      </div>
      <div
        className="min-h-0 flex-1 overflow-y-auto pr-1"
        onScroll={handleScroll}
      >
        <div style={{ paddingTop: visibleRows.paddingTop, paddingBottom: visibleRows.paddingBottom }}>
        {visibleRows.tracks.map((track) => {
          const index = trackIndexById.get(track.id) ?? -1;
          const active = currentTrack?.id === track.id;
          const favorite = !!liked[track.id];
          const recNo = (index >= 0 ? index + 1 : 0)
            .toString()
            .padStart(3, "0");
          const playing = active && isPlaying;

          return (
            <div
              key={track.id}
              className={cn(
                "archive-card relative grid h-[49px] grid-cols-[58px_minmax(0,1fr)_118px_64px_40px] items-center gap-3 px-4 mb-2.5",
                active && "is-playing"
              )}
            >
              {playing && (
                <div className="absolute -top-[7px] left-1/2 -translate-x-1/2 -rotate-2 w-[72px] h-[16px] bg-stamp/[0.18] border-x border-dashed border-stamp/30" />
              )}
              <div className="font-tw text-[11px] text-ink3 leading-tight">
                REC.
                <b className="block text-[15px] text-ink font-bold">
                  <span className={active ? "text-stamp" : undefined}>
                    {recNo}
                  </span>
                </b>
              </div>
              <button
                type="button"
                onClick={() => {
                  if (index >= 0) loadTrack(index);
                }}
                disabled={index < 0}
                className="min-w-0 text-left disabled:cursor-not-allowed disabled:opacity-50"
                aria-label={`播放 ${track.title}`}
              >
                <span className="flex min-w-0 items-center gap-1.5">
                  {playing && (
                    <span className="bars shrink-0">
                      <i />
                      <i />
                      <i />
                    </span>
                  )}
                  <span className="block min-w-0 truncate font-serif text-[15px] font-semibold text-ink leading-tight">
                    {track.title}
                  </span>
                </span>
                <span className="block truncate font-tw text-[11px] text-ink2 mt-0.5">
                  {track.artist} — {track.album}
                </span>
              </button>

              <div
                className={cn(
                  "font-tw text-[10px] leading-tight text-ink2",
                  track.cacheMissing && "text-stamp"
                )}
                title={track.bitdepth}
              >
                {track.cacheMissing ? (
                  <>
                    <b className="block text-stamp underline underline-offset-2">
                      ↻ 需重缓存
                    </b>
                    CACHE EXPIRED
                  </>
                ) : (
                  <>
                    <b className="block text-brown">
                      {compactQualityLabel(track)}
                    </b>
                    {track.format.toUpperCase()}
                  </>
                )}
              </div>
              <span className="text-right font-tw text-[13px] font-bold text-ink2">
                {formatSeconds(track.duration)}
              </span>
              <button
                type="button"
                onClick={() => toggleLike(track.id)}
                className={cn(
                  "w-8 h-8 flex items-center justify-center transition-colors",
                  favorite
                    ? "text-stamp"
                    : "text-ink3 hover:text-stamp"
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
    // L-9: 拉长到 3.5s 降低 B 站风控风险；登录成功 / 二维码过期会立即停止
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
    }, 3500);

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
      <div className="grid gap-2.5 border-[1.5px] border-ink bg-card p-2.5">
        <div className="grid gap-2 xl:grid-cols-[minmax(0,1fr)_auto] xl:items-center">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span
              className={cn(
                "inline-flex h-9 min-w-0 items-center gap-2 border-[1.5px] px-3 font-tw text-xs font-bold",
                loginStatus.loggedIn
                  ? "border-brown text-brown bg-paper2"
                  : "border-line text-ink2 bg-paper2"
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
              className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-50"
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
                className="stamp-btn inline-flex h-9 items-center justify-center px-3 font-tw text-xs font-bold"
              >
                <LogOut className="h-4 w-4" />
              </button>
            ) : null}
            <button
              type="button"
              onClick={() => void refreshBilibiliState()}
              className="stamp-btn inline-flex h-9 items-center justify-center px-3 font-tw text-xs font-bold"
            >
              <RefreshCw className="h-4 w-4" />
            </button>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <button
              type="button"
              onClick={() => setPreferDolbyAtmos((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 border-[1.5px] px-3 font-tw text-xs font-bold transition-colors",
                preferDolbyAtmos
                  ? "border-ink bg-ink text-paper"
                  : "border-line bg-card text-ink2 hover:border-ink"
              )}
            >
              <Headphones className="h-4 w-4" />
              <span>Dolby Atmos</span>
            </button>
            <button
              type="button"
              onClick={() => setPreferFlac((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 border-[1.5px] px-3 font-tw text-xs font-bold transition-colors",
                preferFlac
                  ? "border-ink bg-ink text-paper"
                  : "border-line bg-card text-ink2 hover:border-ink"
              )}
            >
              <Sparkles className="h-4 w-4" />
              <span>FLAC/Hi-Res</span>
            </button>
            <button
              type="button"
              onClick={() => setRemuxWithFfmpeg((value) => !value)}
              className={cn(
                "inline-flex h-9 items-center gap-2 border-[1.5px] px-3 font-tw text-xs font-bold transition-colors",
                remuxWithFfmpeg
                  ? "border-ink bg-ink text-paper"
                  : "border-line bg-card text-ink2 hover:border-ink"
              )}
            >
              <Settings2 className="h-4 w-4" />
              <span>Remux</span>
            </button>
            <span
              className={cn(
                "inline-flex h-9 items-center border-[1.5px] px-3 font-tw text-[11px] font-bold",
                ffmpegStatus.available
                  ? "border-brown bg-paper2 text-brown"
                  : "border-stamp bg-stamp-soft text-stamp"
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
            className="flex items-stretch border-[1.5px] border-ink bg-card"
          >
            <label className="grid min-w-0 flex-1 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 px-3 py-2 text-ink2">
              <Link2 className="h-4 w-4 shrink-0 text-brown" />
              <input
                value={bilibiliInput}
                onChange={(event) => setBilibiliInput(event.target.value)}
                placeholder="链接 / BV号 — 自动识别归档…"
                className="min-w-0 bg-transparent font-tw text-[13px] text-ink outline-none placeholder:text-ink3"
                disabled={isImporting}
              />
            </label>
            <button
              type="submit"
              disabled={!bilibiliInput.trim() || isImporting}
              className="inline-flex items-center gap-2 border-l-[1.5px] border-ink bg-ink px-4 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp disabled:cursor-not-allowed disabled:bg-line disabled:text-ink2"
            >
              {isImporting ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <DownloadCloud className="h-4 w-4" />
              )}
              <span>{isImporting ? "导入中" : "归档 →"}</span>
            </button>
          </form>

          <form
            onSubmit={handleImportFavorites}
            className="flex items-stretch border-[1.5px] border-ink bg-card"
          >
            <label className="grid min-w-0 flex-1 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 px-3 py-2 text-ink2">
              <FolderHeart className="h-4 w-4 shrink-0 text-brown" />
              <input
                value={favoriteInput}
                onChange={(event) => setFavoriteInput(event.target.value)}
                placeholder="收藏夹链接、media_id 或 fid"
                className="min-w-0 bg-transparent font-tw text-[13px] text-ink outline-none placeholder:text-ink3"
                disabled={isBatchImporting}
              />
            </label>
            <button
              type="submit"
              disabled={!favoriteInput.trim() || isBatchImporting}
              className="inline-flex items-center gap-2 border-l-[1.5px] border-ink bg-card px-4 font-tw text-xs font-bold text-ink transition-colors hover:bg-paper2 disabled:cursor-not-allowed disabled:text-ink3"
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
        <div className="absolute left-2 top-12 z-20 w-[236px] border-2 border-ink bg-card p-4 shadow-[5px_5px_0_rgba(43,39,34,0.18)]">
          <div className="flex items-center justify-between">
            <span className="font-tw text-xs font-bold text-ink">哔哩哔哩登录</span>
            <button
              type="button"
              onClick={() => setLoginQr(null)}
              className="px-2 py-1 font-tw text-xs font-bold text-ink3 hover:text-stamp"
            >
              关闭
            </button>
          </div>
          <div className="mt-3 flex h-[184px] w-[184px] items-center justify-center border border-line bg-white">
            {loginQrDataUrl ? (
              <img src={loginQrDataUrl} alt="B 站登录二维码" className="h-[184px] w-[184px]" />
            ) : (
              <Loader2 className="h-5 w-5 animate-spin text-brown" />
            )}
          </div>
          <p className="mt-2 truncate text-center font-tw text-xs font-semibold text-ink2">
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
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
          <button
            type="button"
            onClick={() => setSelectedArtist(null)}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
          >
            <ArrowLeft className="h-4 w-4" />
            返回艺术家
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate font-serif text-sm font-bold text-ink">
              {activeArtist.name}
            </p>
            <p className="font-tw text-[11px] text-ink2">
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
            className="archive-card p-4 text-left"
          >
            <span className="mb-4 flex h-10 w-10 items-center justify-center border-[1.5px] border-ink bg-paper2 text-brown">
              <User className="h-4 w-4" />
            </span>
            <span className="block font-serif text-sm font-bold text-ink">{artist.name}</span>
            <span className="mt-1 block font-tw text-[11px] text-ink2">
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
        <div className="flex items-center justify-between gap-3 border-[1.5px] border-ink bg-card p-3">
          <button
            type="button"
            onClick={() => setSelectedAlbumKey(null)}
            className="stamp-btn inline-flex h-9 items-center gap-2 px-3 font-tw text-xs font-bold"
          >
            <ArrowLeft className="h-4 w-4" />
            返回专辑
          </button>
          <div className="min-w-0 text-right">
            <p className="truncate font-serif text-sm font-bold text-ink">
              {activeAlbum.title}
            </p>
            <p className="font-tw text-[11px] text-ink2">
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
            className="archive-card group flex gap-4 p-3 text-left"
          >
            {album.cover ? (
              <div className="overflow-hidden border-[1.5px] border-ink shrink-0">
                <img
                  src={album.cover}
                  alt=""
                  className="h-20 w-20 object-cover grayscale-[0.2] transition-transform duration-500 group-hover:scale-110"
                />
              </div>
            ) : null}
            <span className="min-w-0 self-center">
              <span className="block truncate font-serif text-sm font-bold text-ink">
                {album.title}
              </span>
              <span className="mt-1 block truncate font-tw text-[11px] text-ink2">
                {album.artist} · {album.year ?? "Unknown"}
              </span>
              <span className="mt-1 block truncate font-tw text-[11px] text-ink3">
                {album.tracks.length} 首曲目
              </span>
              <span className="mt-2 inline-flex items-center gap-1 border border-brown bg-paper2 px-1.5 py-0.5 font-tw text-[9px] font-bold text-brown">
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
  const isPlaying = usePlayerStore((s) => s.isPlaying);

  return (
    <footer className="border-t-2 border-ink bg-card px-4 py-3">
      <div className="flex items-center justify-between gap-5">
        <div className="flex min-w-0 items-center gap-4">
          <div className={cn("reel", isPlaying && "spinning")} />
          <div className="min-w-0">
            <p className="truncate font-serif text-sm font-semibold text-ink">
              {track ? track.title : "未选择曲目"}
            </p>
            <p className="truncate font-tw text-[10px] text-ink2">
              {track
                ? `${track.artist} · ${isPlaying ? "NOW PLAYING" : "PAUSED"}`
                : "添加本地音乐后可播放"}
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
