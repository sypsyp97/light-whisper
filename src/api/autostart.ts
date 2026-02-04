import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";

/** 启用开机自启动 */
export async function enableAutostart(): Promise<void> {
  await enable();
}

/** 禁用开机自启动 */
export async function disableAutostart(): Promise<void> {
  await disable();
}

/** 检查是否已启用开机自启动 */
export async function isAutostartEnabled(): Promise<boolean> {
  return await isEnabled();
}
