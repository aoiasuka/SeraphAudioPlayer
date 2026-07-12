import { useEffect, useState } from "react";
import { DeviceMenu } from "@/components/player/DeviceMenu";
import { PlaybackControls } from "@/components/player/PlaybackControls";
import { VolumeControl } from "@/components/player/VolumeControl";
import { WaveformProgress } from "@/components/player/WaveformProgress";
import { coverSrc } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { usePlayerStore } from "@/store/player";

export function MiniPlayer() {
  const track = usePlayerStore((s) => s.currentTrack());
  const isPlaying = usePlayerStore((s) => s.isPlaying);
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
        <div className="flex min-w-0 items-center gap-4">
          <div className={cn("reel", showCover && "has-cover", isPlaying && "spinning")}>
            {showCover ? (
              <img
                src={cover}
                alt=""
                draggable={false}
                onError={() => setCoverFailed(true)}
                className="h-full w-full rounded-full object-cover"
              />
            ) : null}
          </div>
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

