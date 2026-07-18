import { Headphones, Loader2, RotateCw, Settings2, Sparkles } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Dialog } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { useContextMenuStore } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";

/**
 * B 站流媒体「重新加载」弹窗（v0.4.4）。
 * 让用户按当次需求勾选杜比 / FLAC / 混流后重新解析下载，原位替换曲库记录。
 * 三项默认全不勾选（B 站流媒体的重新加载默认走最省流量/最兼容的普通音质）。
 */
export function ReloadStreamingDialog() {
  const reloadStreamingTrackId = useContextMenuStore(
    (s) => s.reloadStreamingTrackId
  );
  const closeReloadStreaming = useContextMenuStore(
    (s) => s.closeReloadStreaming
  );
  const playlist = usePlayerStore((s) => s.playlist);
  const reloadStreamingTrack = usePlayerStore((s) => s.reloadStreamingTrack);

  const track = useMemo(
    () => playlist.find((item) => item.id === reloadStreamingTrackId) ?? null,
    [playlist, reloadStreamingTrackId]
  );

  const [preferDolbyAtmos, setPreferDolbyAtmos] = useState(false);
  const [preferFlac, setPreferFlac] = useState(false);
  const [remuxWithFfmpeg, setRemuxWithFfmpeg] = useState(false);
  const [isReloading, setIsReloading] = useState(false);

  // 每次打开都重置为「全不勾选」的默认态
  useEffect(() => {
    if (track) {
      setPreferDolbyAtmos(false);
      setPreferFlac(false);
      setRemuxWithFfmpeg(false);
      setIsReloading(false);
    }
  }, [track]);

  const handleClose = () => {
    if (!isReloading) closeReloadStreaming();
  };

  const handleReload = async () => {
    if (!track || isReloading) return;
    setIsReloading(true);
    try {
      const ok = await reloadStreamingTrack(track.id, {
        preferFlac,
        preferDolbyAtmos,
        remuxWithFfmpeg,
      });
      if (ok) closeReloadStreaming();
    } finally {
      setIsReloading(false);
    }
  };

  const toggles: {
    key: "dolby" | "flac" | "remux";
    label: string;
    hint: string;
    icon: typeof Headphones;
    active: boolean;
    onToggle: () => void;
  }[] = [
    {
      key: "dolby",
      label: "杜比全景声",
      hint: "优先解析杜比全景声 / EAC3 音轨（需 FFmpeg）",
      icon: Headphones,
      active: preferDolbyAtmos,
      onToggle: () => setPreferDolbyAtmos((value) => !value),
    },
    {
      key: "flac",
      label: "FLAC 无损",
      hint: "优先解析 FLAC / Hi-Res 无损音轨",
      icon: Sparkles,
      active: preferFlac,
      onToggle: () => setPreferFlac((value) => !value),
    },
    {
      key: "remux",
      label: "FFmpeg 混流",
      hint: "用 FFmpeg 重新封装，兼容性更好",
      icon: Settings2,
      active: remuxWithFfmpeg,
      onToggle: () => setRemuxWithFfmpeg((value) => !value),
    },
  ];

  return (
    <Dialog open={Boolean(track)} onClose={handleClose} className="max-w-sm">
      <div className="space-y-4">
        <div>
          <p className="font-tw text-[10px] font-bold uppercase tracking-[0.18em] text-stamp">
            Reload Stream
          </p>
          <h2 className="mt-1 font-serif text-lg font-bold text-ink">
            重新加载
          </h2>
          <p className="mt-2 truncate font-tw text-xs text-ink2">
            「{track?.title}」
          </p>
          <p className="mt-1 font-tw text-[11px] leading-relaxed text-ink3">
            选择本次重新加载的音质偏好，默认按普通音质加载。
          </p>
        </div>

        <div className="space-y-2">
          {toggles.map((toggle) => {
            const Icon = toggle.icon;
            return (
              <button
                key={toggle.key}
                type="button"
                disabled={isReloading}
                onClick={toggle.onToggle}
                aria-pressed={toggle.active}
                className={cn(
                  "flex w-full items-center gap-3 border-[1.5px] px-3 py-2 text-left transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                  toggle.active
                    ? "border-ink bg-ink text-paper"
                    : "border-line bg-card text-ink2 hover:border-ink"
                )}
              >
                <Icon className="h-4 w-4 shrink-0" />
                <span className="min-w-0 flex-1">
                  <span className="block font-tw text-xs font-bold">
                    {toggle.label}
                  </span>
                  <span
                    className={cn(
                      "block font-tw text-[10px] leading-tight",
                      toggle.active ? "text-paper/70" : "text-ink3"
                    )}
                  >
                    {toggle.hint}
                  </span>
                </span>
                <span
                  className={cn(
                    "flex h-4 w-4 shrink-0 items-center justify-center border-[1.5px] font-tw text-[10px] font-bold",
                    toggle.active
                      ? "border-paper bg-paper text-ink"
                      : "border-line text-transparent"
                  )}
                >
                  ✓
                </span>
              </button>
            );
          })}
        </div>

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={handleClose}
            disabled={isReloading}
            className="stamp-btn h-9 px-3 font-tw text-xs font-bold disabled:cursor-not-allowed disabled:opacity-60"
          >
            取消
          </button>
          <button
            type="button"
            onClick={() => void handleReload()}
            disabled={isReloading}
            className="inline-flex h-9 items-center gap-2 border-[1.5px] border-ink bg-ink px-3 font-tw text-xs font-bold text-paper transition-colors hover:bg-stamp hover:border-stamp disabled:cursor-wait disabled:opacity-70"
          >
            {isReloading ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <RotateCw className="h-3.5 w-3.5" />
            )}
            <span>{isReloading ? "加载中" : "重新加载"}</span>
          </button>
        </div>
      </div>
    </Dialog>
  );
}
