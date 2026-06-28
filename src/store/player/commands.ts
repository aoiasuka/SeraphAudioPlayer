import { invoke } from "@/lib/tauri";

export function sendCommand(cmd: string, args?: Record<string, unknown>) {
  void invoke(cmd, args).catch((err) => {
    // eslint-disable-next-line no-console
    console.warn(`Tauri command failed: ${cmd}`, err);
  });
}

export async function sendCommandAsync(cmd: string, args?: Record<string, unknown>) {
  await invoke(cmd, args);
}
