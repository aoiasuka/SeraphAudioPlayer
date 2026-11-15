import type { PersistStorage } from "zustand/middleware";
import type { PersistedPlayerState } from "./types";

// 审2-R1：水合门闩。store 使用 skipHydration，rehydrate 被 runWhenIdle 最长推迟 1.8s；
// 空窗期内 zustand persist 对任何 set 都会无条件 setItem，此时内存里还是默认状态，
// 落盘会把用户已持久化的收藏/歌单整个覆盖掉。ready 之前丢弃所有写入。
// getItem 只会在 rehydrate 时被调用，因此在 getItem 里自动置位（直接调 rehydrate 的
// 测试等场景也能解锁）；useHydratePlayerStore 里在 rehydrate 前还会显式置位兜底。
export const hydrationGate = { ready: false };

/**
 * FE2-01：持久化出问题时通知界面。本模块不能 import store（循环依赖），
 * 由 useHydratePlayerStore 挂监听；每类事件每次会话只报一次。
 * - write-failed：localStorage 写入失败，之后的改动只在内存里，关闭即丢。
 * - corrupt-backed-up：存档解析失败，原文已另存到 `<name>.corrupt` 再重置。
 * - corrupt-lost：存档解析失败且备份也写不进去。
 */
export type PersistProblem = "write-failed" | "corrupt-backed-up" | "corrupt-lost";
export const persistProblems: {
  listener: ((problem: PersistProblem) => void) | null;
  pending: PersistProblem[];
} = { listener: null, pending: [] };

const reportedProblems = new Set<PersistProblem>();
function reportProblem(problem: PersistProblem) {
  if (reportedProblems.has(problem)) return;
  reportedProblems.add(problem);
  // 存档损坏发生在水合期间，监听可能还没挂上：先排队，挂上时补发。
  if (persistProblems.listener) persistProblems.listener(problem);
  else persistProblems.pending.push(problem);
}

export function subscribePersistProblems(listener: (problem: PersistProblem) => void) {
  persistProblems.listener = listener;
  persistProblems.pending.splice(0).forEach(listener);
  return () => {
    if (persistProblems.listener === listener) persistProblems.listener = null;
  };
}

/** 仅测试用：清掉「已报告」记录。 */
export function resetPersistProblemsForTest() {
  reportedProblems.clear();
  persistProblems.pending.length = 0;
  persistProblems.listener = null;
}

export const corruptBackupKey = (name: string) => `${name}.corrupt`;

function isSamePersistedState(
  previous: PersistedPlayerState,
  next: PersistedPlayerState
) {
  // 直接比较 partialize 的字段集合，新增持久化设置时不会漏掉变更。
  // previous 来自本地存储，水合清洗前也可能是坏结构。
  if (!previous || typeof previous !== "object") return false;
  const keys = Object.keys(next) as (keyof PersistedPlayerState)[];
  return keys.length === Object.keys(previous).length &&
    keys.every((key) => Object.is(previous[key], next[key]));
}

export function createPlayerPersistStorage(): PersistStorage<PersistedPlayerState> {
  let lastValue: {
    state: PersistedPlayerState;
    version?: number;
  } | null = null;
  const memoryStorage = new Map<string, string>();

  const storage = () =>
    typeof window === "undefined" ? null : window.localStorage;

  const writeNow = (name: string, serialized: string) => {
    const localStorage = storage();
    if (localStorage) {
      try {
        localStorage.setItem(name, serialized);
      } catch (err) {
        // QuotaExceededError / SecurityError 等：回退到内存存储，避免崩溃

        console.warn(
          "localStorage.setItem failed, falling back to memory storage",
          err
        );
        memoryStorage.set(name, serialized);
        reportProblem("write-failed");
      }
    } else {
      memoryStorage.set(name, serialized);
    }
  };

  // 发现14：300ms trailing debounce，避免拖动音量滑块等高频 set 触发同步落盘 IO
  let pendingWrite: { name: string; value: { state: PersistedPlayerState; version?: number } } | null = null;
  let writeTimer: number | null = null;

  const flushPendingWrite = () => {
    if (writeTimer !== null) {
      window.clearTimeout(writeTimer);
      writeTimer = null;
    }
    if (!pendingWrite) return;
    const { name, value } = pendingWrite;
    pendingWrite = null;
    writeNow(name, JSON.stringify(value));
  };

  if (typeof window !== "undefined") {
    // 页面卸载前把挂起的状态落盘
    window.addEventListener("pagehide", flushPendingWrite);
  }

  return {
    getItem: (name) => {
      // 审2-R1：getItem 只在 rehydrate 时被调用，读到即代表水合开始，自动打开写门闩
      // （必须先于返回，version 迁移完成后的回写才能通过）。
      hydrationGate.ready = true;
      const value = storage()?.getItem(name) ?? memoryStorage.get(name) ?? null;
      if (value === null) {
        lastValue = null;
        return null;
      }

      try {
        const parsed = JSON.parse(value) as {
          state: PersistedPlayerState;
          version?: number;
        };
        lastValue = parsed;
        return parsed;
      } catch (err) {

        console.warn("Failed to parse persisted player state, resetting it", err);
        lastValue = null;
        // FE2-01：收藏和歌单都在这份存档里，重置前先把原文另存一份，
        // 留下可供人工恢复的现场（与后端曲库的 `.corrupt` 做法一致）。
        let backedUp = false;
        try {
          const local = storage();
          if (local) {
            local.setItem(corruptBackupKey(name), value);
            backedUp = true;
          }
        } catch (backupErr) {
          console.warn("Failed to back up corrupt player state", backupErr);
        }
        reportProblem(backedUp ? "corrupt-backed-up" : "corrupt-lost");
        storage()?.removeItem(name);
        memoryStorage.delete(name);
        return null;
      }
    },
    setItem: (name, value) => {
      // 审2-R1：未水合前的任何写入都只能是默认状态，落盘会覆盖用户数据，直接丢弃。
      if (!hydrationGate.ready) return;
      if (
        lastValue &&
        lastValue.version === value.version &&
        isSamePersistedState(lastValue.state, value.state)
      ) {
        return;
      }

      lastValue = value;
      // Zustand 状态按不可变值更新，防抖期间仅保留最后一份，flush 时再编码。
      pendingWrite = { name, value };
      if (typeof window === "undefined") {
        flushPendingWrite();
        return;
      }
      if (writeTimer !== null) window.clearTimeout(writeTimer);
      writeTimer = window.setTimeout(flushPendingWrite, 300);
    },
    removeItem: (name) => {
      if (writeTimer !== null) {
        window.clearTimeout(writeTimer);
        writeTimer = null;
      }
      pendingWrite = null;
      lastValue = null;
      storage()?.removeItem(name);
      memoryStorage.delete(name);
    },
  };
}

