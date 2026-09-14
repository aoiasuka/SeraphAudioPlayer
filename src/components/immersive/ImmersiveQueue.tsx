import { useDeferredValue, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Music2, Search, X } from "lucide-react";
import { Dialog } from "@/components/ui/dialog";
import { formatSeconds } from "@/lib/format";
import { usePlayerStore } from "@/store/player";

const ROW_HEIGHT = 58;

export function ImmersiveQueue({ onClose }: { onClose: () => void }) {
  const playlist = usePlayerStore((s) => s.playlist);
  const currentId = usePlayerStore((s) => s.currentTrack()?.id);
  const loadTrack = usePlayerStore((s) => s.loadTrack);
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const [scrollTop, setScrollTop] = useState(0);
  const [height, setHeight] = useState(348);
  const containerRef = useRef<HTMLDivElement>(null);
  const initialId = useRef(currentId);
  const initialPositioned = useRef(false);
  const entries = useMemo(() => {
    const term = deferredQuery.trim().toLocaleLowerCase();
    return playlist.map((track, index) => ({ track, index })).filter(({ track }) => !term || `${track.title}\n${track.artist}\n${track.album}`.toLocaleLowerCase().includes(term));
  }, [playlist, deferredQuery]);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const resize = () => setHeight(element.clientHeight);
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useLayoutEffect(() => {
    const element = containerRef.current;
    if (!element) return;
    const index = initialPositioned.current ? 0 : entries.findIndex(({ track }) => track.id === initialId.current);
    initialPositioned.current = true;
    const top = Math.max(0, index * ROW_HEIGHT - element.clientHeight / 2 + ROW_HEIGHT / 2);
    element.scrollTo({ top });
    setScrollTop(top);
  }, [deferredQuery]);

  const clampedTop = Math.min(scrollTop, Math.max(0, entries.length * ROW_HEIGHT - height));
  const start = Math.max(0, Math.floor(clampedTop / ROW_HEIGHT) - 4);
  const end = Math.min(entries.length, Math.ceil((clampedTop + height) / ROW_HEIGHT) + 4);

  return (
    <Dialog open onClose={onClose} className="immersive-queue-dialog">
      <div className="immersive-queue-heading"><h2>播放队列 <span>{playlist.length} RECORDS</span></h2><button className="immersive-icon" onClick={onClose} aria-label="关闭播放队列"><X size={18} /></button></div>
      <label className="immersive-queue-search"><Search size={15} /><input value={query} onChange={(event) => setQuery(event.currentTarget.value)} placeholder="检索曲目、艺术家或专辑" aria-label="检索播放队列" /></label>
      <div ref={containerRef} className="immersive-queue-list" onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}>
        {entries.length ? (
          <div style={{ paddingTop: start * ROW_HEIGHT, paddingBottom: (entries.length - end) * ROW_HEIGHT }}>
            {entries.slice(start, end).map(({ track, index }) => <button key={track.id} className="immersive-queue-row" aria-current={currentId === track.id ? "true" : undefined} onClick={() => { if (currentId !== track.id) loadTrack(index); }}>
              <span className="immersive-queue-index">{currentId === track.id ? <Music2 size={15} /> : String(index + 1).padStart(2, "0")}</span><span className="immersive-queue-title"><strong>{track.title}</strong><small>{track.artist} / {track.album}</small></span><time>{formatSeconds(track.duration)}</time>
            </button>)}
          </div>
        ) : <div className="immersive-empty">没有匹配的曲目</div>}
      </div>
    </Dialog>
  );
}
