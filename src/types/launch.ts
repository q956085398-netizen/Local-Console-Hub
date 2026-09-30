/**
 * Frontend mirror of the launch layer's window-facing message
 * (`src-tauri/src/app/launch.rs`, #63).
 *
 * A shortcut that asks for a terminal is a launch request, not a tray click,
 * and the window does more for it: the terminal the Hub just made has to end up
 * selected *and* holding the keyboard, or the user clicked their PowerShell
 * entry and got a window showing something else. The tray's
 * `session-focus-requested` (`types/tray.ts`) stays as it was — a request to
 * look at a session.
 *
 * Like every other IPC name in this repository the literal is written out here
 * rather than derived, so a rename on the backend breaks this listener instead
 * of silently stopping it.
 */

/** `session-opened` — a launch request made a terminal and wants the user in it. */
export const SESSION_OPENED = "session-opened";

/** Payload of `session-opened`. */
export interface SessionOpenedDto {
  /** The terminal the launch request created. */
  sessionId: string;
}

/**
 * Runtime guard for a `session-opened` payload.
 *
 * A malformed payload is dropped rather than defaulted, for the same reason
 * `isSessionFocusRequestedDto` drops one: an id read as "undefined" would move
 * the workspace somewhere the user did not ask for.
 */
export function isSessionOpenedDto(value: unknown): value is SessionOpenedDto {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  return typeof (value as Record<string, unknown>).sessionId === "string";
}
