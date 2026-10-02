import { mockDevices } from "@/data/mock-playlist";
import { invoke, isTauriRuntime, normalizeIpcError } from "@/lib/tauri";
import type { OutputDevice, Track } from "@/types/track";
import { sendCommand, sendCommandAsync } from "./commands";
import { playbackErrorMessage } from "./playbackActions";
import { bumpPlayEpoch, currentPlayEpoch } from "./playEpoch";
import { syncPlaybackQueue } from "./queueSync";
import type { BackendDevice, PlayerStore, PlayerStoreGet, PlayerStoreSet } from "./types";

async function applyOutputConfiguration(
  get: PlayerStoreGet,
  set: PlayerStoreSet,
  isStillCurrent: () => boolean
) {
  // 等待期间设备可能被用户改选。仅在一轮配置仍与最新选择一致时允许起播。
  while (isStillCurrent()) {
    const { devices, driverKind } = get();
    let { currentDeviceId } = get();
    const configurationUnchanged = () =>
      get().driverKind === driverKind && get().currentDeviceId === currentDeviceId;
    await sendCommandAsync("set_output_driver", { driver: driverKind });
    if (!isStillCurrent()) return false;
    if (!configurationUnchanged()) continue;
    const selectedDevice =
      devices !== mockDevices ? findDeviceByCurrentId(devices, currentDeviceId) : undefined;
    if (selectedDevice) {
      if (selectedDevice.id !== currentDeviceId) {
        currentDeviceId = selectedDevice.id;
        set({ currentDeviceId });
      }
      await sendCommandAsync("select_output_device", { deviceId: currentDeviceId });
      if (!isStillCurrent()) return false;
      if (!configurationUnchanged()) continue;
    }
    // 音量在下发前读取，不能恢复配置开始时的旧静音状态。
    const { volume, isMuted } = get();
    await sendCommandAsync("set_volume", { volume: isMuted ? 0 : volume });
    if (!isStillCurrent()) return false;
    if (configurationUnchanged()) return true;
  }
  return false;
}

