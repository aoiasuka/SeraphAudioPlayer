import { lazy, Suspense, useEffect, useRef, useState, type KeyboardEvent } from "react";
import { ChevronDown, Disc3 } from "lucide-react";
import { coverSrc } from "@/lib/tauri";
import { isStreamingTrack } from "@/components/pages/main-pages/trackFilters";
import { useImmersiveStore, type ImmersiveMode } from "@/store/immersive";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";
import { ImmersiveLyrics } from "./ImmersiveLyrics";
import { ImmersiveTransport } from "./ImmersiveTransport";
import "./immersive.css";

const LazyImmersiveAnalysis = lazy(() => import("./ImmersiveAnalysis").then((module) => ({ default: module.ImmersiveAnalysis })));
const LazyImmersiveQueue = lazy(() => import("./ImmersiveQueue").then((module) => ({ default: module.ImmersiveQueue })));

function RecordPosition() {
  const index = usePlayerStore((s) => s.currentTrackIndex);
  const count = usePlayerStore((s) => s.playlist.length);
  return <span>{String(index + 1).padStart(3, "0")} / {String(count).padStart(3, "0")}</span>;
}

function PlaybackStatus() {
  const playing = usePlayerStore((s) => s.isPlaying);
  return <span className="immersive-status"><i aria-hidden="true" />{playing ? "播放中" : "已暂停"}</span>;
}

function RecordCover({ track }: { track: Track }) {
  const src = coverSrc(track.cover);
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [src]);
  return (
    <div className="immersive-art">
      {src && !failed ? (
        <img src={src} alt={`${track.album || track.title} · 专辑封面`} draggable={false} onError={() => setFailed(true)} />
      ) : (
        <div className="immersive-art-fallback" role="img" aria-label="暂无专辑封面">
          <div className="immersive-vinyl"><Disc3 size={44} strokeWidth={1} /></div>
          <span>SERAPH / AUDIO ARCHIVE</span>
        </div>
      )}
    </div>
  );
}

export function ImmersivePlayer() {
  const track = usePlayerStore((s) => s.currentTrack());
  const mode = useImmersiveStore((s) => s.mode);
  const setMode = useImmersiveStore((s) => s.setMode);
  const close = useImmersiveStore((s) => s.close);
  const closeRef = useRef<HTMLButtonElement>(null);
  const queueTriggerRef = useRef<HTMLButtonElement>(null);
  const [queueOpen, setQueueOpen] = useState(false);
  // 译文开关与设置页共用 store 字段（持久化），不再是沉浸页私有状态
  const showTranslation = usePlayerStore((s) => s.showLyricsTranslation);
  const setShowLyricsTranslation = usePlayerStore((s) => s.setShowLyricsTranslation);
  const toggleTranslation = () => setShowLyricsTranslation(!showTranslation);
  const [largeLyrics, setLargeLyrics] = useState(false);

  useEffect(() => {
    closeRef.current?.focus({ preventScroll: true });
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // 队列、设置与右键菜单先处理自己的 Esc，不连带收起沉浸播放。
      if (document.querySelector('[role="dialog"], [role="menu"]')) return;
      event.preventDefault();
      close();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [close]);

  const switchTab = (event: KeyboardEvent<HTMLButtonElement>) => {
    let next: ImmersiveMode;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") next = mode === "lyrics" ? "analysis" : "lyrics";
    else if (event.key === "Home") next = "lyrics";
    else if (event.key === "End") next = "analysis";
    else return;
    event.preventDefault();
    setMode(next);
    document.getElementById(`immersive-tab-${next}`)?.focus();
  };

  if (!track) return null;

  return (
    <section
      className="immersive-player"
      aria-label="沉浸播放"
      onKeyDown={(event) => {
        // 新视图的按钮保留原生空格激活；空白区域仍使用全局播放快捷键。
        if (event.code === "Space" && event.target instanceof Element && event.target.closest("button")) event.stopPropagation();
      }}
    >
      <header className="immersive-header">
        <div className="immersive-header-left">
          <button ref={closeRef} className="immersive-collapse" onClick={close} aria-label="收起沉浸播放">
            <ChevronDown size={16} strokeWidth={1.4} /><span>收起</span><kbd>ESC</kbd>
          </button>
          <span className="immersive-header-caption">NOW PLAYING <em>—</em> 沉浸播放</span>
        </div>
        <div className="immersive-header-right">
          <div className="immersive-tabs" role="tablist" aria-label="沉浸播放模式">
            {([ ["lyrics", "封面 · 歌词"], ["analysis", "声学分析"] ] as const).map(([value, label]) => (
              <button key={value} id={`immersive-tab-${value}`} role="tab" aria-selected={mode === value} aria-controls={`immersive-panel-${value}`} tabIndex={mode === value ? 0 : -1} onKeyDown={switchTab} onClick={() => setMode(value)}>{label}</button>
            ))}
          </div>
          <PlaybackStatus />
        </div>
      </header>

      <div className={`immersive-body immersive-body-${mode}`}>
        <aside className="immersive-record" aria-label="当前曲目">
          {mode === "analysis" && <div className="immersive-record-heading"><span>NOW LISTENING</span><RecordPosition /></div>}
          <RecordCover key={track.id} track={track} />
          {mode === "lyrics" && <div className="immersive-record-heading"><span className="immersive-source">{isStreamingTrack(track) ? "STREAMING RECORD" : "LOCAL RECORD"}</span><RecordPosition /></div>}
          <div className="immersive-record-meta">
            <h1 title={track.title}>{track.title}</h1>
            <div className="immersive-record-details">
              <p title={track.artist}>{track.artist || "未知艺术家"}</p>
              <span className="immersive-meta-separator" aria-hidden="true">/</span>
              <p title={track.album}>{track.album || "未知专辑"}</p>
            </div>
          </div>
          {mode === "analysis" && <ImmersiveLyrics compact track={track} showTranslation={showTranslation} largeLyrics={largeLyrics} onToggleTranslation={toggleTranslation} onToggleSize={() => setLargeLyrics((value) => !value)} />}
        </aside>

        <div className="immersive-mode-panel" role="tabpanel" id={`immersive-panel-${mode}`} aria-labelledby={`immersive-tab-${mode}`}>
          {mode === "lyrics" ? (
            <ImmersiveLyrics track={track} showTranslation={showTranslation} largeLyrics={largeLyrics} onToggleTranslation={toggleTranslation} onToggleSize={() => setLargeLyrics((value) => !value)} />
          ) : (
            <Suspense fallback={<div className="immersive-empty" role="status">正在打开声学分析…</div>}>
              <LazyImmersiveAnalysis key={track.id} />
            </Suspense>
          )}
        </div>
      </div>

      <ImmersiveTransport track={track} queueOpen={queueOpen} queueTriggerRef={queueTriggerRef} onOpenQueue={() => setQueueOpen(true)} />
      {queueOpen && <Suspense fallback={null}><LazyImmersiveQueue onClose={() => { setQueueOpen(false); queueTriggerRef.current?.focus(); }} /></Suspense>}
    </section>
  );
}
