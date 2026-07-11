import * as QRCode from "qrcode";
import { invoke } from "@/lib/tauri";
import type {
  BilibiliFfmpegStatus,
  BilibiliLoginPollResult,
  BilibiliLoginQrCode,
  BilibiliLoginStatus,
  FfmpegDownloadProgress,
  FfmpegDownloadState,
  PlayerStore,
  PlayerStoreGet,
  PlayerStoreSet,
} from "./types";

// 审2-R5：StreamingPage 被 MainPages 的 key={activeView} 强制卸载，组件局部的
// 下载进度/扫码轮询切页即丢（下载可被重复触发、扫码后登录静默失败）。
// 状态提升到 store，生命周期归 store 管；轮询 interval 是不可序列化对象，
// 存 store 外的模块级变量，登录成功/二维码过期/手动关闭时自行清理。
let loginPollTimer: number | null = null;

function clearLoginPollTimer() {
  if (loginPollTimer !== null) {
    window.clearInterval(loginPollTimer);
    loginPollTimer = null;
  }
}

// 审2-R5：把后端下载进度事件规约成 store 状态（由 useStreamingEvents 在 App 级挂载一次）
export function ffmpegDownloadStateFromProgress(
  progress: FfmpegDownloadProgress
): FfmpegDownloadState {
  if (progress.stage === "done") {
    return { stage: "done", percent: 100 };
  }
  if (progress.stage === "error") {
    return { stage: "error", percent: 0 };
  }
  return {
    stage: "downloading",
    percent: progress.percent >= 0 ? progress.percent : 0,
    message: progress.message ?? undefined,
  };
}

function startLoginPollInterval(
  set: PlayerStoreSet,
  get: PlayerStoreGet,
  qrcodeKey: string
) {
  clearLoginPollTimer();
  // L-9: 拉长到 3.5s 降低 B 站风控风险；登录成功 / 二维码过期会立即停止
  loginPollTimer = window.setInterval(() => {
    // 审2-R5：轮询回调里通过 get() 拿最新状态；二维码已被关闭/更换时自行停止
    const current = get().loginQr;
    if (!current || current.qrcodeKey !== qrcodeKey) {
      clearLoginPollTimer();
      return;
    }
    void invoke<BilibiliLoginPollResult>("bilibili_poll_login", { qrcodeKey })
      .then((result) => {
        const latest = get().loginQr;
        if (!latest || latest.qrcodeKey !== qrcodeKey) return;
        if (result.loggedIn || result.code === 0) {
          clearLoginPollTimer();
          set({
            bilibiliLoginStatus: result.profile ?? { loggedIn: true },
            loginQr: null,
          });
          get().showNotification("B 站登录成功");
          return;
        }
        if (result.code === 86038) {
          clearLoginPollTimer();
          set({ loginQr: null });
          get().showNotification("B 站二维码已过期");
          return;
        }
        set({ loginQr: { ...latest, message: result.message } });
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: bilibili_poll_login", err);
        const latest = get().loginQr;
        if (!latest || latest.qrcodeKey !== qrcodeKey) return;
        set({ loginQr: { ...latest, message: "登录轮询失败" } });
      });
  }, 3500);
}

export function createStreamingActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<
  PlayerStore,
  | "refreshBilibiliState"
  | "startFfmpegDownload"
  | "startLoginPolling"
  | "stopLoginPolling"
  | "logoutBilibili"
> {
  return {
    refreshBilibiliState: async () => {
      try {
        const [status, ffmpeg] = await Promise.all([
          invoke<BilibiliLoginStatus>("bilibili_login_status"),
          invoke<BilibiliFfmpegStatus>("bilibili_ffmpeg_status"),
        ]);
        set({ bilibiliLoginStatus: status, bilibiliFfmpegStatus: ffmpeg });
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: bilibili status", err);
      }
    },

    startFfmpegDownload: async () => {
      // 审2-R5：幂等——已在下载中时直接忽略，切页回来重复点击不会触发第二次下载
      if (get().ffmpegDownload.stage === "downloading") return;
      set({
        ffmpegDownload: { stage: "downloading", percent: 0, message: "准备下载…" },
      });
      get().showNotification("开始下载 FFmpeg，请保持网络畅通…");
      try {
        const status = await invoke<BilibiliFfmpegStatus>("download_ffmpeg");
        set({
          bilibiliFfmpegStatus: status,
          ffmpegDownload: { stage: "done", percent: 100 },
        });
        get().showNotification(
          status.available
            ? "FFmpeg 安装完成，现在可解码杜比/EAC3 了"
            : "FFmpeg 安装未完成"
        );
      } catch (err) {
        set({ ffmpegDownload: { stage: "error", percent: 0 } });
        const reason = typeof err === "string" ? err : "下载失败";
        get().showNotification(reason);
        // eslint-disable-next-line no-console
        console.warn("download_ffmpeg failed", err);
      }
    },

    startLoginPolling: async () => {
      if (get().isLoginBusy) return;
      set({ isLoginBusy: true });
      try {
        const qrcode = await invoke<BilibiliLoginQrCode>("bilibili_login_qrcode");
        let dataUrl = "";
        try {
          dataUrl = await QRCode.toDataURL(qrcode.url, {
            width: 184,
            margin: 1,
            color: { dark: "#0f172a", light: "#ffffff" },
          });
        } catch (qrErr) {
          // 审2-R5（顺带修 L-6）：二维码渲染失败不再静默——置错误态并提示，不启动轮询
          // eslint-disable-next-line no-console
          console.warn("Failed to render bilibili login qrcode", qrErr);
          get().showNotification("二维码生成失败");
          set({
            loginQr: {
              qrcodeKey: qrcode.qrcodeKey,
              url: qrcode.url,
              dataUrl: "",
              message: "二维码生成失败",
            },
          });
          return;
        }
        set({
          loginQr: {
            qrcodeKey: qrcode.qrcodeKey,
            url: qrcode.url,
            dataUrl,
            message: "等待扫码",
          },
        });
        startLoginPollInterval(set, get, qrcode.qrcodeKey);
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: bilibili_login_qrcode", err);
        get().showNotification("无法生成 B 站登录二维码");
      } finally {
        set({ isLoginBusy: false });
      }
    },

    stopLoginPolling: () => {
      clearLoginPollTimer();
      if (get().loginQr) set({ loginQr: null });
    },

    logoutBilibili: async () => {
      try {
        await invoke("bilibili_logout");
        set({ bilibiliLoginStatus: { loggedIn: false } });
        get().showNotification("已退出 B 站登录");
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: bilibili_logout", err);
        get().showNotification("退出 B 站登录失败");
      }
    },
  };
}
