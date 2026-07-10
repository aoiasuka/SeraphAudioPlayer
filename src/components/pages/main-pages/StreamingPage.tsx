import { BadgeCheck, DownloadCloud, FolderHeart, Headphones, Link2, Loader2, LogIn, LogOut, QrCode, RefreshCw, Settings2, Sparkles } from "lucide-react";
import * as QRCode from "qrcode";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { cn } from "@/lib/utils";
import { invoke, listen } from "@/lib/tauri";
import { usePlayerStore, type BilibiliImportOptions } from "@/store/player";
import { TrackRows } from "./TrackRows";
import { isStreamingTrack } from "./trackFilters";

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

interface FfmpegDownloadProgress {
  stage: "download" | "extract" | "done" | "error";
  downloaded: number;
  total: number;
  percent: number;
  message?: string | null;
}

export function StreamingPage() {
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
  const [ffmpegDownload, setFfmpegDownload] = useState<{
    active: boolean;
    percent: number;
    message?: string;
  }>({ active: false, percent: 0 });
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

  // 订阅后端 ffmpeg 下载进度，实时更新按钮文案/百分比。
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void listen<FfmpegDownloadProgress>("seraph://ffmpeg-download", (progress) => {
      if (cancelled) return;
      if (progress.stage === "done") {
        setFfmpegDownload({ active: false, percent: 100 });
        return;
      }
      if (progress.stage === "error") {
        setFfmpegDownload({ active: false, percent: 0 });
        return;
      }
      setFfmpegDownload({
        active: true,
        percent: progress.percent >= 0 ? progress.percent : 0,
        message: progress.message ?? undefined,
      });
    }).then((fn) => {
      // cleanup 已先于 listen resolve 执行时立即注销，避免监听器泄漏
      if (cancelled) {
        fn();
        return;
      }
      unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const handleDownloadFfmpeg = async () => {
    if (ffmpegDownload.active) return;
    setFfmpegDownload({ active: true, percent: 0, message: "准备下载…" });
    showNotification("开始下载 FFmpeg，请保持网络畅通…");
    try {
      const status = await invoke<BilibiliFfmpegStatus>("download_ffmpeg");
      setFfmpegStatus(status);
      setFfmpegDownload({ active: false, percent: 100 });
      showNotification(
        status.available ? "FFmpeg 安装完成，现在可解码杜比/EAC3 了" : "FFmpeg 安装未完成"
      );
    } catch (err) {
      setFfmpegDownload({ active: false, percent: 0 });
      const reason = typeof err === "string" ? err : "下载失败";
      showNotification(reason);
      // eslint-disable-next-line no-console
      console.warn("download_ffmpeg failed", err);
    }
  };

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
      <div className="flex flex-col gap-4 border-[1.5px] border-ink bg-card p-4">
        {/* Top Control Panels */}
        <div className="grid gap-4 md:grid-cols-[1.2fr_1.8fr]">
          {/* Account Profile Section */}
          <div className="flex flex-col">
            <span className="font-tw text-[10px] tracking-[2px] text-ink3 mb-2 block uppercase">
              [ 01 // Account / 哔哩哔哩 ]
            </span>
            <div className="flex items-center gap-2">
              <span
                className={cn(
                  "inline-flex h-9 flex-1 min-w-0 items-center gap-2 border-[1.5px] px-3 font-tw text-xs font-bold bg-paper2/40",
                  loginStatus.loggedIn
                    ? "border-brown text-brown"
                    : "border-line text-ink2"
                )}
              >
                {loginStatus.face ? (
                  <img
                    src={loginStatus.face}
                    alt=""
                    className="h-5 w-5 rounded-full object-cover border border-ink/10"
                  />
                ) : loginStatus.loggedIn ? (
                  <BadgeCheck className="h-4 w-4 text-brown" />
                ) : (
                  <QrCode className="h-4 w-4 text-ink3" />
                )}
                <span className="truncate">
                  {loginStatus.loggedIn
                    ? loginStatus.username ?? "已登录"
                    : "未登录"}
                </span>
              </span>

              {loginStatus.loggedIn ? (
                <button
                  type="button"
                  onClick={logoutBilibili}
                  title="退出登录"
                  className="stamp-btn inline-flex h-9 w-9 items-center justify-center font-tw text-xs font-bold"
                >
                  <LogOut className="h-4 w-4" />
                </button>
              ) : (
                <button
                  type="button"
                  onClick={startBilibiliLogin}
                  disabled={isLoginBusy}
                  className="stamp-btn inline-flex h-9 items-center gap-1.5 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-50 shrink-0"
                >
                  {isLoginBusy ? (
                    <Loader2 className="h-4 w-4 animate-spin" />
                  ) : (
                    <LogIn className="h-4 w-4" />
                  )}
                  <span>登录</span>
                </button>
              )}

              <button
                type="button"
                onClick={() => void refreshBilibiliState()}
                title="刷新状态"
                className="stamp-btn inline-flex h-9 w-9 items-center justify-center font-tw text-xs font-bold shrink-0"
              >
                <RefreshCw className="h-4 w-4" />
              </button>
            </div>
          </div>

          {/* Preferences Section */}
          <div className="flex flex-col">
            <span className="font-tw text-[10px] tracking-[2px] text-ink3 mb-2 block uppercase">
              [ 02 // Preferences / 下载设置 ]
            </span>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
              <button
                type="button"
                onClick={() => setPreferDolbyAtmos((value) => !value)}
                className={cn(
                  "inline-flex h-9 items-center justify-center gap-1.5 border-[1.5px] px-2 font-tw text-xs font-bold transition-all",
                  preferDolbyAtmos
                    ? "border-ink bg-ink text-paper"
                    : "border-line bg-card text-ink2 hover:border-ink"
                )}
              >
                <Headphones className="h-3.5 w-3.5" />
                <span>杜比全景声</span>
              </button>
              <button
                type="button"
                onClick={() => setPreferFlac((value) => !value)}
                className={cn(
                  "inline-flex h-9 items-center justify-center gap-1.5 border-[1.5px] px-2 font-tw text-xs font-bold transition-all",
                  preferFlac
                    ? "border-ink bg-ink text-paper"
                    : "border-line bg-card text-ink2 hover:border-ink"
                )}
              >
                <Sparkles className="h-3.5 w-3.5" />
                <span>FLAC/无损</span>
              </button>
              <button
                type="button"
                onClick={() => setRemuxWithFfmpeg((value) => !value)}
                className={cn(
                  "inline-flex h-9 items-center justify-center gap-1.5 border-[1.5px] px-2 font-tw text-xs font-bold transition-all",
                  remuxWithFfmpeg
                    ? "border-ink bg-ink text-paper"
                    : "border-line bg-card text-ink2 hover:border-ink"
                )}
              >
                <Settings2 className="h-3.5 w-3.5" />
                <span>FFmpeg混流</span>
              </button>
              <span
                className={cn(
                  "inline-flex h-9 items-center justify-center border-[1.5px] px-2 font-tw text-[11px] font-bold text-center",
                  ffmpegStatus.available
                    ? "border-brown bg-paper2/40 text-brown"
                    : "border-stamp bg-stamp-soft text-stamp"
                )}
                title={ffmpegStatus.path ?? undefined}
              >
                FFmpeg: {ffmpegStatus.available ? "可用" : "未找到"}
              </span>
              {!ffmpegStatus.available && (
                <button
                  type="button"
                  onClick={handleDownloadFfmpeg}
                  disabled={ffmpegDownload.active}
                  title="下载并安装 ffmpeg/ffprobe 以支持杜比全景声 / EAC3 解码"
                  className={cn(
                    "inline-flex h-9 items-center justify-center gap-1.5 border-[1.5px] px-2 font-tw text-[11px] font-bold transition-all",
                    ffmpegDownload.active
                      ? "border-line bg-card text-ink3 cursor-not-allowed"
                      : "border-ink bg-ink text-paper hover:opacity-90"
                  )}
                >
                  {ffmpegDownload.active ? (
                    <>
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      <span>
                        {ffmpegDownload.percent > 0
                          ? `下载中 ${Math.round(ffmpegDownload.percent)}%`
                          : ffmpegDownload.message ?? "下载中…"}
                      </span>
                    </>
                  ) : (
                    <>
                      <DownloadCloud className="h-3.5 w-3.5" />
                      <span>下载 FFmpeg</span>
                    </>
                  )}
                </button>
              )}
            </div>
          </div>
        </div>

        {/* Divider line */}
        <div className="border-t border-dashed border-line my-0.5" />

        {/* Ingestion Actions */}
        <div className="flex flex-col">
          <span className="font-tw text-[10px] tracking-[2px] text-ink3 mb-2 block uppercase">
            [ 03 // Ingestion / 音频归档任务 ]
          </span>
          <div className="grid gap-3 md:grid-cols-2">
            <form
              onSubmit={handleImportBilibili}
              className="flex items-stretch border-[1.5px] border-ink bg-card"
            >
              <label className="grid min-w-0 flex-1 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 px-3 py-2 text-ink2 cursor-text">
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
                className="inline-flex items-center gap-2 border-l-[1.5px] border-ink bg-ink px-4 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp disabled:cursor-not-allowed disabled:bg-line disabled:text-ink2 shrink-0"
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
              <label className="grid min-w-0 flex-1 grid-cols-[18px_minmax(0,1fr)] items-center gap-2 px-3 py-2 text-ink2 cursor-text">
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
                className="inline-flex items-center gap-2 border-l-[1.5px] border-ink bg-card px-4 font-tw text-xs font-bold text-ink transition-colors hover:bg-paper2 disabled:cursor-not-allowed disabled:text-ink3 shrink-0"
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

