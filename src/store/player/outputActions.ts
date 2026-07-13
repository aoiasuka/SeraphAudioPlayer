import { mockDevices } from "@/data/mock-playlist";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import type { OutputDevice, Track } from "@/types/track";
import { sendCommand, sendCommandAsync } from "./commands";
import { playbackErrorMessage } from "./playbackActions";
import { bumpPlayEpoch, currentPlayEpoch } from "./playEpoch";
import { syncPlaybackQueue } from "./queueSync";
import type { BackendDevice, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

async function applyOutputConfiguration(get: PlayerStoreGet, set: PlayerStoreSet) {
  const { currentDeviceId, devices, driverKind, volume, isMuted } =
    get();
  await sendCommandAsync("set_output_driver", { driver: driverKind });
  const selectedDevice =
    devices !== mockDevices
      ? findDeviceByCurrentId(devices, currentDeviceId)
      : undefined;
  if (selectedDevice) {
    if (selectedDevice.id !== currentDeviceId) {
      set({ currentDeviceId: selectedDevice.id });
    }
    await sendCommandAsync("select_output_device", {
      deviceId: selectedDevice.id,
    });
  }
  // M-4：每次播放前同步音量，避免重启后引擎停在默认 0.7 而 UI 显示其它值，
  // 造成「UI 显示 20% 实际 70%」的突然大音量。
  await sendCommandAsync("set_volume", { volume: isMuted ? 0 : volume });
}

export async function sendPlayCommand(
  track: Track,
  get: PlayerStoreGet,
  set: PlayerStoreSet,
  startSeconds = 0,
  isStillCurrent?: () => boolean
) {
  await syncPlaybackQueue(get);
  await applyOutputConfiguration(get, set);
  // 审2-R2：上面两个 await 期间用户可能已切歌/暂停（代际递增），
  // 发送 "play" 前复查播放意图是否仍然有效，过期则丢弃，避免旧续体顶掉新状态。
  if (isStillCurrent && !isStillCurrent()) return;
  await sendCommandAsync("play", {
    path: track.path,
    trackId: track.id,
    startSeconds,
  });
}

function normalizeDevice(device: BackendDevice): OutputDevice {
  return {
    id: device.id,
    name: device.name,
    isDefault: device.isDefault ?? device.is_default ?? false,
    legacyIds: device.legacyIds ?? device.legacy_ids ?? [],
  };
}

function findDeviceByCurrentId(devices: OutputDevice[], currentDeviceId: string) {
  const exact = devices.find(
    (device) =>
      device.id === currentDeviceId ||
      device.legacyIds?.includes(currentDeviceId)
  );
  if (exact) return exact;

  const legacySlug = legacyIndexDeviceSlug(currentDeviceId);
  if (!legacySlug) return undefined;

  const slugMatches = devices.filter((device) =>
    device.legacyIds?.some((id) => legacyIndexDeviceSlug(id) === legacySlug)
  );
  return slugMatches.length === 1 ? slugMatches[0] : undefined;
}

function legacyIndexDeviceSlug(deviceId: string) {
  const match = deviceId.match(/^cpal:\d+:(.+)$/);
  return match?.[1] || null;
}

export function createOutputActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "loadDevices" | "selectDevice" | "setDriver" | "setSmtcEnabled" | "toggleDeviceMenu" | "closeDeviceMenu"> {
  return {
  loadDevices: () => {
    void invoke<BackendDevice[]>("list_devices")
      .then(async (devices) => {
        if (!Array.isArray(devices) || devices.length === 0) return;
        const normalized = devices.map(normalizeDevice);
        const currentDeviceId = get().currentDeviceId;
        const currentDevice = findDeviceByCurrentId(normalized, currentDeviceId);
        const selectedDeviceId =
          currentDevice?.id ??
          normalized.find((device) => device.isDefault)?.id ??
          normalized[0].id;
        set({
          devices: normalized,
          currentDeviceId: selectedDeviceId,
        });
        await sendCommandAsync("set_output_driver", { driver: get().driverKind });
        await sendCommandAsync("select_output_device", { deviceId: selectedDeviceId });
      })
      .catch((err) => {
        // eslint-disable-next-line no-console
        console.warn("Tauri command failed: list_devices", err);
      });
  },

  selectDevice: (id) => {
    const { currentDeviceId, deviceMenuOpen } = get();
    if (currentDeviceId === id) {
      if (deviceMenuOpen) set({ deviceMenuOpen: false });
      return;
    }

    const device = get().devices.find((item) => item.id === id);
    sendCommand("select_output_device", { deviceId: id });
    set({ currentDeviceId: id, deviceMenuOpen: false });
    get().showNotification(`输出设备已切换到: ${device?.name ?? id}`);
  },

  setDriver: (k) => {
    if (k === "asio") {
      get().showNotification("ASIO 输出尚未开放，请先使用 WASAPI 独占或系统共享输出");
      return;
    }
    if (get().driverKind === k) return;
    // M-7: 切换 driver 前先停掉正在播的 session，避免后端 same-track 优化路径
    // 残留旧 driver 配置，导致用户切换后偶发音轨不切换。
    const wasPlaying = get().isPlaying;
    sendCommand("stop");
    sendCommand("set_output_driver", { driver: k });
    set({ driverKind: k, isPlaying: false, currentTime: 0 });

    // 若刚才在播，driver 切换后自动从头继续播放当前曲目，体验上无感
    if (wasPlaying) {
      const track = get().currentTrack();
      if (track) {
        // 审2-R2：为续播链申请新代际；期间用户切歌/暂停则放弃续播。
        const epoch = bumpPlayEpoch();
        const isStillCurrent = () =>
          epoch === currentPlayEpoch() && get().currentTrack()?.id === track.id;
        void sendPlayCommand(track, get, set, 0, isStillCurrent)
          .then(() => {
            // 审2-R2：Tauri 下 isPlaying 改由 playback_started 事件驱动（与发现15一致），
            // 删除乐观置位，避免后端实际起播失败时 UI 卡在播放态；stub 模式无事件，保留置位。
            if (!isTauriRuntime() && isStillCurrent()) set({ isPlaying: true });
          })
          .catch((err) => {
            // eslint-disable-next-line no-console
            console.warn("Failed to resume after driver switch", err);
            get().showNotification(playbackErrorMessage(err));
          });
      }
    }
  },

  toggleDeviceMenu: () => {
    const next = !get().deviceMenuOpen;
    set({ deviceMenuOpen: next });
    if (next) get().loadDevices();
  },

  closeDeviceMenu: () => {
    if (!get().deviceMenuOpen) return;
    set({ deviceMenuOpen: false });
  },

  setSmtcEnabled: (enabled) => {
    if (get().smtcEnabled === enabled) return;
    set({ smtcEnabled: enabled });
    sendCommand("set_smtc_enabled", { enabled });
    get().showNotification(
      enabled ? "已启用系统媒体控件" : "已停用系统媒体控件"
    );
  },
  };
}

