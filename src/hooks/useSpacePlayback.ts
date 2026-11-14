import { useEffect } from "react";
import { usePlayerStore } from "@/store/player";

/**
 * 空格键切换播放/暂停（焦点在播放器窗口内时）。
 *
 * 不劫持的目标：文本输入类元素（检索框、歌单重命名等正常打空格）、
 * 弹窗/菜单内的交互，以及任何获得焦点的可交互控件（按钮、链接、开关、
 * 滑块、标签页…）——这些控件按惯例用空格激活（UI5-02），与沉浸页一致。
 * 焦点在页面空白处或非交互元素上时才作为播放快捷键，并 preventDefault 阻止滚动。
 */
const INTERACTIVE_SELECTOR = [
  "button", "a[href]", "summary", "[contenteditable='true']",
  "[role='button']", "[role='switch']", "[role='checkbox']", "[role='radio']",
  "[role='tab']", "[role='slider']", "[role='option']", "[role='menuitem']", "[role='link']",
].join(",");

function shouldIgnoreSpaceTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  if (target.closest('[role="dialog"], [role="menu"]')) return true;
  if (target.closest(INTERACTIVE_SELECTOR)) return true;
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
