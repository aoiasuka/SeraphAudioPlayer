import React from "react";

/** boot 侧（applyConfigImport）识别的界面重置标记 */
export const UI_RESET_FLAG = "seraph-ui-reset-request";

interface AppErrorBoundaryState {
  error: Error | null;
}

/**
 * 根级错误边界：任何页面渲染抛错时兜底，避免 React 卸载整棵树后白屏，
 * 且无法自救（坏状态被持久化时曾形成“每次启动都白屏”的死循环，H-1）。
 *
 * “重置界面设置”走 sessionStorage 标记 + reload，由 main.tsx 首个 import
 * 的 boot 模块在所有 store 水合前清理 localStorage——绕开 persist 的
 * pagehide flush 时序（直接删 key 会被旧内存状态回写覆盖）。
 */
export class AppErrorBoundary extends React.Component<
  { children: React.ReactNode },
  AppErrorBoundaryState
> {
  state: AppErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // eslint-disable-next-line no-console
    console.error("UI render crashed", error, info.componentStack);
  }

  private reload = () => {
    window.location.reload();
  };

  private resetAndReload = () => {
    try {
      sessionStorage.setItem(UI_RESET_FLAG, "1");
    } catch {
      // sessionStorage 不可用时退化为纯 reload
    }
    window.location.reload();
  };

  render() {
    if (!this.state.error) return this.props.children;
    const message = this.state.error.message || String(this.state.error);
    // 全 inline style：崩溃兜底页不依赖应用样式管线
    return (
      <div
        style={{
          minHeight: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "#f3ead9",
          color: "#2b2722",
          fontFamily: "'Noto Sans SC', sans-serif",
          padding: 24,
        }}
      >
        <div style={{ maxWidth: 560, border: "1.5px solid #2b2722", padding: 24 }}>
          <h1 style={{ fontSize: 18, fontWeight: 700, margin: "0 0 12px" }}>
            界面渲染出现异常
          </h1>
          <p style={{ fontSize: 13, lineHeight: 1.7, margin: "0 0 8px" }}>
            播放内核与曲库数据不受影响。可先尝试重新加载；若每次启动都出现此页，
            多半是某项界面/音效设置数据损坏，用下方按钮重置后即可恢复
            （曲库、歌单与喜欢标记都会保留）。
          </p>
          <pre
            style={{
              fontSize: 11,
              background: "#e9dfc9",
              padding: 8,
              overflow: "auto",
              maxHeight: 120,
              whiteSpace: "pre-wrap",
              wordBreak: "break-all",
              margin: "0 0 16px",
            }}
          >
            {message}
          </pre>
          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button
              type="button"
              onClick={this.reload}
              style={{
                border: "1.5px solid #2b2722",
                background: "#2b2722",
                color: "#f3ead9",
                padding: "8px 16px",
                fontSize: 13,
                fontWeight: 700,
                cursor: "pointer",
              }}
            >
              重新加载
            </button>
            <button
              type="button"
              onClick={this.resetAndReload}
              style={{
                border: "1.5px solid #b4553c",
                background: "transparent",
                color: "#b4553c",
                padding: "8px 16px",
                fontSize: 13,
                fontWeight: 700,
                cursor: "pointer",
              }}
            >
              重置界面与音效设置并重载
            </button>
          </div>
        </div>
      </div>
    );
  }
}
