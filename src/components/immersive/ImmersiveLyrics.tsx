import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Languages, Music2, Type } from "lucide-react";
import { activeGroupIndex, groupLyricsByTime } from "@/lib/lyrics/activeLine";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

function useLyricGroups(track: Track) {
  const groups = useMemo(() => groupLyricsByTime(track.lyrics), [track.lyrics]);
  // 只在当前句变化时重渲染，不把高频播放进度传播到整篇歌词。
  const activeIndex = usePlayerStore((s) => activeGroupIndex(groups, s.currentTime));
  return { groups, activeIndex };
}

interface LyricsProps {
  track: Track;
  showTranslation: boolean;
  largeLyrics: boolean;
  compact?: boolean;
  onToggleTranslation: () => void;
  onToggleSize: () => void;
}

export function ImmersiveLyrics({ track, showTranslation, largeLyrics, compact = false, onToggleTranslation, onToggleSize }: LyricsProps) {
  const { groups, activeIndex } = useLyricGroups(track);
  const seek = usePlayerStore((s) => s.seek);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const resumeTimer = useRef<ReturnType<typeof setTimeout>>();
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const centerPadding = viewport.height / 2;
  const [following, setFollowing] = useState(true);
  const lastContext = useRef<{ id: string; groups: typeof groups; viewport: typeof viewport }>();
  const hasTranslation = groups.some((group) => group.lines.length > 1);

  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    const resize = () => {
      const width = container.clientWidth;
      const height = container.clientHeight;
      setViewport((previous) => previous.width === width && previous.height === height ? previous : { width, height });
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(container);
    return () => observer.disconnect();
  }, [groups.length > 0]);

  useLayoutEffect(() => {
    const container = scrollRef.current;
    if (!container) return;
    const changed = lastContext.current?.id !== track.id || lastContext.current?.groups !== groups;
    const resized = lastContext.current?.viewport !== viewport;
    lastContext.current = { id: track.id, groups, viewport };
    if (changed) {
      clearTimeout(resumeTimer.current);
      setFollowing(true);
    } else if (!following) return;
    const line = lineRefs.current[Math.max(0, activeIndex)];
    const top = line ? Math.max(0, line.offsetTop - container.clientHeight / 2 + line.clientHeight / 2) : 0;
    const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    container.scrollTo({ top, behavior: changed || resized || reducedMotion ? "instant" : "smooth" });
  }, [activeIndex, viewport, track.id, groups, following, showTranslation, largeLyrics]);

  useEffect(() => () => clearTimeout(resumeTimer.current), []);

  const pauseFollowing = () => {
    setFollowing(false);
    clearTimeout(resumeTimer.current);
    resumeTimer.current = setTimeout(() => setFollowing(true), 3000);
  };
  const canSeek = Number.isFinite(track.duration) && track.duration > 0;

  return (
    <section className={`immersive-lyrics${compact ? " immersive-lyrics-compact" : ""}${largeLyrics ? " immersive-lyrics-large" : ""}`} aria-label="同步歌词">
      <header className="immersive-lyrics-heading">
        <span className="immersive-eyebrow">LYRICS / 歌词</span>
        <div className="immersive-lyric-tools">
          <button aria-label="显示译文" title={showTranslation ? "隐藏译文" : "显示译文"} aria-pressed={showTranslation} disabled={!hasTranslation} onClick={onToggleTranslation}><Languages size={13} /><span>译文</span></button>
          <button aria-label="放大歌词字号" title={largeLyrics ? "恢复歌词字号" : "放大歌词字号"} aria-pressed={largeLyrics} onClick={onToggleSize}><Type size={14} /><span>字号</span></button>
        </div>
      </header>
      {groups.length ? (
        <div ref={scrollRef} className="immersive-lyrics-scroll" tabIndex={0} aria-label="滚动歌词" onWheel={pauseFollowing} onPointerDown={pauseFollowing} onKeyDown={(event) => { if (["PageUp", "PageDown", "ArrowUp", "ArrowDown", "Home", "End", "Tab"].includes(event.key)) pauseFollowing(); }}>
          <div style={{ paddingBlock: centerPadding }}>
            {groups.map((group, index) => (
              <button
                key={`${group.time}-${index}`}
                ref={(element) => { lineRefs.current[index] = element; }}
                className={`immersive-lyric-line${index === activeIndex ? " is-current" : ""}${Math.abs(index - activeIndex) > 1 ? " is-distant" : ""}`}
                aria-current={index === activeIndex ? "true" : undefined}
                aria-label={`${formatSeconds(group.time)} · ${group.lines[0]?.text}`}
                disabled={!canSeek || group.time > track.duration}
                onClick={() => {
                  clearTimeout(resumeTimer.current);
                  setFollowing(true);
                  seek(Math.max(0, group.time));
                }}
              >
                <span>{group.lines[0]?.text}</span>
                {showTranslation && group.lines.slice(1).map((line, translationIndex) => <small key={translationIndex}>{line.text}</small>)}
              </button>
            ))}
          </div>
        </div>
      ) : (
        <div className="immersive-empty" role="status"><Music2 size={26} strokeWidth={1} /><p>{track.lyricsLoaded === false ? "正在读取歌词…" : "暂无歌词"}</p><small>{track.lyricsLoaded === false ? "即将与播放同步" : "此刻，听音乐。"}</small></div>
      )}
    </section>
  );
}
