import { create } from "zustand";
import { persist } from "zustand/middleware";
import { createBilibiliActions } from "./player/bilibiliActions";
import { createLibraryActions } from "./player/libraryActions";
import { createLyricsActions } from "./player/lyricsActions";
import { createOutputActions } from "./player/outputActions";
import { createPlaybackActions } from "./player/playbackActions";
import {
  sanitizeLiked,
  sanitizeLikedAt,
  sanitizeUserPlaylists,
} from "./player/personalData";
import { createPlayerPersistStorage } from "./player/persistStorage";
import { createStreamingActions } from "./player/streamingActions";
import type {
  BilibiliMultiPageMode,
  PlayerStore,
  PlayerStoreGet,
  PlayerStoreSet,
} from "./player/types";
import { createUiActions } from "./player/uiActions";
import { sanitizeExcludeRules } from "@/lib/lyrics/exclude";
import {
  DEFAULT_AMLL_TTML_DB_URL,
  isValidAmllTtmlDbUrl,
  normalizeAmllTtmlDbUrl,
  sanitizeLyricsSourcePriority,
} from "@/lib/lyrics/settings";
import type { DriverKind, LibraryView } from "@/types/track";

export type {
  BilibiliBatchImportResult,
  BilibiliImportFailure,
  BilibiliImportOptions,
  BilibiliMultiPageMode,
  BilibiliVideoInfo,
  PersistedPlayerState,
  PlayerStore,
} from "./player/types";

// v2: 新增 persistedCurrentTrackId，用曲目 id（而非索引）恢复上次播放位置
// v3: 新增 rememberPlayback（记忆播放开关，默认开）
const PERSIST_VERSION = 3;
const validDrivers = new Set<DriverKind>(["wasapi", "direct", "asio"]);
const validViews = new Set<LibraryView>([
  "local",
  "streaming",
  "recent",
  "liked",
  "playlists",
  "artists",
  "albums",
  "eq",
  "analysis",
]);

function asRecord(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" ? value as Record<string, unknown> : {};
}

