/**
 * Frontend mirror of the tray's one window-facing message
 * (`src-tauri/src/tray/mod.rs`, T09 #10).
 *
 * The tray is otherwise a backend-only surface: it reads Session Core and calls
 * it, and the window never has to know the tray exists. The exception is a
 * click on a session row — that is a *request to look at something*, which only
 * the window can honour, so it crosses back as an event rather than as a second
 * way of setting lifecycle state.
 *
 * Like every other IPC name in this repository the literal is written out here
 * rather than derived, so a rename on the backend breaks the listener that
 * reads it instead of silently stopping it (`types/runtime.ts`).
 */

/** `session-focus-requested` — the tray asked the window to show a session. */
export const SESSION_FOCUS_REQUESTED = "session-focus-requested";

/** Payload of `session-focus-requested`. */
export interface SessionFocusRequestedDto {
  /** The session the user picked in the tray. */
  sessionId: string;
}

/**
 * Runtime guard for a `session-focus-requested` payload.
 *
 * An id and nothing else: the event carries no lifecycle claim, so a payload
 * with more in it is not a richer message to interpret — it is a payload from
 * something other than the tray, and the window ignores it.
 *
 * A malformed payload is dropped rather than defaulted. The alternative — a
 * missing id read as "session undefined" — would move the selection to whatever
 * the workspace falls back to, which the user did not ask for.
 */
export function isSessionFocusRequestedDto(value: unknown): value is SessionFocusRequestedDto {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  return typeof (value as Record<string, unknown>).sessionId === "string";
}
