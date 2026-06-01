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

export const FRONTEND_EVENT = "seraph://event";
