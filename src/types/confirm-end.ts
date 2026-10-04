/**
 * Frontend mirror of `confirm_end_occupant` (`src-tauri/src/ipc/confirm_end.rs`).
 *
 * The command ends one process only after this identity is checked again.
 * `ended` is false when nothing was signalled, and `message` then says so.
 */

export const CONFIRM_END_OCCUPANT = "confirm_end_occupant";

/** Pid plus the creation time that was just read. Not a port, and not a name. */
export interface EndTarget {
  pid: number;
  createdAt: string;
}

export interface ConfirmEndResult {
  ended: boolean;
  message: string;
}

export function isConfirmEndResult(value: unknown): value is ConfirmEndResult {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return typeof candidate.ended === "boolean" && typeof candidate.message === "string";
}
