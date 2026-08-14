import { useEffect, useState } from "react";

/**
 * W-01:逐字动画的文本长度上限。打字机每 tick 做一次 `text.slice(0, n)`(O(n) 重建
 * 整串)并触发一次 React 重渲染,速度下限是 30 ms/字符——文本一长就是双重灾难:
 * 跑完要 `length × 30 ms`(2 MB 单行 >16 小时),期间每 30 ms 重建一次巨串,UI 永久冻结。
 * 后端已把单行钳到 512 字符(`MAX_LYRIC_LINE_CHARS`),这里是前端侧的独立兜底——
 * 歌词条/歌词稿之外若有别的调用方传入长文本,超限直接整段显示、不做动画。
 */
const MAX_TYPEWRITER_CHARS = 512;

/**
 * 打字机逐字显示文本(歌词当前行的签名效果),尾随 `.type-caret` 光标。
 * 从 LyricsPanel 提炼,主窗口歌词稿与任务栏歌词条共用。
 */
export function TypewriterText({ text }: { text: string }) {
  const [displayedLength, setDisplayedLength] = useState(0);
  // 超长文本退化为静态显示:动画本身才是卡死根源,不是渲染
  const animated = text.length <= MAX_TYPEWRITER_CHARS;

  useEffect(() => {
    if (!animated) return;
    setDisplayedLength(0);
    if (!text) return;

    const speed = Math.max(30, Math.min(80, 800 / text.length));

    const interval = setInterval(() => {
      setDisplayedLength((prev) => {
        if (prev >= text.length) {
          clearInterval(interval);
          return text.length;
        }
        return prev + 1;
      });
    }, speed);

    return () => clearInterval(interval);
  }, [text, animated]);

  if (!animated) {
    return <span className="type-caret">{text}</span>;
  }

  return <span className="type-caret">{text.slice(0, displayedLength)}</span>;
}
