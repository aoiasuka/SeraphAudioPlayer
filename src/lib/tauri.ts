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
  cb: (payload: T) => void
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
    listen: async (event) => {
      // eslint-disable-next-line no-console
      console.debug(`[stub] listen(${event})`);
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
    listenFn = async <T,>(event: string, cb: (payload: T) => void) => {
      const unlisten = await evt.listen<T>(event, (e) => cb(e.payload));
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

export async function listen<T = unknown>(
  event: string,
  cb: (payload: T) => void
): Promise<() => void> {
  const listen = await getListen();
  return listen<T>(event, cb);
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
