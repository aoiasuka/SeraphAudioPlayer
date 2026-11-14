import { MoreHorizontal } from "lucide-react";
import { useEffect, useState } from "react";
import { coverSrc } from "@/lib/tauri";
import { buildUpNextMenuEntries } from "@/lib/trackMenu";
import { showContextMenu } from "@/store/contextMenu";
import { usePlayerStore } from "@/store/player";

export function UpNextCard() {
  const playNextPreview = usePlayerStore((s) => s.playNextPreview);
  const next = usePlayerStore((s) => s.nextTrackPreview());
  const [coverFailed, setCoverFailed] = useState(false);
  const cover = coverSrc(next?.cover);

  useEffect(() => {
    setCoverFailed(false);
  }, [cover]);

  if (!next) return null;

  return (
    <div className="space-y-2">
      <h3 className="font-tw text-[10px] font-bold text-ink3 tracking-[3px] uppercase">
        UP NEXT — 下一首
      </h3>
      <div
        onContextMenu={(event) =>
          showContextMenu(event, buildUpNextMenuEntries(next))
        }
        className="archive-card flex items-center justify-between gap-2.5 p-2.5"
      >
        {/* 切到下一首是一个真正的按钮：此前整张卡片是带 onClick 的 div，键盘够不着 */}
        <button
          type="button"
          onClick={playNextPreview}
          // 空格是全局的播放 / 暂停快捷键；在这个按钮上按空格应当只触发按钮本身
          onKeyDown={(event) => { if (event.code === "Space") event.stopPropagation(); }}
          aria-label={`播放下一首：${next.title}，${next.artist}`}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 text-left"
        >
          {cover && !coverFailed ? (
            <img
              src={cover}
              alt=""
              draggable={false}
              onError={() => setCoverFailed(true)}
              className="h-9 w-9 shrink-0 border border-ink/20 object-cover"
            />
          ) : null}
          <span className="block min-w-0">
            <span className="block font-serif text-xs font-semibold text-ink line-clamp-1">
              {next.title}
            </span>
            <span className="mt-0.5 block truncate font-tw text-[10px] text-ink2">{next.artist}</span>
          </span>
        </button>
        {/* v0.4.3：「更多」按钮左键打开与右键相同的菜单 */}
        <button
          type="button"
          className="w-6 h-6 flex shrink-0 items-center justify-center text-ink3 hover:text-ink transition-colors"
          onClick={(e) => showContextMenu(e, buildUpNextMenuEntries(next))}
          aria-label="更多"
        >
          <MoreHorizontal className="w-3 h-3" />
        </button>
      </div>
    </div>
  );
}
