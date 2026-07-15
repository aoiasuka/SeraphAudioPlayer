import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState } from "react";

export function SplashScreen() {
  const [visible, setVisible] = useState(true);

  useEffect(() => {
    // 保持动画显示至少 2 秒，以展示复古质感
    const timer = setTimeout(() => {
      setVisible(false);
    }, 2000);
    return () => clearTimeout(timer);
  }, []);

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 1 }}
          exit={{ opacity: 0, filter: "blur(10px)" }}
          transition={{ duration: 0.8, ease: "easeInOut" }}
          className="absolute inset-0 z-[9999] flex flex-col items-center justify-center bg-[#f5f1e8]"
          data-tauri-drag-region
        >
          {/* Vinyl Container */}
          <motion.div
            animate={{ rotate: 360 }}
            transition={{
              repeat: Infinity,
              duration: 3,
              ease: "linear",
            }}
            className="relative flex h-32 w-32 items-center justify-center rounded-full bg-[#1a1a1a] shadow-2xl pointer-events-none"
            style={{
              boxShadow: "0 20px 40px -10px rgba(0,0,0,0.4), inset 0 0 15px rgba(0,0,0,1)",
              border: "1px solid #333"
            }}
          >
            {/* Vinyl Grooves - using concentric borders */}
            <div className="absolute inset-3 rounded-full border border-[#2a2a2a]" />
            <div className="absolute inset-6 rounded-full border border-[#2a2a2a]" />
            <div className="absolute inset-9 rounded-full border border-[#2a2a2a]" />
            
            {/* Center Label */}
            <div className="absolute h-11 w-11 rounded-full bg-[#c2a370] flex items-center justify-center shadow-inner">
              {/* Spindle hole */}
              <div className="h-2 w-2 rounded-full bg-[#f5f1e8] shadow-sm" />
            </div>
          </motion.div>
          
          <motion.div 
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            transition={{ delay: 0.4, duration: 0.8 }}
            className="mt-10 flex flex-col items-center pointer-events-none"
          >
            <h1 className="text-xl font-bold tracking-[0.3em] text-[#1a1a1a]">SERAPH</h1>
            <p className="mt-2 text-[10px] tracking-[0.2em] text-[#888]">AUDIO PLAYER</p>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