export async function sendPlayCommand(
  track: Track,
  get: PlayerStoreGet,
  set: PlayerStoreSet,
  startSeconds: number | (() => number) = 0,
  isStillCurrent: () => boolean = () => true
) {
  if (!isStillCurrent()) return;
  await syncPlaybackQueue(get, set);
  if (!isStillCurrent()) return;
  if (!await applyOutputConfiguration(get, set, isStillCurrent)) return;
  // 审2-R2：上面两个 await 期间用户可能已切歌/暂停（代际递增），
  // 发送 "play" 前复查播放意图是否仍然有效，过期则丢弃，避免旧续体顶掉新状态。
  if (!isStillCurrent()) return;
  await sendCommandAsync("play", {
    path: track.path,
    trackId: track.id,
    startSeconds: typeof startSeconds === "function" ? startSeconds() : startSeconds,
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

type BackendToggleKey =
  | "smtcEnabled"
  | "taskbarButtonsEnabled"
  | "taskbarProgressEnabled"
  | "taskbarLyricsEnabled"
  | "taskbarLyricsClickThrough";

/** 同一设置串行提交；旧成功仍更新确认值，失败不能把乐观值当作回滚基线。 */
function createConfirmedQueue<T>() {
  let confirmed: T;
  const queue: Array<() => Promise<void>> = [];
  const drain = async () => {
    while (queue.length > 0) {
      await queue[0]();
      queue.shift();
    }
  };
  return (
    previous: T,
    value: T,
    send: () => Promise<void>,
    onSuccess: () => void | Promise<void>,
    onFailure: (err: unknown, confirmed: T) => void
  ) => {
    const idle = queue.length === 0;
    // 队列空闲时从 store 接续水合/外部刷新；在途期间绝不重新读取乐观状态作基线。
    if (idle) confirmed = previous;
    queue.push(async () => {
      try {
        await send();
      } catch (err) {
        onFailure(err, confirmed);
        return;
      }
      confirmed = value;
      await onSuccess();
    });
    if (idle) void drain();
  };
}

/** 后端确认后才提示，失败回到串行队列最后一次成功的值。 */
function commitBackendToggle(
  set: PlayerStoreSet,
  get: PlayerStoreGet,
  key: BackendToggleKey,
  value: boolean,
  command: string,
  args: Record<string, unknown>,
  successText: string,
  failureLabel: string,
  isStillCurrent: () => boolean,
  enqueue: ReturnType<typeof createConfirmedQueue<boolean>>
) {
  const previous = get()[key];
  set({ [key]: value } as Pick<PlayerStore, BackendToggleKey>);
  enqueue(previous, value, () => sendCommandAsync(command, args),
    () => {
      if (isStillCurrent() && get()[key] === value) get().showNotification(successText);
    },
    (err, confirmed) => {
      console.warn(`Tauri command failed: ${command}`, err);
      if (!isStillCurrent() || get()[key] !== value) return;
      set({ [key]: confirmed } as Pick<PlayerStore, BackendToggleKey>);
      get().showNotification(`${failureLabel}失败：${normalizeIpcError(err).message}`, "error");
    }
  );
}

export function createOutputActions(
  set: PlayerStoreSet,
  get: PlayerStoreGet
): Pick<PlayerStore, "loadDevices" | "selectDevice" | "setDriver" | "setSmtcEnabled" | "setRememberPlayback" | "setTaskbarButtonsEnabled" | "setTaskbarProgressEnabled" | "setTaskbarLyricsEnabled" | "setTaskbarLyricsClickThrough" | "setTaskbarLyricsPosition" | "toggleDeviceMenu" | "closeDeviceMenu"> {
  let deviceRequest = 0;
  let outputRevision = 0;
  let driverRevision = 0;
  const enqueueDevice = createConfirmedQueue<string>();
  const enqueueDriver = createConfirmedQueue<PlayerStore["driverKind"]>();
  const toggleQueues = {
    smtcEnabled: createConfirmedQueue<boolean>(),
    taskbarLyricsEnabled: createConfirmedQueue<boolean>(),
    taskbarLyricsClickThrough: createConfirmedQueue<boolean>(),
  };
  const toggleRevisions = new Map<BackendToggleKey, number>();
  const nextToggle = (key: BackendToggleKey) => {
    const revision = (toggleRevisions.get(key) ?? 0) + 1;
    toggleRevisions.set(key, revision);
    return () => toggleRevisions.get(key) === revision;
  };

  type TaskbarKey = "taskbarButtonsEnabled" | "taskbarProgressEnabled";
  let confirmedTaskbar: Record<TaskbarKey, boolean>;
  const taskbarQueue: Array<() => Promise<void>> = [];
  const drainTaskbar = async () => {
    while (taskbarQueue.length > 0) {
      await taskbarQueue[0]();
      taskbarQueue.shift();
    }
  };
  const commitTaskbarToggle = (
    key: TaskbarKey, value: boolean, successText: string, failureLabel: string
  ) => {
    const idle = taskbarQueue.length === 0;
    if (idle) {
      const { taskbarButtonsEnabled, taskbarProgressEnabled } = get();
      confirmedTaskbar = { taskbarButtonsEnabled, taskbarProgressEnabled };
    }
    const isStillCurrent = nextToggle(key);
    set({ [key]: value });
    // 两字段共用一条写命令，必须串行；另一字段只带已确认值，不能夹带未确认意图。
    taskbarQueue.push(async () => {
      try {
        await sendCommandAsync("set_taskbar_features", {
          buttons: key === "taskbarButtonsEnabled" ? value : confirmedTaskbar.taskbarButtonsEnabled,
          progress: key === "taskbarProgressEnabled" ? value : confirmedTaskbar.taskbarProgressEnabled,
        });
        confirmedTaskbar[key] = value;
        if (isStillCurrent() && get()[key] === value) get().showNotification(successText);
      } catch (err) {

        console.warn("Tauri command failed: set_taskbar_features", err);
        if (!isStillCurrent() || get()[key] !== value) return;
        set({ [key]: confirmedTaskbar[key] });
        get().showNotification(`${failureLabel}失败：${normalizeIpcError(err).message}`, "error");
      }
    });
    if (idle) void drainTaskbar();
  };
  return {
  loadDevices: (options?: { enumerateOnly: boolean }) => {
    const request = ++deviceRequest;
    return invoke<BackendDevice[]>("list_devices")
      .then(async (devices) => {
        if (request !== deviceRequest) return;
        if (!Array.isArray(devices)) return;
        const normalized = devices.map(normalizeDevice);
        // 设备丢失时只刷新候选，不自动选中默认扬声器，更不能触发引擎重新起播。
        if (options?.enumerateOnly) {
          set({ devices: normalized });
          return;
        }
        if (normalized.length === 0) return;
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
        const revision = outputRevision;
        const driver = get().driverKind;
        await sendCommandAsync("set_output_driver", { driver });
        if (
          revision !== outputRevision || request !== deviceRequest ||
          get().currentDeviceId !== selectedDeviceId || get().driverKind !== driver
        ) return;
        await sendCommandAsync("select_output_device", { deviceId: selectedDeviceId });
      })
      .catch((err) => {

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
    const revision = ++outputRevision;
    set({ currentDeviceId: id, deviceMenuOpen: false });
    // 后续请求等待前项确认；旧请求成功也会推进回滚基线，但不覆盖最新界面意图。
    enqueueDevice(currentDeviceId, id,
      () => sendCommandAsync("select_output_device", { deviceId: id }),
      () => {
        if (revision === outputRevision && get().currentDeviceId === id) {
          get().showNotification(`输出设备已切换到: ${device?.name ?? id}`);
        }
      },
      (err, confirmed) => {
        console.warn("Tauri command failed: select_output_device", err);
        if (revision !== outputRevision || get().currentDeviceId !== id) return;
        set({ currentDeviceId: confirmed });
        get().showNotification(`切换输出设备失败：${normalizeIpcError(err).message}`, "error");
      }
    );
  },

  setDriver: (k) => {
    if (k === "asio") {
      get().showNotification("ASIO 输出尚未开放，请先使用 WASAPI 独占或系统共享输出");
      return;
    }
    if (get().driverKind === k) return;
    const epoch = bumpPlayEpoch();
    const revision = ++driverRevision;
    outputRevision++;
    // M-7 / 前端 M-5：切换 driver 前先停掉正在播的 session，避免后端 same-track 优化路径
    // 残留旧 driver 配置。stop 与 set_output_driver 必须串行（await stop 后再发 driver），
    // 否则 fire-and-forget 下两条命令到后端的顺序不保证，恰好触发想避免的“切换后不切音轨”。
    const { isPlaying: wasPlaying, driverKind: previousDriver } = get();
    const rollbackDriver = (err: unknown, confirmed: PlayerStore["driverKind"]) => {
      if (revision !== driverRevision || get().driverKind !== k) return;
      // 停播可能已经生效，只还原配置，不伪造播放成功或恢复旧时间。
      outputRevision++;
      set({ driverKind: confirmed });
      get().showNotification(playbackErrorMessage(err), "error");
    };
    set({ driverKind: k, isPlaying: false, currentTime: 0 });

    enqueueDriver(previousDriver, k, async () => {
      try {
        await sendCommandAsync("stop");
      } catch (err) {
        console.warn("Failed to stop before driver switch", err);
        throw err;
      }
      try {
        await sendCommandAsync("set_output_driver", { driver: k });
      } catch (err) {
        console.warn("Failed to set output driver", err);
        throw err;
      }
    }, async () => {

      // 若刚才在播，driver 切换后自动从头继续播放当前曲目，体验上无感
      if (wasPlaying && epoch === currentPlayEpoch() && revision === driverRevision) {
        const track = get().currentTrack();
        if (track) {
          // 续播沿用入口代际，迟到流程不能把自己重新标记为最新播放意图。
          const isStillCurrent = () =>
            epoch === currentPlayEpoch() && revision === driverRevision &&
            get().currentTrack()?.id === track.id;
          try {
            await sendPlayCommand(track, get, set, () => get().currentTime, isStillCurrent);
            // 审2-R2：Tauri 下 isPlaying 改由 playback_started 事件驱动（与发现15一致），
            // 删除乐观置位，避免后端实际起播失败时 UI 卡在播放态；stub 模式无事件，保留置位。
            if (!isTauriRuntime() && isStillCurrent()) set({ isPlaying: true });
          } catch (err) {

            console.warn("Failed to resume after driver switch", err);
            get().showNotification(playbackErrorMessage(err), "error");
          }
        }
      }
    }, rollbackDriver);
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
    commitBackendToggle(
      set, get, "smtcEnabled", enabled, "set_smtc_enabled", { enabled },
      enabled ? "已启用系统媒体控件" : "已停用系统媒体控件",
      enabled ? "启用系统媒体控件" : "停用系统媒体控件",
      nextToggle("smtcEnabled"), toggleQueues.smtcEnabled
    );
  },

  setTaskbarButtonsEnabled: (enabled) => {
    if (get().taskbarButtonsEnabled === enabled) return;
    commitTaskbarToggle(
      "taskbarButtonsEnabled", enabled,
      enabled ? "已启用任务栏播控按钮" : "已停用任务栏播控按钮",
      enabled ? "启用任务栏播控按钮" : "停用任务栏播控按钮"
    );
  },

  setTaskbarProgressEnabled: (enabled) => {
    if (get().taskbarProgressEnabled === enabled) return;
    commitTaskbarToggle(
      "taskbarProgressEnabled", enabled,
      enabled ? "已启用任务栏播放进度" : "已停用任务栏播放进度",
      enabled ? "启用任务栏播放进度" : "停用任务栏播放进度"
    );
  },

  setTaskbarLyricsEnabled: (enabled) => {
    if (get().taskbarLyricsEnabled === enabled) return;
    commitBackendToggle(
      set, get, "taskbarLyricsEnabled", enabled, "set_taskbar_lyrics_enabled", { enabled },
      enabled ? "已开启任务栏歌词条" : "已关闭任务栏歌词条",
      enabled ? "开启任务栏歌词条" : "关闭任务栏歌词条",
      nextToggle("taskbarLyricsEnabled"), toggleQueues.taskbarLyricsEnabled
    );
  },

  setTaskbarLyricsClickThrough: (enabled) => {
    if (get().taskbarLyricsClickThrough === enabled) return;
    commitBackendToggle(
      set, get, "taskbarLyricsClickThrough", enabled, "set_taskbar_lyrics_click_through", { enabled },
      enabled ? "歌词条已切换为仅显示（鼠标穿透）" : "歌词条已恢复鼠标交互",
      enabled ? "切换歌词条鼠标穿透" : "恢复歌词条鼠标交互",
      nextToggle("taskbarLyricsClickThrough"), toggleQueues.taskbarLyricsClickThrough
    );
  },

  setTaskbarLyricsPosition: (ratio) => {
    const clamped = Math.min(1, Math.max(0, ratio));
    if (!Number.isFinite(clamped)) return;
    if (get().taskbarLyricsPosition === clamped) return;
    set({ taskbarLyricsPosition: clamped });
    // 滑块拖动是连续事件，但后端只是一次幂等 SetWindowPos，直接透传即可；
    // 不发通知——每帧弹一条会刷屏。
    sendCommand("set_taskbar_lyrics_position", { ratio: clamped });
  },

  setRememberPlayback: (enabled) => {
    if (get().rememberPlayback === enabled) return;
    // 关闭记忆播放时立即清掉已持久化的续播位置，避免磁盘上残留上次播放痕迹；
    // partialize 也会在关闭时不再写入位置，双保险。
    if (!enabled) {
      set({
        rememberPlayback: false,
        persistedCurrentTrackId: null,
        persistedCurrentTime: 0,
      });
    } else {
      set({ rememberPlayback: true });
    }
    get().showNotification(
      enabled ? "已开启记忆播放" : "已关闭记忆播放"
    );
  },
  };
}