function stringArray(value: unknown) {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function finiteNumber(value: unknown, fallback: number) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function clampVolume(value: unknown, fallback: number) {
  return Math.max(0, Math.min(1, finiteNumber(value, fallback)));
}

function migrateMultiPageMode(value: unknown): BilibiliMultiPageMode {
  return value === "all" ? "all" : "first";
}

function migrateDriver(value: unknown): DriverKind {
  if (value === "usb") return "wasapi";
  if (value === "asio") return "direct";
  return typeof value === "string" && validDrivers.has(value as DriverKind)
    ? value as DriverKind
    : "direct";
}

function migrateView(value: unknown): LibraryView {
  return typeof value === "string" && validViews.has(value as LibraryView)
    ? value as LibraryView
    : "local";
}

/**
 * 歌词条默认位置比例。对应后端 `bar_metrics` 的默认落点（右缘留 6×条高
 * 的托盘区）：1920×40 任务栏上约在可移动区间的 88% 处。
 */
export const DEFAULT_TASKBAR_LYRICS_POSITION = 0.88;

function normalizeLyricsPosition(value: unknown) {
  return typeof value === "number" && Number.isFinite(value)
    ? Math.min(1, Math.max(0, value))
    : DEFAULT_TASKBAR_LYRICS_POSITION;
}

export function migratePersistedPlayerState(persistedState: unknown) {
  const state = asRecord(persistedState);
  const volume = clampVolume(state.volume, 0.7);
  return {
    currentTrackIndex: Math.max(0, Math.trunc(finiteNumber(state.currentTrackIndex, 0))),
    persistedCurrentTrackId:
      typeof state.persistedCurrentTrackId === "string" && state.persistedCurrentTrackId
        ? state.persistedCurrentTrackId
        : null,
    persistedCurrentTime: Math.max(0, finiteNumber(state.persistedCurrentTime, 0)),
    recentTrackIds: stringArray(state.recentTrackIds).slice(0, 12),
    volume,
    isMuted: typeof state.isMuted === "boolean" ? state.isMuted : volume === 0,
    previousVolume: clampVolume(state.previousVolume, 0.7),
    shuffleMode: state.shuffleMode === true,
    loopMode: state.loopMode === true,
    liked: sanitizeLiked(state.liked),
    // 旧存档没有收藏时间：留空，「我喜欢」里这些曲目按曲库顺序排在后面
    likedAt: sanitizeLikedAt(state.likedAt),
    userPlaylists: sanitizeUserPlaylists(state.userPlaylists),
    currentDeviceId:
      typeof state.currentDeviceId === "string" && state.currentDeviceId.trim()
        ? state.currentDeviceId
        : "",
    driverKind: migrateDriver(state.driverKind),
    activeView: migrateView(state.activeView),
    // 旧版本无此字段时默认启用（与既有行为一致）
    smtcEnabled: state.smtcEnabled !== false,
    // v3：旧版本无此字段时默认启用记忆播放（保持既有“恢复上次播放”的行为）
    rememberPlayback: state.rememberPlayback !== false,
    // 旧版本无此字段时保持既有行为（启动时检查）
    autoUpdateCheck: state.autoUpdateCheck !== false,
    // v0.5.4：任务栏集成，旧版本无此字段时默认启用（与后端默认一致）
    taskbarButtonsEnabled: state.taskbarButtonsEnabled !== false,
    taskbarProgressEnabled: state.taskbarProgressEnabled !== false,
    // v0.5.4：任务栏歌词条默认关闭（第二窗口有内存成本，按需开启）
    taskbarLyricsEnabled: state.taskbarLyricsEnabled === true,
    // v0.5.4：仅歌词模式（鼠标穿透）默认关闭
    taskbarLyricsClickThrough: state.taskbarLyricsClickThrough === true,
    // v0.5.5：歌词条位置比例，旧版本无此字段时用默认落点对应的比例
    taskbarLyricsPosition: normalizeLyricsPosition(state.taskbarLyricsPosition),
    // v0.6.0：歌词设置。TTML 默认开（SPlayer 同款默认）；繁体默认关；地址非法回退默认
    lyricsSourcePriority: sanitizeLyricsSourcePriority(state.lyricsSourcePriority),
    preferTraditionalLyrics: state.preferTraditionalLyrics === true,
    ttmlLyricsEnabled: state.ttmlLyricsEnabled !== false,
    amllTtmlDbCustom: state.amllTtmlDbCustom === true,
    amllTtmlDbUrl:
      typeof state.amllTtmlDbUrl === "string" &&
      isValidAmllTtmlDbUrl(state.amllTtmlDbUrl, state.amllTtmlDbCustom === true)
        ? normalizeAmllTtmlDbUrl(state.amllTtmlDbUrl)
        : DEFAULT_AMLL_TTML_DB_URL,
    lyricsExcludeRules: sanitizeExcludeRules(state.lyricsExcludeRules),
    showLyricsTranslation: state.showLyricsTranslation !== false,
    showLyricsRoman: state.showLyricsRoman === true,
    lyricsFolder: typeof state.lyricsFolder === "string" ? state.lyricsFolder.trim() : "",
    // B2：制作信息默认显示（升级不改变既有可见内容）；忽略文件 offset 默认关
    showLyricsCredits: state.showLyricsCredits !== false,
    ignoreLyricsFileOffset: state.ignoreLyricsFileOffset === true,
    bilibiliPreferDolbyAtmos: state.bilibiliPreferDolbyAtmos === true,
    bilibiliPreferFlac: state.bilibiliPreferFlac === true,
    bilibiliRemuxWithFfmpeg: state.bilibiliRemuxWithFfmpeg === true,
    // BILI-05：缺省（旧存储）与认不出的值都是只导入第 1 P——与此前的行为一致
    bilibiliMultiPageMode: migrateMultiPageMode(state.bilibiliMultiPageMode),
  };
}

export const usePlayerStore = create<PlayerStore>()(
  persist(
    (set, get) => {
      const storeSet = set as PlayerStoreSet;
      const storeGet = get as PlayerStoreGet;

      return {
        playlist: [],
        playbackQueuePreview: null,
        playScope: null,
        currentTrackIndex: 0,
        persistedCurrentTrackId: null,
        persistedCurrentTime: 0,
        recentTrackIds: [],
        isPlaying: false,
        currentTime: 0,
        outputLatency: 0,
        outputFormat: null,
        volume: 0.7,
        isMuted: false,
        previousVolume: 0.7,
        shuffleMode: false,
        loopMode: false,
        liked: {},
        likedAt: {},
        userPlaylists: [],
        // FE2-03：设备枚举完成前是空列表和空 ID（此前是一份演示设备，
        // 首次启动时设备菜单会短暂显示不存在的设备）。空 ID = 跟随系统默认设备。
        devices: [],
        currentDeviceId: "",
        // 默认使用系统共享输出（兼容性最高，适合普通扬声器/蓝牙），
        // 已选过其它 driver 的老用户仍从持久化状态恢复自己的选择。
        driverKind: "direct",
        activeView: "local",
        smtcEnabled: true,
        rememberPlayback: true,
        autoUpdateCheck: true,
        taskbarButtonsEnabled: true,
        taskbarProgressEnabled: true,
        taskbarLyricsEnabled: false,
        taskbarLyricsClickThrough: false,
        taskbarLyricsPosition: DEFAULT_TASKBAR_LYRICS_POSITION,
        lyricsSourcePriority: "auto",
        preferTraditionalLyrics: false,
        ttmlLyricsEnabled: true,
        amllTtmlDbUrl: DEFAULT_AMLL_TTML_DB_URL,
        amllTtmlDbCustom: false,
        lyricsExcludeRules: [],
        showLyricsTranslation: true,
        showLyricsRoman: false,
        lyricsFolder: "",
        showLyricsCredits: true,
        ignoreLyricsFileOffset: false,
        bilibiliPreferDolbyAtmos: false,
        bilibiliPreferFlac: false,
        bilibiliRemuxWithFfmpeg: false,
        bilibiliMultiPageMode: "first",
        deviceMenuOpen: false,
        settingsOpen: false,
        notification: null,
        notificationHistory: [],
        // 审2-R5：流媒体页提升到 store 的状态（非持久化，不进 partialize）
        bilibiliLoginStatus: { loggedIn: false },
        bilibiliFfmpegStatus: { available: false },
        ffmpegDownload: { stage: "idle", percent: 0 },
        bilibiliBatchProgress: null,
        loginQr: null,
        isLoginBusy: false,

        currentTrack: () => storeGet().playlist[storeGet().currentTrackIndex] ?? null,

        ...createPlaybackActions(storeSet, storeGet),
        ...createUiActions(storeSet, storeGet),
        ...createLibraryActions(storeSet, storeGet),
        ...createBilibiliActions(storeSet, storeGet),
        ...createLyricsActions(storeSet, storeGet),
        ...createOutputActions(storeSet, storeGet),
        ...createStreamingActions(storeSet, storeGet),

        resetLyricsSettings: () => {
          storeSet({
            lyricsSourcePriority: "auto",
            preferTraditionalLyrics: false,
            ttmlLyricsEnabled: true,
            amllTtmlDbUrl: DEFAULT_AMLL_TTML_DB_URL,
            amllTtmlDbCustom: false,
            showLyricsTranslation: true,
            showLyricsRoman: false,
            lyricsFolder: "",
          });
          // 排除规则匹配与显示投影都在 Rust 侧，必须走 action 让后端同步并广播重拉
          storeGet().setLyricsExcludeRules([]);
          storeGet().setShowLyricsCredits(true);
          storeGet().setIgnoreLyricsFileOffset(false);
          storeGet().showNotification("歌词设置已恢复默认");
        },
      };
    },
    {
      name: "seraph-player-state",
      version: PERSIST_VERSION,
      storage: createPlayerPersistStorage(),
      skipHydration: true,
      migrate: migratePersistedPlayerState,
      // 同版本和缺版本的存储不会触发 migrate，所有水合都必须清洗字段。
      merge: (persisted, current) => ({
        ...current,
        ...migratePersistedPlayerState(persisted),
      }),
      partialize: (state) => ({
        currentTrackIndex: state.currentTrackIndex,
        // 发现1：持久化当前曲目 id；playlist 未加载（为空）时回退到已持久化的 id。
        // v3：记忆播放关闭时不写入曲目/位置，磁盘上不留播放痕迹。
        persistedCurrentTrackId: state.rememberPlayback
          ? state.playlist[state.currentTrackIndex]?.id ?? state.persistedCurrentTrackId
          : null,
        // 播放进度按 5 秒粒度持久化：恢复误差 ≤5s，同时把播放中每秒 tick
        // 触发的 localStorage 写频率压到 1/5Hz
        persistedCurrentTime: state.rememberPlayback
          ? Math.floor(Math.max(0, state.currentTime) / 5) * 5
          : 0,
        recentTrackIds: state.recentTrackIds,
        volume: state.volume,
        isMuted: state.isMuted,
        previousVolume: state.previousVolume,
        shuffleMode: state.shuffleMode,
        loopMode: state.loopMode,
        liked: state.liked,
        likedAt: state.likedAt,
        userPlaylists: state.userPlaylists,
        currentDeviceId: state.currentDeviceId,
        driverKind: state.driverKind,
        activeView: state.activeView,
        smtcEnabled: state.smtcEnabled,
        rememberPlayback: state.rememberPlayback,
        autoUpdateCheck: state.autoUpdateCheck,
        taskbarButtonsEnabled: state.taskbarButtonsEnabled,
        taskbarProgressEnabled: state.taskbarProgressEnabled,
        taskbarLyricsEnabled: state.taskbarLyricsEnabled,
        taskbarLyricsClickThrough: state.taskbarLyricsClickThrough,
        taskbarLyricsPosition: state.taskbarLyricsPosition,
        lyricsSourcePriority: state.lyricsSourcePriority,
        preferTraditionalLyrics: state.preferTraditionalLyrics,
        ttmlLyricsEnabled: state.ttmlLyricsEnabled,
        amllTtmlDbUrl: state.amllTtmlDbUrl,
        amllTtmlDbCustom: state.amllTtmlDbCustom,
        lyricsExcludeRules: state.lyricsExcludeRules,
        showLyricsTranslation: state.showLyricsTranslation,
        showLyricsRoman: state.showLyricsRoman,
        lyricsFolder: state.lyricsFolder,
        showLyricsCredits: state.showLyricsCredits,
        ignoreLyricsFileOffset: state.ignoreLyricsFileOffset,
        bilibiliPreferDolbyAtmos: state.bilibiliPreferDolbyAtmos,
        bilibiliPreferFlac: state.bilibiliPreferFlac,
        bilibiliRemuxWithFfmpeg: state.bilibiliRemuxWithFfmpeg,
        bilibiliMultiPageMode: state.bilibiliMultiPageMode,
      }),
    }
  )
);
