/**
 * REL-06：前端诊断转发。发布版没有 devtools，`console.warn/error`、未捕获异常与未处理的
 * Promise 拒绝此前全部丢失（唯一留痕是控制台）。这里把它们转发到后端 `log_frontend`，
 * 写进与 Rust 同一份轮转日志（后端负责限长、单行化、按窗口限频与凭据脱敏）。
 *
 * `send` 由各窗口入口传入（`invoke("log_frontend", …)` 字面量写在入口文件里）：
 * 歌词条的命令白名单回归测试只扫 `src/taskbar/`，调用点必须在那里才能被双向核对。
 */

export interface FrontendLogRecord {
  level: "warn" | "error";
  scope: string;
  message: string;
}

const MAX_MESSAGE_CHARS = 2000;

/** 把任意值转成一行可读文本：IpcError `{code, message}`、Error（含首段栈）、其它 JSON。 */
export function describeForLog(value: unknown): string {
  if (typeof value === "string") return value;
  if (value instanceof Error) {
    const stack = value.stack?.split("\n").slice(1, 4).map((line) => line.trim()).join(" | ");
    return `${value.name}: ${value.message}${stack ? ` @ ${stack}` : ""}`;
  }
  if (value && typeof value === "object" && "message" in value) {
    const { code, message } = value as { code?: unknown; message?: unknown };
    if (typeof message === "string") return typeof code === "string" ? `${message} (${code})` : message;
  }
  try {
    const json = JSON.stringify(value);
    if (json !== undefined) return json;
  } catch {
    // 循环引用等：退回 String()
  }
  return String(value);
}

function format(args: unknown[]): string {
  return args.map(describeForLog).join(" ").slice(0, MAX_MESSAGE_CHARS);
}

/**
 * 安装转发；返回卸载函数（测试与热重载用）。同一窗口只应安装一次。
 * 原始控制台输出照常保留；发送失败静默——日志通道自身出错时不能再写日志（会递归）。
 */
export function installDiagnosticsForwarding(
  scope: string,
  send: (record: FrontendLogRecord) => Promise<unknown>
): () => void {
  let forwarding = false;
  const forward = (level: FrontendLogRecord["level"], recordScope: string, args: unknown[]) => {
    if (forwarding) return;
    forwarding = true;
    try {
      void send({ level, scope: recordScope, message: format(args) }).catch(() => undefined);
    } catch {
      // 同上：日志通道不可用时放弃这一条
    } finally {
      forwarding = false;
    }
  };

  const originalWarn = console.warn;
  const originalError = console.error;
  console.warn = (...args: unknown[]) => {
    originalWarn(...args);
    forward("warn", scope, args);
  };
  console.error = (...args: unknown[]) => {
    originalError(...args);
    forward("error", scope, args);
  };
  const onError = (event: ErrorEvent) => forward("error", `${scope}:uncaught`, [event.error ?? event.message]);
  const onRejection = (event: PromiseRejectionEvent) =>
    forward("error", `${scope}:unhandledrejection`, [event.reason]);
  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);

  return () => {
    console.warn = originalWarn;
    console.error = originalError;
    window.removeEventListener("error", onError);
    window.removeEventListener("unhandledrejection", onRejection);
  };
}
