import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isDirectoryScan, type PathKind } from "../types/discovery";

export function useApplicationDiscovery() {
  const pick = useCallback(async (kind: PathKind, cwd: string) => {
    const value = await invoke<unknown>("pick_application_path", { kind, cwd });
    if (value === null || typeof value === "string") return value;
    throw new Error("文件选择器返回了无法识别的路径。");
  }, []);
  const scan = useCallback(async (cwd: string) => {
    const value = await invoke<unknown>("scan_application_directory", { cwd });
    if (isDirectoryScan(value)) return value;
    throw new Error("目录扫描返回了无法识别的结果。");
  }, []);
  return { pick, scan };
}
