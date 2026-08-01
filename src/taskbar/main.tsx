// 任务栏歌词条独立入口。
//
// ⚠️ 项目约束(persist 门闩):本入口与其下所有模块绝不 import
// src/store/** 与 src/boot/**——两个窗口共享 localStorage,第二窗口一旦
// 水合 persist store,pagehide flush 会用旧内存状态覆盖主窗口数据。
// 数据一律走 IPC(快照 + seraph://event 事件流 + get_track_info)。
import React from "react";
import ReactDOM from "react-dom/client";
import { TaskbarLyricsBar } from "./TaskbarLyricsBar";
import "@fontsource/courier-prime/400.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/700.css";
import "../index.css";
import "./taskbar.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <TaskbarLyricsBar />
  </React.StrictMode>
);
