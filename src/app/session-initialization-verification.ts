/** Opt-in desktop acceptance aid; production builds cannot enable it. */
const mode = import.meta.env.DEV ? import.meta.env.VITE_VERIFY_SESSION_INITIALIZATION : undefined;
const enabled = mode === "configs-last" || mode === "runtimes-last";
const SNAPSHOT_DELAY_MS = 30_000;

/** Capture the real IPC result now, but deliver one of the two lists later.
 * Subscription and lifecycle events remain live during the delay, so a tray
 * action must supersede the captured, older state rather than a fresh read.
 */
export async function readVerificationSnapshot(
  kind: "configs" | "runtimes",
  read: () => Promise<unknown>,
): Promise<unknown> {
  const snapshot = await read();
  if (enabled) {
    console.info(`[session initialization acceptance] captured ${kind}`);
    if (mode === `${kind}-last`) {
      console.info(`[session initialization acceptance] delaying ${kind} for 30 seconds`);
      await new Promise<void>((resolve) => setTimeout(resolve, SNAPSHOT_DELAY_MS));
    }
    console.info(`[session initialization acceptance] delivering ${kind}`);
  }
  return snapshot;
}

/** Reload only the frontend; Session Core and its processes stay alive. */
export function installVerificationReload(): (() => void) | undefined {
  if (!enabled) return undefined;
  const reload = (event: KeyboardEvent) => {
    if (event.ctrlKey && event.altKey && event.code === "KeyR" && !event.repeat) {
      event.preventDefault();
      event.stopPropagation();
      window.location.reload();
    }
  };
  window.addEventListener("keydown", reload, true);
  return () => window.removeEventListener("keydown", reload, true);
}
