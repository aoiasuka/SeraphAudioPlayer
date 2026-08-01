import { useEffect, useState } from "react";

/**
 * 打字机逐字显示文本(歌词当前行的签名效果),尾随 `.type-caret` 光标。
 * 从 LyricsPanel 提炼,主窗口歌词稿与任务栏歌词条共用。
 */
export function TypewriterText({ text }: { text: string }) {
  const [displayedLength, setDisplayedLength] = useState(0);

  useEffect(() => {
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
  }, [text]);

  return <span className="type-caret">{text.slice(0, displayedLength)}</span>;
}
