/**
 * Tauri IPC 封装。
 *
 * - 在 Tauri 桌面运行时，调用真实的 `invoke` / `listen`
 * - 在纯浏览器 (`npm run dev`) 时降级为 console.log，便于纯前端迭代
 */

type InvokeFn = <T = unknown>(
  cmd: string,
  args?: Record<string, unknown>
) => Promise<T>;

type ListenFn = <T = unknown>(
  event: string,
  cb: (payload: T) => void,
  /** 只接收该 label 窗口发出的事件；省略则接收任意来源 */
  windowLabel?: string
) => Promise<() => void>;

interface TauriBridge {
  invoke: InvokeFn;
  listen: ListenFn;
}

function createBrowserStub(): TauriBridge {
  return {
    invoke: async (cmd, args) => {
      // eslint-disable-next-line no-console
      console.debug(`[stub] invoke(${cmd})`, args);
      return undefined as never;
    },
    listen: async (event, _cb, windowLabel) => {
      // eslint-disable-next-line no-console
      console.debug(`[stub] listen(${event})`, windowLabel);
      return () => undefined;
    },
  };
}

const browserStub = createBrowserStub();
let invokeFn: InvokeFn | null = null;
let listenFn: ListenFn | null = null;

export function isTauriRuntime() {
  return (
    typeof window !== "undefined" &&
    "__TAURI_INTERNALS__" in window
  );
}

async function getInvoke(): Promise<InvokeFn> {
  if (invokeFn) return invokeFn;
  if (!isTauriRuntime()) {
    invokeFn = browserStub.invoke;
    return invokeFn;
  }

  try {
    const core = await import("@tauri-apps/api/core");
    invokeFn = core.invoke;
    return invokeFn;
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("Tauri API unavailable, falling back to stub", err);
    invokeFn = browserStub.invoke;
    return invokeFn;
  }
}

async function getListen(): Promise<ListenFn> {
  if (listenFn) return listenFn;
  if (!isTauriRuntime()) {
    listenFn = browserStub.listen;
    return listenFn;
  }

  try {
    const evt = await import("@tauri-apps/api/event");
    listenFn = async <T,>(
      event: string,
      cb: (payload: T) => void,
      windowLabel?: string
    ) => {
      const unlisten = await evt.listen<T>(
        event,
        (e) => cb(e.payload),
        windowLabel ? { target: { kind: "AnyLabel", label: windowLabel } } : undefined
      );
      return unlisten;
    };
    return listenFn;
  } catch (err) {
    // eslint-disable-next-line no-console
    console.warn("Tauri events unavailable, falling back to stub", err);
    listenFn = browserStub.listen;
    return listenFn;
  }
}

export async function invoke<T = unknown>(
  cmd: string,
  args?: Record<string, unknown>
): Promise<T> {
  const invoke = await getInvoke();
  return invoke<T>(cmd, args);
}

/**
 * 监听应用内事件。
 *
 * ⚠️ 默认 target 是 `Any`——Tauri v2 的 `Any` 监听器无视 emit 目标全收，
 * 连 `tauri://move` 这类**其他窗口**的窗口事件也会收到（多窗口下曾导致
 * 歌词条把主窗口的坐标当成自己的）。只关心本窗口的窗口事件时，必须传
 * `windowLabel` 收窄。
 */
export async function listen<T = unknown>(
  event: string,
  cb: (payload: T) => void,
  windowLabel?: string
): Promise<() => void> {
  const listen = await getListen();
  return listen<T>(event, cb, windowLabel);
}

/**
 * 定向发送应用内事件给主窗口。非 Tauri 环境为空操作。
 *
 * S-12：歌词条等第二窗口回传事件一律走 `emitTo("main")` 而非全局 `emit`——
 * 配合 capability 只授予 `core:event:allow-emit-to`，第二窗口无法向任意窗口
 * 广播/伪造事件。主窗口的 `listen`（target 为 Any）能正常收到定向事件。
 */
export async function emitToMain(event: string, payload?: unknown): Promise<void> {
  if (!isTauriRuntime()) return;
  const evt = await import("@tauri-apps/api/event");
  await evt.emitTo("main", event, payload);
}

export async function isTauri(): Promise<boolean> {
  return isTauriRuntime();
}

type TauriInternals = {
  convertFileSrc?: (filePath: string, protocol?: string) => string;
};

/**
 * 曲目封面地址归一化：
 * - http(s)/data/blob/asset 等浏览器可直接加载的地址原样返回（B 站封面是 https URL）
 * - 本地绝对路径（本地曲目提取出的封面文件）转成 asset 协议 URL
 * - 纯浏览器开发模式无法加载本地文件，返回空串让 UI 走无封面默认样式
 */
export function coverSrc(cover: string | undefined | null): string {
  if (!cover) return "";
  if (/^(https?:|data:|blob:|asset:)/i.test(cover)) return cover;
  const internals = (
    window as unknown as { __TAURI_INTERNALS__?: TauriInternals }
  ).__TAURI_INTERNALS__;
  if (internals?.convertFileSrc) return internals.convertFileSrc(cover);
  return "";
}

export const FRONTEND_EVENT = "seraph://event";

/** 结构化 IPC 错误码，与后端 IpcErrorCode 对齐。 */
export type IpcErrorCode =
  | "internal"
  | "invalid_input"
  | "not_found"
  | "cache_corrupt"
  | "io"
  | "network";

export interface IpcError {
  code: IpcErrorCode;
  message: string;
}

/**
 * 把 invoke 抛出的错误归一化为 { code, message }。
 * 后端结构化命令抛出 `{ code, message }` 对象；旧命令仍抛字符串——
 * 两种形态都归一，前端可安全按 code 分支、按 message 展示。
 */
export function normalizeIpcError(err: unknown): IpcError {
  if (err && typeof err === "object" && "code" in err && "message" in err) {
    const e = err as { code: unknown; message: unknown };
    if (typeof e.code === "string" && typeof e.message === "string") {
      return { code: e.code as IpcErrorCode, message: e.message };
    }
  }
  const message =
    typeof err === "string"
      ? err
      : err instanceof Error
        ? err.message
        : String(err);
  return { code: "internal", message };
}
