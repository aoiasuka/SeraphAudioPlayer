import { invoke, normalizeIpcError } from "@/lib/tauri";
import { usePlayerStore } from "@/store/player";

/**
 * 在资源管理器中定位曲目文件。后端会校验路径存在性，
 * 文件已被移动/删除（如失效缓存）时把结构化错误转成通知展示。
 */
export async function revealTrackFile(path: string): Promise<void> {
  const notify = usePlayerStore.getState().showNotification;
  if (!path.trim()) {
    notify("该曲目没有本地文件路径");
    return;
  }
  try {
    await invoke("reveal_in_explorer", { path });
  } catch (err) {
    notify(`定位文件失败: ${normalizeIpcError(err).message}`);
  }
}
