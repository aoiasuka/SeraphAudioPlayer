import { BadgeCheck, DownloadCloud, FolderHeart, Headphones, Link2, Loader2, LogIn, LogOut, QrCode, RefreshCw, Settings2, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { cn } from "@/lib/utils";
import { usePlayerStore, type BilibiliImportOptions } from "@/store/player";
import { TrackRows } from "./TrackRows";
import { isStreamingTrack } from "./trackFilters";

export function StreamingPage() {
  const playlist = usePlayerStore((s) => s.playlist);
  const streamingTracks = useMemo(
    () => playlist.filter(isStreamingTrack),
    [playlist]
  );
  const importBilibiliAudio = usePlayerStore((s) => s.importBilibiliAudio);
  const importBilibiliFavorites = usePlayerStore((s) => s.importBilibiliFavorites);
  // 审2-R5：登录/FFmpeg 状态、下载进度与扫码轮询全部提升到 store（streamingActions），
  // MainPages 的 key={activeView} 切页卸载不再丢下载进度、不再中断登录轮询。
  const loginStatus = usePlayerStore((s) => s.bilibiliLoginStatus);
  const ffmpegStatus = usePlayerStore((s) => s.bilibiliFfmpegStatus);
  const ffmpegDownload = usePlayerStore((s) => s.ffmpegDownload);
  const loginQr = usePlayerStore((s) => s.loginQr);
  const isLoginBusy = usePlayerStore((s) => s.isLoginBusy);
  const refreshBilibiliState = usePlayerStore((s) => s.refreshBilibiliState);
  const startFfmpegDownload = usePlayerStore((s) => s.startFfmpegDownload);
  const startLoginPolling = usePlayerStore((s) => s.startLoginPolling);
  const stopLoginPolling = usePlayerStore((s) => s.stopLoginPolling);
  const logoutBilibili = usePlayerStore((s) => s.logoutBilibili);
  const [bilibiliInput, setBilibiliInput] = useState("");
  const [favoriteInput, setFavoriteInput] = useState("");
  const [isImporting, setIsImporting] = useState(false);
  const [isBatchImporting, setIsBatchImporting] = useState(false);
  const [preferDolbyAtmos, setPreferDolbyAtmos] = useState(true);
  const [preferFlac, setPreferFlac] = useState(true);
  const [remuxWithFfmpeg, setRemuxWithFfmpeg] = useState(true);

  const isFfmpegDownloading = ffmpegDownload.stage === "downloading";

  const importOptions: BilibiliImportOptions = {
    preferFlac,
    preferDolbyAtmos,
    remuxWithFfmpeg,
  };

  useEffect(() => {
    void refreshBilibiliState();
  }, [refreshBilibiliState]);

  const handleImportBilibili = async (event: FormEvent) => {
    event.preventDefault();
    const input = bilibiliInput.trim();
    if (!input || isImporting) return;

    setIsImporting(true);
    try {
      // 审2-R10（L-7）：导入失败保留输入框内容便于修正重试，与收藏夹分支行为对齐
      const imported = await importBilibiliAudio(input, importOptions);
      if (imported) setBilibiliInput("");
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
                  onClick={() => void logoutBilibili()}
                  title="退出登录"
                  className="stamp-btn inline-flex h-9 w-9 items-center justify-center font-tw text-xs font-bold"
                >
                  <LogOut className="h-4 w-4" />
                </button>
              ) : (
                <button
                  type="button"
                  onClick={() => void startLoginPolling()}
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
                  onClick={() => void startFfmpegDownload()}
                  disabled={isFfmpegDownloading}
                  title="下载并安装 ffmpeg/ffprobe 以支持杜比全景声 / EAC3 解码"
                  className={cn(
                    "inline-flex h-9 items-center justify-center gap-1.5 border-[1.5px] px-2 font-tw text-[11px] font-bold transition-all",
                    isFfmpegDownloading
                      ? "border-line bg-card text-ink3 cursor-not-allowed"
                      : "border-ink bg-ink text-paper hover:opacity-90"
                  )}
                >
                  {isFfmpegDownloading ? (
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
              onClick={stopLoginPolling}
              className="px-2 py-1 font-tw text-xs font-bold text-ink3 hover:text-stamp"
            >
              关闭
            </button>
          </div>
          <div className="mt-3 flex h-[184px] w-[184px] items-center justify-center border border-line bg-white">
            {loginQr.dataUrl ? (
              <img src={loginQr.dataUrl} alt="B 站登录二维码" className="h-[184px] w-[184px]" />
            ) : (
              <Loader2 className="h-5 w-5 animate-spin text-brown" />
            )}
          </div>
          <p className="mt-2 truncate text-center font-tw text-xs font-semibold text-ink2">
            {loginQr.message || "等待扫码"}
          </p>
        </div>
      ) : null}

      <TrackRows tracks={streamingTracks} empty="暂无流媒体曲目" />
    </div>
  );
}
