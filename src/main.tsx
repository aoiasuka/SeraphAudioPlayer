// 必须最先执行：把上次导入的配置写回 localStorage，再让各 store 水合
import "./boot/applyConfigImport";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
// Keep bundled fonts lean; heavier display weights fall back to system synthesis.
import "@fontsource/courier-prime/400.css";
import "@fontsource/noto-sans-sc/400.css";
import "@fontsource/noto-sans-sc/700.css";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
