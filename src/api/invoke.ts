import { invoke } from "@tauri-apps/api/core";

export function invokeCommand<T>(
  command: string,
  args?: Record<string, unknown>
): Promise<T> {
  return args ? invoke<T>(command, args) : invoke<T>(command);
}
