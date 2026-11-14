import { motion, AnimatePresence, MotionConfig } from "framer-motion";
import { useEffect, useState } from "react";
import { usePlayerStore } from "@/store/player";

/** 最短显示时间：界面就绪得再快，也让启动画面完整闪现一下，避免一闪而过的白块。 */
const MIN_VISIBLE_MS = 600;
/** 兜底上限：水合信号迟迟不来（或测试环境从不水合）时也要让出界面。 */
const MAX_VISIBLE_MS = 2800;
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

export function SplashScreen() {
  const [visible, setVisible] = useState(() => !alreadyShownThisSession());

  useEffect(() => {
    if (!visible) return;
    // UI5-01：以「设置水合完成」为结束条件，而不是固定计时 2.8 秒。
    const startedAt = performance.now();
    let minTimer: number | undefined;
    const finish = () => {
      if (minTimer !== undefined) return;
      const remaining = Math.max(0, MIN_VISIBLE_MS - (performance.now() - startedAt));
      minTimer = window.setTimeout(() => setVisible(false), remaining);
    };
    const persist = usePlayerStore.persist;
    const unsubscribe = persist.hasHydrated() ? (finish(), undefined) : persist.onFinishHydration(finish);
    const maxTimer = window.setTimeout(() => setVisible(false), MAX_VISIBLE_MS);
    return () => {
      unsubscribe?.();
      window.clearTimeout(maxTimer);
      if (minTimer !== undefined) window.clearTimeout(minTimer);
    };
    // 只在挂载时建立一次
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    // 系统开启「减弱动态效果」时，位移 / 缩放 / 旋转动画由 framer-motion 关闭，只保留淡入淡出
    <MotionConfig reducedMotion="user">
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 1 }}
          exit={{ opacity: 0, filter: "blur(12px)", scale: 1.05 }}
          transition={{ duration: 0.35, ease: "easeInOut" }}
          className="absolute inset-0 z-[9999] flex flex-col items-center justify-center bg-[#f5f1e8] overflow-hidden"
          data-tauri-drag-region
        >
          {/* Subtle noise/texture overlay could be applied here if desired */}
          
          <div className="relative flex items-center justify-center h-48 w-48 pointer-events-none mb-4">
            
            {/* Emitting Audio Waves */}
            <motion.div
              initial={{ scale: 0.8, opacity: 0 }}
              animate={{ scale: 1.5, opacity: 0 }}
              transition={{ delay: 1.2, duration: 2, repeat: Infinity, ease: "easeOut" }}
              className="absolute inset-0 rounded-full border border-[#c2a370] z-0"
            />
            <motion.div
              initial={{ scale: 0.8, opacity: 0 }}
              animate={{ scale: 1.5, opacity: 0 }}
              transition={{ delay: 2.2, duration: 2, repeat: Infinity, ease: "easeOut" }}
              className="absolute inset-0 rounded-full border border-[#c2a370] z-0"
            />

            {/* Vinyl Record */}
            <motion.div
              initial={{ scale: 0, rotate: -180 }}
              animate={{ scale: 1, rotate: 0 }}
              transition={{ duration: 1, type: "spring", bounce: 0.4 }}
              className="absolute inset-0 z-10"
            >
              <motion.div
                animate={{ rotate: 360 }}
                transition={{
                  repeat: Infinity,
                  duration: 2.5,
                  ease: "linear",
                }}
                className="relative flex h-full w-full items-center justify-center rounded-full bg-[#111] shadow-2xl"
                style={{
                  boxShadow: "0 25px 50px -12px rgba(0,0,0,0.5), inset 0 0 20px rgba(0,0,0,1)",
                  border: "1px solid #222"
                }}
              >
                {/* Vinyl Grooves - using concentric borders */}
                <div className="absolute inset-[12%] rounded-full border border-white/[0.03]" />
                <div className="absolute inset-[24%] rounded-full border border-white/[0.04]" />
                <div className="absolute inset-[36%] rounded-full border border-white/[0.03]" />
                <div className="absolute inset-[48%] rounded-full border border-white/[0.02]" />
                
                {/* Shiny Reflection Overlay */}
                <div className="absolute inset-0 rounded-full bg-gradient-to-tr from-white/0 via-white/10 to-white/0 mix-blend-overlay pointer-events-none" />
                <div className="absolute inset-0 rounded-full bg-gradient-to-bl from-white/0 via-white/5 to-white/0 mix-blend-overlay pointer-events-none" />

                {/* Center Label */}
                <div className="absolute h-16 w-16 rounded-full bg-[#c2a370] flex items-center justify-center shadow-[inset_0_0_10px_rgba(0,0,0,0.3)]">
                  {/* Spindle hole */}
                  <div className="h-3 w-3 rounded-full bg-[#f5f1e8] shadow-[inset_0_2px_4px_rgba(0,0,0,0.5)]" />
                  {/* Outer ring of the label */}
                  <div className="absolute inset-1 rounded-full border border-[#8a724b]/50" />
                  <div className="absolute inset-2 rounded-full border border-[#8a724b]/30" />
                </div>
              </motion.div>
            </motion.div>

            {/* Tonearm (Stylized) */}
            <motion.div
              initial={{ rotate: -35, opacity: 0 }}
              animate={{ rotate: 12, opacity: 1 }}
              transition={{ delay: 0.8, duration: 0.8, type: "spring", bounce: 0.3 }}
              exit={{ rotate: -35, opacity: 0, transition: { duration: 0.4 } }}
              className="absolute -top-2 -right-6 h-36 w-10 origin-[20px_20px] z-20"
            >
              {/* Pivot */}
              <div className="absolute top-0 right-0 h-10 w-10 rounded-full bg-[#d4d4d4] shadow-[0_4px_10px_rgba(0,0,0,0.4),inset_0_2px_4px_rgba(255,255,255,0.8)] flex items-center justify-center z-30">
                <div className="h-5 w-5 rounded-full bg-[#222] shadow-inner" />
              </div>
              {/* Arm Tube */}
              <div className="absolute top-8 right-[16px] h-24 w-2 bg-gradient-to-r from-[#d4d4d4] via-[#f5f5f5] to-[#a3a3a3] rounded-full shadow-[2px_2px_5px_rgba(0,0,0,0.3)] origin-top transform rotate-3" />
              {/* Headshell */}
              <div className="absolute bottom-[2px] right-[10px] h-7 w-4 bg-[#2a2a2a] rounded-sm shadow-lg border-t-2 border-[#555] transform rotate-[18deg]" />
            </motion.div>
          </div>
          
          <motion.div 
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ delay: 0.5, duration: 1 }}
            className="mt-6 flex flex-col items-center pointer-events-none"
          >
            <h1 className="text-3xl font-bold tracking-[0.4em] text-[#1a1a1a] drop-shadow-sm flex ml-[0.4em]">
              {"SERAPH".split("").map((char, index) => (
                <motion.span
                  key={index}
                  initial={{ opacity: 0, y: 10, filter: "blur(4px)" }}
                  animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
                  transition={{ delay: 0.7 + index * 0.1, duration: 0.6, ease: "easeOut" }}
                >
                  {char}
                </motion.span>
              ))}
            </h1>
            <motion.div
              initial={{ scaleX: 0 }}
              animate={{ scaleX: 1 }}
              transition={{ delay: 1.4, duration: 0.8, ease: "easeInOut" }}
              className="h-px w-24 bg-gradient-to-r from-transparent via-[#c2a370] to-transparent my-4 origin-center"
            />
            <motion.p 
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ delay: 1.6, duration: 1 }}
              className="text-[11px] font-medium tracking-[0.3em] text-[#888] ml-[0.3em]"
            >
              AUDIO PLAYER
            </motion.p>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
    </MotionConfig>
  );
}
