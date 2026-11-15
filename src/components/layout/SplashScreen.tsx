import { useEffect, useState } from "react";
import { usePlayerStore } from "@/store/player";

/** 最短显示时间：界面就绪得再快，也让启动画面完整闪现一下，避免一闪而过的白块。 */
const MIN_VISIBLE_MS = 600;
/** 兜底上限：水合信号迟迟不来（或测试环境从不水合）时也要让出界面。 */
const MAX_VISIBLE_MS = 2800;
/** 退场动画时长（与 index.css 的 .splash-root.is-leaving 一致）。 */
const EXIT_MS = 350;
const SHOWN_FLAG = "seraph-splash-shown";

/** 同一会话里的重载（导入配置、崩溃兜底页「重新加载」）不再播放启动画面。 */
function alreadyShownThisSession() {
  try {
    if (window.sessionStorage.getItem(SHOWN_FLAG)) return true;
    window.sessionStorage.setItem(SHOWN_FLAG, "1");
  } catch {
    // 存储不可用：按首次启动处理
  }
  return false;
}

type Phase = "visible" | "leaving" | "gone";

/**
 * 启动画面。动画全部用 CSS（`index.css` 的 `.splash-*`）：此前为了这一个组件
 * 把 framer-motion 整个打进了首屏主包（FE-05）。系统开启「减弱动态效果」时
 * 位移 / 缩放 / 旋转由 CSS 媒体查询关闭，只保留淡入淡出。
 */
export function SplashScreen() {
  const [phase, setPhase] = useState<Phase>(() => (alreadyShownThisSession() ? "gone" : "visible"));

  useEffect(() => {
    if (phase !== "visible") return;
    // UI5-01：以「设置水合完成」为结束条件，而不是固定计时 2.8 秒。
    const startedAt = performance.now();
    let minTimer: number | undefined;
    const leave = () => setPhase((current) => (current === "visible" ? "leaving" : current));
    const finish = () => {
      if (minTimer !== undefined) return;
      const remaining = Math.max(0, MIN_VISIBLE_MS - (performance.now() - startedAt));
      minTimer = window.setTimeout(leave, remaining);
    };
    const persist = usePlayerStore.persist;
    const unsubscribe = persist.hasHydrated() ? (finish(), undefined) : persist.onFinishHydration(finish);
    const maxTimer = window.setTimeout(leave, MAX_VISIBLE_MS);
    return () => {
      unsubscribe?.();
      window.clearTimeout(maxTimer);
      if (minTimer !== undefined) window.clearTimeout(minTimer);
    };
    // 只在挂载时建立一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (phase !== "leaving") return;
    const timer = window.setTimeout(() => setPhase("gone"), EXIT_MS);
    return () => window.clearTimeout(timer);
  }, [phase]);

  if (phase === "gone") return null;

  return (
    <div
      className={`splash-root absolute inset-0 z-[9999] flex flex-col items-center justify-center bg-[#f5f1e8] overflow-hidden${phase === "leaving" ? " is-leaving" : ""}`}
      data-tauri-drag-region
      data-testid="splash-screen"
    >
      <div className="relative flex items-center justify-center h-48 w-48 pointer-events-none mb-4">
        {/* 向外扩散的声波 */}
        <div className="splash-wave absolute inset-0 rounded-full border border-[#c2a370] z-0" style={{ animationDelay: "1.2s" }} />
        <div className="splash-wave absolute inset-0 rounded-full border border-[#c2a370] z-0" style={{ animationDelay: "2.2s" }} />

        {/* 唱片 */}
        <div className="splash-vinyl-in absolute inset-0 z-10">
          <div
            className="splash-vinyl-spin relative flex h-full w-full items-center justify-center rounded-full bg-[#111] shadow-2xl"
            style={{
              boxShadow: "0 25px 50px -12px rgba(0,0,0,0.5), inset 0 0 20px rgba(0,0,0,1)",
              border: "1px solid #222",
            }}
          >
            <div className="absolute inset-[12%] rounded-full border border-white/[0.03]" />
            <div className="absolute inset-[24%] rounded-full border border-white/[0.04]" />
            <div className="absolute inset-[36%] rounded-full border border-white/[0.03]" />
            <div className="absolute inset-[48%] rounded-full border border-white/[0.02]" />
            <div className="absolute inset-0 rounded-full bg-gradient-to-tr from-white/0 via-white/10 to-white/0 mix-blend-overlay pointer-events-none" />
            <div className="absolute inset-0 rounded-full bg-gradient-to-bl from-white/0 via-white/5 to-white/0 mix-blend-overlay pointer-events-none" />
            <div className="absolute h-16 w-16 rounded-full bg-[#c2a370] flex items-center justify-center shadow-[inset_0_0_10px_rgba(0,0,0,0.3)]">
              <div className="h-3 w-3 rounded-full bg-[#f5f1e8] shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)]" />
              <div className="absolute inset-1 rounded-full border border-[#8a724b]/50" />
              <div className="absolute inset-2 rounded-full border border-[#8a724b]/30" />
            </div>
          </div>
        </div>

        {/* 唱臂 */}
        <div className="splash-tonearm absolute -top-2 -right-6 h-36 w-10 origin-[20px_20px] z-20">
          <div className="absolute top-0 right-0 h-10 w-10 rounded-full bg-[#d4d4d4] shadow-[0_4px_10px_rgba(0,0,0,0.4),inset_0_2px_4px_rgba(255,255,255,0.8)] flex items-center justify-center z-30">
            <div className="h-5 w-5 rounded-full bg-[#222] shadow-inner" />
          </div>
          <div className="absolute top-8 right-[16px] h-24 w-2 bg-gradient-to-r from-[#d4d4d4] via-[#f5f5f5] to-[#a3a3a3] rounded-full shadow-[2px_2px_5px_rgba(0,0,0,0.3)] origin-top transform rotate-3" />
          <div className="absolute bottom-[2px] right-[10px] h-7 w-4 bg-[#2a2a2a] rounded-sm shadow-lg border-t-2 border-[#555] transform rotate-[18deg]" />
        </div>
      </div>

      <div className="splash-fade mt-6 flex flex-col items-center pointer-events-none" style={{ animationDelay: "0.5s", animationDuration: "1s" }}>
        <h1 className="text-3xl font-bold tracking-[0.4em] text-[#1a1a1a] drop-shadow-sm flex ml-[0.4em]">
          {"SERAPH".split("").map((char, index) => (
            <span key={index} className="splash-letter" style={{ animationDelay: `${0.7 + index * 0.1}s` }}>
              {char}
            </span>
          ))}
        </h1>
        <div className="splash-rule h-px w-24 bg-gradient-to-r from-transparent via-[#c2a370] to-transparent my-4 origin-center" />
        <p className="splash-fade text-[11px] font-medium tracking-[0.3em] text-[#888] ml-[0.3em]" style={{ animationDelay: "1.6s", animationDuration: "1s" }}>
          AUDIO PLAYER
        </p>
      </div>
    </div>
  );
}
