import { useEffect, useRef, useState } from "react";
import { DeviceMenu } from "@/components/player/DeviceMenu";
import { PlaybackControls } from "@/components/player/PlaybackControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { WaveformProgress } from "@/components/player/WaveformProgress";
import { coverSrc } from "@/lib/tauri";
import { buildCurrentTrackMenuEntries } from "@/lib/trackMenu";
import { cn } from "@/lib/utils";
import { showContextMenu } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";
import { useImmersiveStore } from "@/store/immersive";

export function MiniPlayer() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const immersiveOpen = useImmersiveStore((s) => s.isOpen);
  const openImmersive = useImmersiveStore((s) => s.open);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const wasImmersiveOpen = useRef(false);

  useEffect(() => {
    if (wasImmersiveOpen.current && !immersiveOpen && track) {
      triggerRef.current?.focus({ preventScroll: true });
    }
    wasImmersiveOpen.current = immersiveOpen;
  }, [immersiveOpen, track]);
  // 封面加载失败时回退到默认转盘刻度样式
  const [coverFailed, setCoverFailed] = useState(false);
  const cover = coverSrc(track?.cover);

  useEffect(() => {
    setCoverFailed(false);
  }, [cover]);

  const showCover = cover !== "" && !coverFailed;

  return (
    <footer className="border-t-2 border-ink bg-card px-4 py-3">
      <div className="flex items-center justify-between gap-5">
        <div
          className="flex min-w-0 items-center gap-4"
          onContextMenu={(event) => {
            if (track) showContextMenu(event, buildCurrentTrackMenuEntries(track));
          }}
        >
          <button
            ref={triggerRef}
            type="button"
            className={cn("reel shrink-0 cursor-pointer focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-stamp disabled:cursor-default", showCover && "has-cover", isPlaying && "spinning")}
            onClick={openImmersive}
            onKeyDown={(event) => { if (event.code === "Space") event.stopPropagation(); }}
            disabled={!track}
            aria-label="打开沉浸播放"
            title="沉浸播放 · 封面、歌词与声学分析"
          >
            {showCover ? (
              <img
                src={cover}
                alt=""
                draggable={false}
                onError={() => setCoverFailed(true)}
                className="h-full w-full rounded-full object-cover"
              />
            ) : null}
          </button>
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

