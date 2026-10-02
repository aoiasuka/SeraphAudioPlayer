// 任务栏歌词条独立入口。
//
// ⚠️ 项目约束(persist 门闩):本入口与其下所有模块绝不 import
// src/store/** 与 src/boot/**——两个窗口共享 localStorage,第二窗口一旦
// 水合 persist store,pagehide flush 会用旧内存状态覆盖主窗口数据。
// 数据一律走 IPC(快照 + seraph://event 事件流 + get_track_info)。
import React from "react";
import ReactDOM from "react-dom/client";
import { installDiagnosticsForwarding } from "@/lib/diagnostics";
import { invoke, isTauriRuntime } from "@/lib/tauri";
import { TaskbarLyricsBar } from "./TaskbarLyricsBar";
import "@fontsource/courier-prime/400.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/700.css";
import "../index.css";
import "./taskbar.css";

// REL-06/07：歌词条的 IPC 失败此前被 `.catch(() => undefined)` 吞掉，发布版无从定位。
// 命令名字面量必须写在 src/taskbar/ 下（F-03 白名单回归测试据此双向核对）。
if (isTauriRuntime()) {
  installDiagnosticsForwarding("taskbar", (record) => invoke("log_frontend", { ...record }));
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <TaskbarLyricsBar />
  </React.StrictMode>
);
