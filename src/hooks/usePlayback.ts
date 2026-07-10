import { useCallback, useEffect } from "react";
import { isTauriRuntime } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";
import { usePlayerEvents } from "@/hooks/usePlayerEvents";
import { resetNextIndexCache, seekGuard, withRecentTrack } from "@/store/player/playbackActions";
import { syncPlaybackQueue } from "@/store/player/queueSync";

/**
 * 每秒推进播放进度（mock 时间轴）。
 * 真正接通音频后，进度由 Rust 侧 `Progress` 事件驱动，此 hook 即可移除。
 */
export function usePlayback() {
  const tick = usePlayerStore((s) => s.tick);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const playlist = usePlayerStore((s) => s.playlist);
  const currentTrackIndex = usePlayerStore((s) => s.currentTrackIndex);
  const recentTrackIds = usePlayerStore((s) => s.recentTrackIds);
  const shuffleMode = usePlayerStore((s) => s.shuffleMode);
  const loopMode = usePlayerStore((s) => s.loopMode);
  const handleBackendEvent = useCallback((event: { type: string; [key: string]: unknown }) => {
    if (event.type === "progress") {
      const seconds = typeof event.seconds === "number" ? event.seconds : 0;
      // 发现7：seek 后的抑制窗口内，忽略仍携带旧位置的在途 Progress 事件，避免进度条回跳
      if (Date.now() < seekGuard.until && Math.abs(seconds - seekGuard.target) > 1.5) {
        return;
      }
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

    if (event.type === "track_changed") {
      const trackId =
        typeof event.track_id === "string"
          ? event.track_id
          : typeof event.trackId === "string"
            ? event.trackId
            : undefined;
      if (!trackId) return;

      usePlayerStore.setState((state) => {
        const index = state.playlist.findIndex((track) => track.id === trackId);
        if (index < 0) return {};
        resetNextIndexCache();
        return {
          currentTrackIndex: index,
          currentTime: 0,
          recentTrackIds: withRecentTrack(state.recentTrackIds, trackId),
        };
      });
      return;
    }

    if (event.type === "playback_paused" || event.type === "playback_stopped") {
      usePlayerStore.setState({ isPlaying: false });
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

  useEffect(() => {
    if (!isTauriRuntime()) return;
    void syncPlaybackQueue(usePlayerStore.getState).catch((err) => {
      // eslint-disable-next-line no-console
      console.warn("Failed to sync playback queue", err);
    });
  }, [playlist, currentTrackIndex, recentTrackIds, shuffleMode, loopMode]);
}
