import { useCallback, useEffect } from "react";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import type { Track } from "@/types/track";
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
  const currentTrack = playlist[currentTrackIndex];

  useEffect(() => {
    if (!isTauriRuntime() || !currentTrack || currentTrack.lyricsLoaded !== false) return;
    let disposed = false;
    void invoke<Track | null>("get_track_info", { trackId: currentTrack.id }).then((details) => {
      if (disposed || !details || details.id !== currentTrack.id) return;
      usePlayerStore.setState((state) => {
        // 同一曲目也可能已替换歌词；只合并本次请求看到的不可变记录。
        if (state.currentTrack() !== currentTrack) return {};
        return { playlist: state.playlist.map((track) => track === currentTrack
          ? { ...track, lyrics: details.lyrics, lyricsLoaded: true } : track) };
      });
    }).catch((error) => { if (!disposed) console.warn("读取当前曲目歌词失败", error); });
    return () => { disposed = true; };
  }, [currentTrack]);
  const handleBackendEvent = useCallback((event: { type: string; [key: string]: unknown }) => {
    if (event.type === "progress") {
      // 审2-R12：NaN/Infinity 的进度事件直接丢弃，避免污染 currentTime 后 UI 显示 "NaN:NaN"
      if (typeof event.seconds === "number" && !Number.isFinite(event.seconds)) {
        return;
      }
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
        // 切歌后的旧 Progress 必须丢弃，不能把新曲目的进度和歌词重置到开头。
        if (eventTrackId && track?.id && eventTrackId !== track.id) {
          return {};
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

    if (event.type === "playback_resumed") {
      usePlayerStore.setState({ isPlaying: true });
      return;
    }

    if (event.type === "track_changed" || event.type === "playback_started") {
      // 审2-R6：后端切歌后，上一次 seek 的抑制窗口随之失效，
      // 否则会误吞新曲目开头（与旧 seek 目标差距大）的 Progress 事件。
      seekGuard.until = 0;
      const trackId =
        typeof event.track_id === "string"
          ? event.track_id
          : typeof event.trackId === "string"
            ? event.trackId
            : undefined;
      usePlayerStore.setState((state) => {
        const index = state.playlist.findIndex((track) => track.id === trackId);
        const playing = event.type === "playback_started" ? { isPlaying: true } : {};
        if (index < 0 || !trackId) return playing;
        // PlaybackStarted 也带曲目身份：即使遗漏 TrackChanged，歌词仍能跟随起播。
        if (event.type === "playback_started" && index === state.currentTrackIndex) {
          return playing;
        }
        resetNextIndexCache();
        return {
          ...playing,
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

    if (event.type === "seek_failed") {
      const state = usePlayerStore.getState();
      const trackId = event.track_id ?? event.trackId;
      if (trackId !== state.currentTrack()?.id) return;
      seekGuard.until = 0;
      if (typeof event.seconds === "number" && Number.isFinite(event.seconds)) {
        usePlayerStore.setState({ currentTime: Math.max(0, event.seconds) });
      }
      state.showNotification(typeof event.message === "string" ? event.message : "无法跳转到指定位置");
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
    void syncPlaybackQueue(usePlayerStore.getState, usePlayerStore.setState).catch((err) => {
      // eslint-disable-next-line no-console
      console.warn("Failed to sync playback queue", err);
    });
  }, [playlist, currentTrackIndex, recentTrackIds, shuffleMode, loopMode]);
}
