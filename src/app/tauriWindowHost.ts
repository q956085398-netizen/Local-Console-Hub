import { getCurrentWindow } from "@tauri-apps/api/window";
import type { WindowHost } from "../state/window-controls";

/**
 * How often a desktop window re-reads `isVisible`.
 *
 * Hiding to the tray does not minimize, and it does not reliably deliver a
 * focus or page-visibility event, so a reading that only listens can keep
 * polling a window the user has already closed. One second is inside the
 * port list's own refresh period, which is what that pause has to beat.
 */
const WINDOW_VISIBILITY_SAMPLE_MS = 1_000;

/**
 * How long a close may keep the window "hidden" before `isVisible` agrees.
 *
 * The close handler hides the window, but the focus event can be sampled
 * while the window is still visible, and that one `true` would undo the
 * pause. After the window has actually gone, a later `true` is a show.
 */
const HIDE_SETTLE_MS = 2_000;

/**
 * Whether the main window is visible, including after the Hub hides it on close.
 *
 * The browser preview has no Tauri window, so the page's own visibility is the
 * reading. A desktop window asks `isVisible`. Focus is only a hint: hiding to
 * the tray is not a minimize, and the close that hides is the moment the poll
 * has to stop. The caller decides what to pause.
 */
export function watchMainWindowVisible(receive: (visible: boolean) => void): () => void {
  if (typeof document === "undefined") {
    receive(true);
    return () => {};
  }
  if (!("__TAURI_INTERNALS__" in window)) {
    const read = () => receive(document.visibilityState !== "hidden");
    read();
    document.addEventListener("visibilitychange", read);
    return () => document.removeEventListener("visibilitychange", read);
  }

  const current = getCurrentWindow();
  let stopped = false;
  let hideUntil = 0;
  const unlistens: Array<() => void> = [];
  const publish = (windowVisible: boolean) => {
    if (stopped) return;
    if (windowVisible && Date.now() < hideUntil) {
      receive(false);
      return;
    }
    if (!windowVisible) hideUntil = 0;
    receive(windowVisible && document.visibilityState !== "hidden");
  };
  const read = () => {
    void current
      .isVisible()
      .then((visible) => publish(visible))
      .catch(() => publish(document.visibilityState !== "hidden"));
  };
  const keep = (registered: Promise<() => void>) => {
    void registered
      .then((unlisten) => {
        if (stopped) unlisten();
        else unlistens.push(unlisten);
      })
      .catch(() => {
        // Focus and close listeners are hints. The sample below still reads
        // `isVisible` when the capability refuses one of them.
      });
  };

  read();
  document.addEventListener("visibilitychange", read);
  keep(current.onFocusChanged(() => read()));
  keep(
    current.onCloseRequested(() => {
      // Closing the main window hides it to the tray. Treat that as hidden
      // now, before a late focus reading can call the window visible.
      hideUntil = Date.now() + HIDE_SETTLE_MS;
      if (!stopped) receive(false);
    }),
  );
  const timer = window.setInterval(read, WINDOW_VISIBILITY_SAMPLE_MS);
  return () => {
    stopped = true;
    document.removeEventListener("visibilitychange", read);
    window.clearInterval(timer);
    for (const unlisten of unlistens) unlisten();
  };
}

/**
 * The main window as the desktop app reaches it.
 *
 * The only place that knows both sides: `state/window-controls.ts` decides what a
 * window control means, and `tauri.conf.json` + `capabilities/default.json`
 * declare the window the desktop app actually opens (undecorated, resizable,
 * with the four `core:window:` permissions the controls need). Keeping this
 * adapter this small is the point.
 *
 * ## Why this returns `null` instead of a host
 *
 * The same frontend runs in two places: the desktop app, and a browser preview
 * (`npm run dev` with no Rust host — the fixture workspace the visual capture
 * drives). `getCurrentWindow()` reads `window.__TAURI_INTERNALS__`, so in a
 * browser it throws before it can ask for anything. `null` says "there is no
 * window here to control" in one place, and the title bar renders its controls
 * inert rather than offering buttons that would fail on click.
 *
 * `subscribe` has to return its unlisten synchronously while Tauri's
 * `onResized` resolves asynchronously, so the two orderings are both handled: a
 * view that stops tracking before the listener is registered unsubscribes the
 * moment it arrives (the same shape as `tauriTerminalBackend`).
 */
export function tauriWindowHost(): WindowHost | null {
  if (!("__TAURI_INTERNALS__" in window)) return null;

  const current = getCurrentWindow();

  return {
    minimize: () => current.minimize(),
    toggleMaximize: () => current.toggleMaximize(),
    close: () => current.close(),
    isMaximized: () => current.isMaximized(),

    onResized: (receive) => {
      let unlisten: (() => void) | null = null;
      let stopped = false;
      void current
        .onResized(() => receive())
        .then((registered) => {
          if (stopped) {
            registered();
          } else {
            unlisten = registered;
          }
        });
      return () => {
        stopped = true;
        unlisten?.();
      };
    },
  };
}
