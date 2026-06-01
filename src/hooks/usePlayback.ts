import { useCallback, useEffect } from "react";
import { isTauriRuntime } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { usePlayerEvents } from "@/hooks/usePlayerEvents";

/**
 * 每秒推进播放进度（mock 时间轴）。
 * 真正接通音频后，进度由 Rust 侧 `Progress` 事件驱动，此 hook 即可移除。
 */
export function usePlayback() {
  const tick = usePlayerStore((s) => s.tick);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const handleBackendEvent = useCallback((event: { type: string; [key: string]: unknown }) => {
    if (event.type === "progress") {
      const seconds = typeof event.seconds === "number" ? event.seconds : 0;
      usePlayerStore.setState((state) => {
        const track = state.playlist[state.currentTrackIndex];
        const eventTrackId =
          typeof event.track_id === "string"
            ? event.track_id
            : typeof event.trackId === "string"
              ? event.trackId
              : undefined;
        if (eventTrackId && track?.id && eventTrackId !== track.id) {
          return {};
        }
        const duration = track?.duration ?? Number.POSITIVE_INFINITY;
        return { currentTime: Math.max(0, Math.min(seconds, duration)) };
      });
      return;
    }

    if (event.type === "playback_started" || event.type === "playback_resumed") {
      usePlayerStore.setState({ isPlaying: true });
      return;
    }

    if (event.type === "playback_paused" || event.type === "playback_stopped") {
      usePlayerStore.setState({ isPlaying: false });
      return;
    }

    if (event.type === "playback_ended") {
      const state = usePlayerStore.getState();
      const current = state.currentTrack();
      const endedTrackId =
        typeof event.track_id === "string" ? event.track_id : current?.id;
      if (current && endedTrackId && current.id !== endedTrackId) return;

      if (state.loopMode) {
        state.loadTrack(state.currentTrackIndex);
        return;
      }

      if (state.playlist.length > 1) {
        state.nextTrack();
        return;
      }

      usePlayerStore.setState({
        isPlaying: false,
        currentTime: current?.duration ?? 0,
      });
      return;
    }

    if (event.type === "error") {
      const message =
        typeof event.message === "string" ? event.message : "音频播放失败";
      const state = usePlayerStore.getState();
      state.showNotification(message);
      usePlayerStore.setState({ isPlaying: false });
    }
  }, []);

  usePlayerEvents(handleBackendEvent);

  useEffect(() => {
    if (isTauriRuntime()) return;
    if (!isPlaying) return;
    const id = window.setInterval(() => tick(), 1000);
    return () => window.clearInterval(id);
  }, [isPlaying, tick]);
}
