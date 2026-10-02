// 必须最先执行：把上次导入的配置写回 localStorage，再让各 store 水合
import "./boot/applyConfigImport";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AppErrorBoundary } from "./components/AppErrorBoundary";
import { installDiagnosticsForwarding } from "./lib/diagnostics";
import { invoke, isTauriRuntime } from "./lib/tauri";
// Keep bundled fonts lean; heavier display weights fall back to system synthesis.
import "@fontsource/courier-prime/400.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/700.css";
import "./index.css";

// REL-06：发布版没有 devtools，前端警告 / 未捕获异常转发进后端诊断日志（seraph.log）
if (isTauriRuntime()) {
  installDiagnosticsForwarding("main", (record) => invoke("log_frontend", { ...record }));
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppErrorBoundary>
      <App />
    </AppErrorBoundary>
  </React.StrictMode>
);
