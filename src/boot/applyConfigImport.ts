/**
 * 启动 boot 副作用：必须是 main.tsx 的首个 import。
 * 在任何 store 模块创建（analysisSettings 同步水合）之前：
 * 1. 把上次会话导入的配置写回 localStorage；
 * 2. 处理崩溃兜底页（AppErrorBoundary）发出的界面重置请求。
 * 本模块不得 import 任何 store 或组件——只做 localStorage 手术。
 */
import { applyPendingConfigImport } from "@/lib/configTransfer";

applyPendingConfigImport();
applyPendingUiReset();

/**
 * AppErrorBoundary 的自救出口（标记常量与组件中的 UI_RESET_FLAG 保持一致）：
 * 清掉纯设置类 store，player 只把落地页拨回本地音乐——坏状态页随
 * activeView 持久化时曾形成“每次启动都白屏”的死循环（H-1）。
 * 曲库、歌单、喜欢标记等个人数据全部保留。
 */
function applyPendingUiReset() {
  try {
    if (sessionStorage.getItem("seraph-ui-reset-request") !== "1") return;
    sessionStorage.removeItem("seraph-ui-reset-request");
    localStorage.removeItem("seraph-eq-state");
    localStorage.removeItem("seraph-analysis-settings");
    const raw = localStorage.getItem("seraph-player-state");
    if (raw) {
      const parsed = JSON.parse(raw) as { state?: Record<string, unknown> };
      if (parsed && typeof parsed === "object" && parsed.state && typeof parsed.state === "object") {
        parsed.state.activeView = "local";
        localStorage.setItem("seraph-player-state", JSON.stringify(parsed));
      }
    }
  } catch {
    // 自救失败不阻塞启动（player-state 解析失败等留给 store migrate 兜底）
  }
}
