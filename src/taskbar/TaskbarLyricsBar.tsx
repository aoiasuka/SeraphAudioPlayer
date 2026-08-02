import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Home, Pause, Play, SkipBack, SkipForward, X } from "lucide-react";
import { TypewriterText } from "@/components/ui/TypewriterText";
import {
  activeGroupIndex,
  groupLyricsByTime,
} from "@/lib/lyrics/activeLine";
import {
  coverSrc,
  emitEvent,
  FRONTEND_EVENT,
  invoke,
  listen,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { LyricLine } from "@/types/track";

/** 本窗口 label,窗口事件(tauri://move)据此收窄,免收主窗口的同名事件。 */
const WINDOW_LABEL = "taskbar-lyrics";
/** v0.5.4 遗留:曾把拖拽位置以物理像素存在这里,现已迁到主窗口 store。 */
const LEGACY_POSITION_KEY = "seraph-taskbar-bar-x";
/** ✕ 按钮 → 主窗口关闭开关(设置持久化在主窗口 store,单一事实来源)。 */
const CLOSE_EVENT = "seraph://taskbar-lyrics-close";
/** 拖拽结束 → 把反解出的位置比例回传主窗口落进设置(设置页滑块随之同步)。 */
const POSITION_EVENT = "seraph://taskbar-lyrics-position";
/** 后端写完歌词缓存的广播,payload 为 trackId。 */
const LYRICS_UPDATED_EVENT = "seraph://track-lyrics-updated";

interface PlaybackSnapshot {
  trackId: string | null;
  playing: boolean;
  seconds: number;
  total: number;
  /** 任务栏是否深色主题（据此选墨签/纸签配色） */
  darkTaskbar: boolean;
}

/** get_track_info 返回的字段子集(后端 ImportedTrack,camelCase)。 */
interface BarTrack {
  id: string;
  title: string;
  artist: string;
  cover: string;
  duration: number;
  lyrics: LyricLine[];
}

interface PlayerEventPayload {
  type: string;
  track_id?: string;
  seconds?: number;
  total?: number;
}

/**
 * 一次性迁移 v0.5.4 的像素位置:把旧值喂给后端反解成比例回传主窗口,
 * 随即删掉 key。此后位置的单一事实来源只有主窗口 store。
 */
function takeLegacyStoredX(): number | null {
  const raw = window.localStorage.getItem(LEGACY_POSITION_KEY);
  if (raw === null) return null;
  window.localStorage.removeItem(LEGACY_POSITION_KEY);
  const value = Number(raw);
  return Number.isFinite(value) ? value : null;
}

export function TaskbarLyricsBar() {
  const [trackId, setTrackId] = useState<string | null>(null);
  const [track, setTrack] = useState<BarTrack | null>(null);
  const [playing, setPlaying] = useState(false);
  const [seconds, setSeconds] = useState(0);
  const [total, setTotal] = useState(0);
  const [hovered, setHovered] = useState(false);
  // 墨签配色：深色任务栏 → 墨底纸字（快照初始化 + 主题切换事件热更新）
  const [dark, setDark] = useState(false);
  const trackIdRef = useRef<string | null>(null);
  trackIdRef.current = trackId;
  /** 曲目元数据重拉的触发计数:歌词导入后 trackId 不变也要重拉 */
  const [trackRevision, setTrackRevision] = useState(0);
  const moveDebounceRef = useRef<number | null>(null);
  /** 最近一次上报的窗口左上角,用来吃掉后端吸附回写触发的 move 回声 */
  const lastReportedRef = useRef<string | null>(null);

  // 启动:迁移遗留位置 → 拉取播放快照(位置本身由主窗口在建窗前同步给后端)
  useEffect(() => {
    let cancelled = false;

    // 只传 x:旧值本就只有横向语义,竖排任务栏下后端取不到纵向坐标会退回
    // 默认落点,正是想要的结果。
    const legacyX = takeLegacyStoredX();
    if (legacyX !== null) {
      void invoke<number>("position_taskbar_bar", { x: legacyX })
        .then((ratio) => void emitEvent(POSITION_EVENT, ratio))
        .catch(() => undefined);
    }

    void invoke<PlaybackSnapshot>("get_playback_snapshot")
      .then((snapshot) => {
        if (cancelled || !snapshot) return;
        setTrackId(snapshot.trackId);
        setPlaying(snapshot.playing);
        setSeconds(snapshot.seconds);
        setTotal(snapshot.total);
        setDark(snapshot.darkTaskbar === true);
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
    };
  }, []);

  // 播放事件流(后端 app.emit 广播到所有窗口)
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<PlayerEventPayload>(FRONTEND_EVENT, (event) => {
      if (disposed) return;
      switch (event.type) {
        case "playback_started":
          setTrackId(event.track_id ?? null);
          setPlaying(true);
          setSeconds(0);
          setTotal(0);
          break;
        case "track_changed":
          setTrackId(event.track_id ?? null);
          setSeconds(0);
          setTotal(0);
          break;
        case "playback_resumed":
          setPlaying(true);
          break;
        case "playback_paused":
          setPlaying(false);
          break;
        case "playback_stopped":
          setPlaying(false);
          setSeconds(0);
          break;
        case "progress":
          setSeconds(event.seconds ?? 0);
          setTotal(event.total ?? 0);
          // 自愈:错过 TrackChanged(如窗口刚创建)时按 Progress 归位
          if (event.track_id && event.track_id !== trackIdRef.current) {
            setTrackId(event.track_id);
          }
          break;
        default:
          break;
      }
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 任务栏主题切换事件（thumbbar 的 WM_SETTINGCHANGE 侦测后广播）
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<boolean>("seraph://taskbar-theme", (isDark) => {
      if (!disposed) setDark(isDark === true);
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 曲目元数据(标题/封面/歌词)按 trackId 拉取;trackRevision 让「同一首歌
  // 刚导入歌词」也能重拉——否则原本没歌词的曲目要切歌回来才看得到新歌词。
  useEffect(() => {
    if (!trackId) {
      setTrack(null);
      return;
    }
    let cancelled = false;
    void invoke<BarTrack | null>("get_track_info", { trackId })
      .then((info) => {
        if (!cancelled) setTrack(info ?? null);
      })
      .catch(() => {
        if (!cancelled) setTrack(null);
      });
    return () => {
      cancelled = true;
    };
  }, [trackId, trackRevision]);

  // 歌词写盘广播(本地导入 / 在线歌词都会发):命中当前曲目就重拉元数据
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<string>(LYRICS_UPDATED_EVENT, (updatedTrackId) => {
      if (disposed) return;
      if (updatedTrackId && updatedTrackId !== trackIdRef.current) return;
      setTrackRevision((value) => value + 1);
    })
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  // 拖拽结束(移动事件去抖)→ 后端钳制吸附并反解成比例 → 回传主窗口落设置。
  //
  // ⚠️ 必须把监听收窄到本窗口:Tauri v2 的全局 listen 目标是 Any,会连
  // **主窗口**的 tauri://move 一起收(主窗口显示/移动给出屏幕中部坐标、
  // 最小化给出 -32000),歌词条会把那当成自己的位置跳过去。
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;

    void listen<{ x: number; y: number }>(
      "tauri://move",
      (position) => {
        if (disposed) return;
        if (moveDebounceRef.current !== null) {
          window.clearTimeout(moveDebounceRef.current);
        }
        moveDebounceRef.current = window.setTimeout(() => {
          moveDebounceRef.current = null;
          const x = Math.round(position.x);
          const y = Math.round(position.y);
          // 后端吸附回写会再发一次 move,坐标没变就别绕第二圈
          const key = `${x},${y}`;
          if (key === lastReportedRef.current) return;
          lastReportedRef.current = key;
          void invoke<number>("position_taskbar_bar", { x, y })
            .then((ratio) => emitEvent(POSITION_EVENT, ratio))
            .catch(() => undefined);
        }, 400);
      },
      WINDOW_LABEL
    )
      .then((fn) => {
        if (disposed) {
          fn();
          return;
        }
        unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      disposed = true;
      if (moveDebounceRef.current !== null) {
        window.clearTimeout(moveDebounceRef.current);
      }
      unlisten?.();
    };
  }, []);

  const lyricGroups = useMemo(
    () => groupLyricsByTime(track?.lyrics ?? []),
    [track]
  );
  const activeIdx = useMemo(
    () => activeGroupIndex(lyricGroups, seconds),
    [lyricGroups, seconds]
  );
  const activeLine =
    activeIdx >= 0 ? (lyricGroups[activeIdx]?.lines[0]?.text ?? "") : "";

  const effectiveTotal = total > 0 ? total : (track?.duration ?? 0);
  const progressRatio =
    effectiveTotal > 0 ? Math.min(1, seconds / effectiveTotal) : 0;
  const cover = coverSrc(track?.cover);

  const handleSeek = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      event.stopPropagation();
      if (effectiveTotal <= 0) return;
      const rect = event.currentTarget.getBoundingClientRect();
      if (rect.width <= 0) return;
      const ratio = Math.min(
        1,
        Math.max(0, (event.clientX - rect.left) / rect.width)
      );
      void invoke("seek", { seconds: ratio * effectiveTotal }).catch(
        () => undefined
      );
    },
    [effectiveTotal]
  );

  const controlButton = cn(
    "flex h-[22px] w-[22px] shrink-0 items-center justify-center border-[1.5px] transition-colors",
    dark
      ? "border-paper2 bg-ink text-paper hover:bg-paper hover:text-ink"
      : "border-ink bg-card text-ink hover:bg-ink hover:text-paper"
  );

  return (
    <div
      data-tauri-drag-region
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      title={
        track
          ? `${track.title}${track.artist ? ` · ${track.artist}` : ""}`
          : undefined
      }
      className={cn(
        "relative flex h-full w-full items-stretch overflow-hidden border-[1.5px]",
        dark ? "border-paper2/70 bg-ink" : "border-ink bg-card"
      )}
    >
      {/* 左缘印章红竖条:纸签的"档案标签"识别符 */}
      <div className="w-[3px] shrink-0 bg-stamp" />

      {/* 封面:播放中缓速旋转 */}
      <div
        data-tauri-drag-region
        className="flex shrink-0 items-center px-1.5"
      >
        <div
          className={cn(
            "h-[24px] w-[24px] overflow-hidden rounded-full border-[1.5px] bg-paper2",
            dark ? "border-paper2/70" : "border-ink",
            playing && "animate-spin-slow"
          )}
        >
          {cover ? (
            <img
              src={cover}
              alt=""
              draggable={false}
              className="h-full w-full object-cover"
            />
          ) : (
            <div className="flex h-full w-full items-center justify-center">
              <div className="h-[6px] w-[6px] rounded-full border-[1.5px] border-ink" />
            </div>
          )}
        </div>
      </div>

      {/* 歌词单行(实测反馈:不常驻曲名/歌手,信息密度让位可读性) */}
      <div
        data-tauri-drag-region
        className="flex min-w-0 flex-1 items-center pr-1.5"
      >
        <div
          data-tauri-drag-region
          className={cn(
            "w-full truncate font-serif text-[15px] font-semibold leading-tight",
            dark ? "text-paper" : "text-ink"
          )}
        >
          {activeLine ? (
            <TypewriterText text={activeLine} />
          ) : (
            <span className="font-tw text-[10px] font-medium text-ink3">
              {track ? "— 暂无歌词稿 —" : "— 未在播放 —"}
            </span>
          )}
        </div>
      </div>

      {/* 悬停控制排:覆盖右侧,不改变窗口尺寸 */}
      {hovered && (
        <div
          className={cn(
            "absolute inset-y-0 right-0 flex items-center gap-1 border-l-[1.5px] pl-1.5 pr-1",
            dark ? "border-paper2/40 bg-ink" : "border-line bg-card"
          )}
        >
          <button
            type="button"
            className={controlButton}
            title="上一首"
            aria-label="上一首"
            onClick={() => void invoke("prev_track").catch(() => undefined)}
          >
            <SkipBack className="h-3 w-3" />
          </button>
          <button
            type="button"
            className={controlButton}
            title={playing ? "暂停" : "播放"}
            aria-label={playing ? "暂停" : "播放"}
            onClick={() => void invoke("toggle_play").catch(() => undefined)}
          >
            {playing ? (
              <Pause className="h-3 w-3" />
            ) : (
              <Play className="h-3 w-3" />
            )}
          </button>
          <button
            type="button"
            className={controlButton}
            title="下一首"
            aria-label="下一首"
            onClick={() => void invoke("next_track").catch(() => undefined)}
          >
            <SkipForward className="h-3 w-3" />
          </button>
          <button
            type="button"
            className={controlButton}
            title="打开主窗口"
            aria-label="打开主窗口"
            onClick={() =>
              void invoke("focus_main_window").catch(() => undefined)
            }
          >
            <Home className="h-3 w-3" />
          </button>
          <button
            type="button"
            className={cn(
              controlButton,
              "hover:border-stamp hover:bg-stamp hover:text-paper"
            )}
            title="关闭歌词条"
            aria-label="关闭歌词条"
            onClick={() => void emitEvent(CLOSE_EVENT).catch(() => undefined)}
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      )}

      {/* 底边进度线:悬停可点击定位 */}
      <div
        className={cn(
          "absolute inset-x-0 bottom-0 h-[2px]",
          dark ? "bg-paper2/25" : "bg-line/70",
          effectiveTotal > 0 && "cursor-pointer"
        )}
        onClick={handleSeek}
      >
        <div
          className="h-full bg-stamp transition-[width] duration-500 ease-linear"
          style={{ width: `${progressRatio * 100}%` }}
        />
      </div>
    </div>
  );
}
