import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { CloudDownload, Languages, Music2, Type } from "lucide-react";
import { KaraokeLine } from "@/components/lyrics/KaraokeLine";
import { TypewriterText } from "@/components/ui/TypewriterText";
import { useSmoothTime } from "@/hooks/useSmoothTime";
import { activeVisibleIndex, hasWordTiming, isInIntermission, lyricsPosition, resolveVisibleGroups } from "@/lib/lyrics/activeLine";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";
import type { Track } from "@/types/track";

function useLyricGroups(track: Track) {
  // 排除规则由后端打 hidden 标记：全量分组定位、可见分组渲染，隐藏句区间不并入上一句
  const resolved = useMemo(() => resolveVisibleGroups(track.lyrics), [track.lyrics]);
  const groups = resolved.visible;
  // 只在当前句变化时重渲染，不把高频播放进度传播到整篇歌词。定位按可听位置（减输出延迟）。
  const activeIndex = usePlayerStore((s) => activeVisibleIndex(resolved, lyricsPosition(s.currentTime, s.outputLatency)));
  // 逐字来源带行结束时间：一句唱完且距下一句尚远时，当前句淡出（布尔选择器，只在翻转时重渲染）
  const intermission = usePlayerStore((s) => isInIntermission(resolved, activeIndex, lyricsPosition(s.currentTime, s.outputLatency)));
  // 原始歌词非空但全部被排除规则隐藏
  const allHiddenByRules = track.lyrics.length > 0 && groups.length === 0;
  return { groups, activeIndex, intermission, allHiddenByRules };
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
  const { groups, activeIndex, intermission, allHiddenByRules } = useLyricGroups(track);
  const seek = usePlayerStore((s) => s.seek);
  const showRoman = usePlayerStore((s) => s.showLyricsRoman);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const resumeTimer = useRef<ReturnType<typeof setTimeout>>();
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const centerPadding = viewport.height / 2;
  const [following, setFollowing] = useState(true);
  const lastContext = useRef<{ id: string; groups: typeof groups; viewport: typeof viewport }>();
  // 译文两种形态：LRC 类的相邻同时间戳行，或 TTML 的 translation 字段
  const hasTranslation = groups.some((group) => group.lines.length > 1 || !!group.lines[0]?.translation);
  const activeLine = activeIndex >= 0 ? groups[activeIndex]?.lines[0] : undefined;
  const activeHasWords = hasWordTiming(activeLine);
  const currentTime = usePlayerStore((s) => (activeHasWords ? lyricsPosition(s.currentTime, s.outputLatency) : 0));
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const smoothTime = useSmoothTime(currentTime, isPlaying, activeHasWords);

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
          <button aria-label="在线匹配歌词" title="在线匹配歌词" onClick={() => window.dispatchEvent(new CustomEvent("seraph:open-lyrics-search"))}><CloudDownload size={14} /><span>在线匹配</span></button>
          <button aria-label="显示译文" title={showTranslation ? "隐藏译文" : "显示译文"} aria-pressed={showTranslation} disabled={!hasTranslation} onClick={onToggleTranslation}><Languages size={13} /><span>译文</span></button>
          <button aria-label="放大歌词字号" title={largeLyrics ? "恢复歌词字号" : "放大歌词字号"} aria-pressed={largeLyrics} onClick={onToggleSize}><Type size={14} /><span>字号</span></button>
        </div>
      </header>
      {groups.length ? (
        <div ref={scrollRef} className="immersive-lyrics-scroll" tabIndex={0} aria-label="滚动歌词" onWheel={pauseFollowing} onPointerDown={pauseFollowing} onKeyDown={(event) => { if (["PageUp", "PageDown", "ArrowUp", "ArrowDown", "Home", "End", "Tab"].includes(event.key)) pauseFollowing(); }}>
          <div style={{ paddingBlock: centerPadding }}>
            {groups.map((group, index) => {
              const main = group.lines[0];
              const isCurrent = index === activeIndex;
              return (
                <button
                  key={`${track.id}-${group.time}-${index}`}
                  ref={(element) => { lineRefs.current[index] = element; }}
                  className={`immersive-lyric-line${isCurrent ? " is-current" : ""}${isCurrent && intermission ? " is-intermission" : ""}${Math.abs(index - activeIndex) > 1 ? " is-distant" : ""}`}
                  aria-current={isCurrent ? "true" : undefined}
                  aria-label={`${formatSeconds(group.time)} · ${main?.text}`}
                  disabled={!canSeek || group.time > track.duration}
                  onClick={() => {
                    clearTimeout(resumeTimer.current);
                    setFollowing(true);
                    seek(Math.max(0, group.time));
                  }}
                >
                  <span className="immersive-lyric-text">
                    {isCurrent ? (
                      hasWordTiming(main) ? (
                        <KaraokeLine
                          words={main.words}
                          currentTime={smoothTime}
                          sungColor="var(--stamp)"
                          unsungColor="rgba(181, 72, 42, 0.32)"
                        />
                      ) : (
                        <>
                          <span className="immersive-lyric-placeholder type-caret" aria-hidden="true">{main?.text}</span>
                          <span className="immersive-lyric-typing"><TypewriterText key={main?.text} text={main?.text ?? ""} /></span>
                        </>
                      )
                    ) : main?.text}
                  </span>
                  {showRoman && main?.roman ? <small className="immersive-lyric-roman">{main.roman}</small> : null}
                  {showTranslation && main?.translation ? <small>{main.translation}</small> : null}
                  {showTranslation && group.lines.slice(1).map((line, translationIndex) => <small key={translationIndex}>{line.text}</small>)}
                </button>
              );
            })}
          </div>
        </div>
      ) : (
        <div className="immersive-empty" role="status"><Music2 size={26} strokeWidth={1} /><p>{track.lyricsLoaded === false ? "正在读取歌词…" : allHiddenByRules ? "歌词已被排除规则全部隐藏" : "暂无歌词"}</p><small>{track.lyricsLoaded === false ? "即将与播放同步" : allHiddenByRules ? "可在设置 → 歌词设置里调整排除规则" : "此刻，听音乐。"}</small></div>
      )}
    </section>
  );
}
