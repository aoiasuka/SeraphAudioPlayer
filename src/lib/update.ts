import { invoke, normalizeIpcError } from "@/lib/tauri";

export interface UpdateCheckResult {
  currentVersion: string;
  latestVersion: string;
  updateAvailable: boolean;
  releaseUrl: string;
  releaseNotes: string | null;
}

/** 检查 GitHub Releases 是否有新版本。失败抛出归一化的 IpcError。 */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  try {
    return await invoke<UpdateCheckResult>("check_for_update");
  } catch (err) {
    throw normalizeIpcError(err);
  }
}

/** 打开 Release 下载页（后端校验 URL 白名单）。 */
export async function openReleasePage(url: string): Promise<void> {
  await invoke("open_release_page", { url });
}
