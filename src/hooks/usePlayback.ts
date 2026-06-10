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
        // M-10: 事件 trackId 与当前曲目不一致时，先把进度复位到 0，
        // 避免上一首的尾段 progress 与新曲目的 PlaybackStarted 之间 UI 卡在错误时间。
        if (eventTrackId && track?.id && eventTrackId !== track.id) {
          return state.currentTime > 0 ? { currentTime: 0 } : {};
        }
        // M-7：仅在已知时长(>0)时才钳制，否则透传后端进度，
        // 避免 duration 探测失败(=0)的曲目进度永远停在 0:00。
        const duration = track?.duration;
        const clamped =
          duration && duration > 0 ? Math.min(seconds, duration) : seconds;
        return { currentTime: Math.max(0, clamped) };
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
