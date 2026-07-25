import { useEffect } from "react";
import { usePlayerStore } from "@/store/player";

/**
 * 空格键切换播放/暂停（焦点在播放器窗口内时）。
 *
 * 不劫持的目标：文本输入类元素（检索框、歌单重命名等正常打空格）与
 * 弹窗/菜单内的交互（设置弹窗按钮等优先于全局快捷键）。
 * 其余场景 preventDefault——既阻止页面滚动，也阻止「点完播放按钮后焦点
 * 留在按钮上、再按空格被浏览器当作按钮激活」的歧义：此时用户预期是暂停。
 */
function shouldIgnoreSpaceTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target.closest('[role="dialog"], [role="menu"]')) return true;
  return false;
}

export function useSpacePlayback() {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.code !== "Space") return;
      if (event.repeat) return;
      if (event.ctrlKey || event.altKey || event.metaKey || event.shiftKey) return;
      if (shouldIgnoreSpaceTarget(event.target)) return;
      event.preventDefault();
      const store = usePlayerStore.getState();
      if (!store.currentTrack()) return;
      store.togglePlayback();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
