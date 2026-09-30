import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isDisplayAdviceDto, type DisplayAdviceDto } from "../types/config";

/**
 * The "添加应用" form's read-only question: what does this launch method look
 * like to the Hub (#66)?
 *
 * Kept out of `useSessionRegistry` on purpose. The registry is about *sessions*
 * — what exists, what they are doing, and the lifecycle commands that change
 * them — and this asks about a command string that has not been saved yet. A
 * separate seam keeps both interfaces honest about what they are for, and it is
 * where the guard for this payload lives.
 *
 * The answer is `null` when the question could not be asked at all (no backend,
 * a payload that is not a `DisplayAdviceDto`). That is not "no recommendation":
 * the form distinguishes them by having asked at all, and a failed lookup must
 * not silently look like a confirmed "no console".
 */
export function useDisplayAdvice(): (
  command: string,
  cwd: string,
) => Promise<DisplayAdviceDto | null> {
  return useCallback(async (command: string, cwd: string) => {
    try {
      const raw = await invoke<unknown>("recommend_display", { command, cwd });
      return isDisplayAdviceDto(raw) ? raw : null;
    } catch {
      return null;
    }
  }, []);
}
