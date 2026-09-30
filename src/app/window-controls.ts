/**
 * The OS window operations the merged title bar owns, injected.
 *
 * Since #68 the main window is undecorated: the dark in-app title bar *is* the
 * window's title bar, so minimize, maximize/restore, close, dragging and
 * resizing have to come from the app instead of from the system. This module
 * owns the two things about that which are decisions rather than wiring:
 *
 * 1. **Every control reaches exactly one operation**, and an operation the OS
 *    refused is reported instead of swallowed — a window control that silently
 *    does nothing is worse than one that says it failed.
 * 2. **The maximize button is a reading, not a toggle.** Whether the glyph is
 *    "maximize" or "restore" is what the window reports, kept current by the
 *    window's own resize events. A local boolean would drift the first time
 *    anything else maximized the window (double-clicking the drag region,
 *    Aero Snap, Win+Up).
 *
 * The window is injected (`WindowHost`) for the same reason the backend ping
 * is (`src/state/backend-connection.ts`): the rules above run in the node test
 * environment, with no DOM and no Tauri host. The desktop app's own host is
 * `src/app/tauriWindowHost.ts`; a browser preview has none, which is why the
 * caller gets `null` rather than a host that throws on use.
 *
 * What this module deliberately does not own: the close *semantics*.
 * `close()` asks the window to close, and the app's close handler hides it to
 * the tray (D-006, `src-tauri/src/tray/mod.rs`) — the same path the system's own
 * X took before the title bars were merged.
 */

/** The three window operations the title bar offers. */
export type WindowControl = "minimize" | "toggle-maximize" | "close";

/**
 * What each control is called where a person reads it.
 *
 * The label is used for the button's accessible name and for the failure
 * message, so "which button was that" is answered once rather than twice.
 */
export const WINDOW_CONTROL_LABELS: Record<WindowControl, string> = {
  minimize: "最小化窗口",
  "toggle-maximize": "最大化或还原窗口",
  close: "关闭窗口",
};

/**
 * The window, as the title bar reaches it.
 *
 * Four commands and one reading — nothing here is a lifecycle claim, and
 * nothing here is about sessions. `onResized` reports that the window changed
 * size and returns its own unsubscribe, so the caller can tie it to a React
 * effect cleanup.
 */
export interface WindowHost {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  onResized(receive: () => void): () => void;
}

/** What happened to one window operation. */
export type WindowControlOutcome = { ok: true } | { ok: false; message: string };

/**
 * Ask the window for one operation, reporting a refusal rather than throwing.
 *
 * A rejection here means the OS did not do it (a permission that was not
 * granted, a window that is already gone). The caller has no local state to
 * repair in that case — the window itself is the truth — so the only useful
 * thing it can do is tell the user, which is what the message is for.
 */
export async function runWindowControl(
  host: WindowHost,
  control: WindowControl,
): Promise<WindowControlOutcome> {
  const label = WINDOW_CONTROL_LABELS[control];
  try {
    switch (control) {
      case "minimize":
        await host.minimize();
        break;
      case "toggle-maximize":
        await host.toggleMaximize();
        break;
      case "close":
        await host.close();
        break;
    }
    return { ok: true };
  } catch (error) {
    return { ok: false, message: `${label}没有生效：${reason(error)}` };
  }
}

/**
 * Report whether the window is maximized, now and on every resize.
 *
 * The first reading is asked for immediately, so the glyph is right the moment
 * the title bar appears rather than on the window's next resize. A read that
 * fails reports nothing: it says the window was not asked, not that it answered
 * "no", and the last reading is a better guess than a flip.
 *
 * Returns the stop function, which unsubscribes and makes every later answer a
 * no-op — a resize event or a slow reading that arrives after the title bar is
 * gone must not report into it.
 */
export function trackMaximized(host: WindowHost, report: (maximized: boolean) => void): () => void {
  let stopped = false;

  const read = () => {
    void host.isMaximized().then(
      (maximized) => {
        if (!stopped) report(maximized);
      },
      () => {
        // See above: a failed read is not a reading.
      },
    );
  };

  const unlisten = host.onResized(read);
  read();

  return () => {
    stopped = true;
    unlisten();
  };
}

/** The reason an operation failed, as text, whatever it arrived as. */
function reason(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return String(error);
}
